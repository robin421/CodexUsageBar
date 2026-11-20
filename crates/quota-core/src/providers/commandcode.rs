//! Command Code (`commandcode.ai`) quota provider.
//!
//! Reads credentials from `~/.commandcode/auth.json` and/or
//! `$XDG_CONFIG_HOME/magpie/plugin-auth.json` (`~/.config/magpie/plugin-auth.json`),
//! and queries `/alpha/billing/credits`, `/alpha/billing/subscriptions`, and `/alpha/whoami`.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::ctx::{send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{Balance, ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{num, parse_time, pct, str_at};

const API_BASE: &str = "https://api.commandcode.ai";
const FIVE_HOUR_MINS: u32 = 5 * 60;
const WEEKLY_MINS: u32 = 7 * 24 * 60;
const MONTH_MINS: u32 = 30 * 24 * 60;

/// `(prefix, display_name, monthly_credits, is_usage_value_units)`
/// Longest prefix first, matching `opencode-commandcode-auth/index.mjs`.
const PLAN_NAMES: &[(&str, &str, f64, bool)] = &[
    ("individual-provider", "Provider", 0.0, false),
    ("individual-pro-v1", "Pro", 80.0, true),
    ("individual-goat", "GOAT", 70.0, true),
    ("individual-ultra", "Ultra", 300.0, false),
    ("individual-max", "Max", 150.0, false),
    ("individual-pro", "Pro", 30.0, true),
    ("individual-go", "Go", 10.0, true),
    ("teams-pro", "Teams Pro", 40.0, false),
];

#[derive(Clone, Debug)]
struct CcCreds {
    key: String,
    account_hint: Option<String>,
    plan_hint: Option<String>,
    source: &'static str,
}

#[derive(Clone, Debug, Default)]
struct SubInfo {
    plan_id: Option<String>,
    plan_name: Option<String>,
    until: Option<DateTime<Utc>>,
}

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let creds = load_creds(ctx)?;

    let res = fetch_with_creds(ctx, &creds).await;
    match res {
        Ok(snap) => Ok(snap),
        Err(FetchError::Http { status: 401 | 403, .. }) => {
            if let Some(reloaded) = reload_cli_key(ctx, &creds.key) {
                let mut next = creds.clone();
                next.key = reloaded;
                match fetch_with_creds(ctx, &next).await {
                    Ok(snap) => return Ok(snap),
                    Err(FetchError::Http { status: 401 | 403, .. }) => {
                        return Err(FetchError::NotLoggedIn);
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(FetchError::NotLoggedIn)
        }
        Err(e) => Err(e),
    }
}

async fn fetch_with_creds(ctx: &Ctx, creds: &CcCreds) -> Result<Snapshot> {
    let credits_req = send_json(
        ctx.get(&format!("{API_BASE}/alpha/billing/credits"))
            .header("User-Agent", "node")
            .header("Authorization", format!("Bearer {}", creds.key)),
    );
    let subs_req = send_json(
        ctx.get(&format!("{API_BASE}/alpha/billing/subscriptions"))
            .header("User-Agent", "node")
            .header("Authorization", format!("Bearer {}", creds.key)),
    );
    let whoami_req = async {
        if creds.account_hint.is_some() {
            None
        } else {
            send_json(
                ctx.get(&format!("{API_BASE}/alpha/whoami"))
                    .header("User-Agent", "node")
                    .header("Authorization", format!("Bearer {}", creds.key)),
            )
            .await
            .ok()
        }
    };

    let (credits_res, subs_res, whoami_opt) = tokio::join!(credits_req, subs_req, whoami_req);
    let credits_val = credits_res?;
    let sub_info = subs_res.ok().as_ref().and_then(parse_subscription);
    let whoami_account = whoami_opt
        .as_ref()
        .and_then(|v| v.get("user"))
        .and_then(|u| str_at(u, &["email", "userName", "id"]))
        .map(str::to_string);

    let mut snap = parse_allowance(&credits_val, sub_info.as_ref(), creds.source);
    if snap.plan.is_none() {
        snap.plan = creds.plan_hint.clone();
    }
    snap.account = whoami_account.or_else(|| creds.account_hint.clone());
    Ok(snap)
}

fn reload_cli_key(ctx: &Ctx, old_key: &str) -> Option<String> {
    let path = ctx.home.join(".commandcode/auth.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let k = str_at(&v, &["apiKey", "accessToken"])?;
    if k != old_key {
        Some(k.to_string())
    } else {
        None
    }
}

fn magpie_auth_paths(ctx: &Ctx) -> Vec<PathBuf> {
    let xdg = ctx.xdg_config().join("magpie/plugin-auth.json");
    let dot_config = ctx.home.join(".config/magpie/plugin-auth.json");
    if xdg == dot_config {
        vec![xdg]
    } else {
        vec![xdg, dot_config]
    }
}

fn load_creds(ctx: &Ctx) -> Result<CcCreds> {
    let mut saw_dir_or_entry = false;

    let cc_dir = ctx.home.join(".commandcode");
    let cli_path = cc_dir.join("auth.json");
    let cli_creds = if let Ok(raw) = std::fs::read_to_string(&cli_path) {
        saw_dir_or_entry = true;
        serde_json::from_str::<Value>(&raw).ok().and_then(|v| {
            let key = str_at(&v, &["apiKey", "accessToken"])?.to_string();
            let account_hint = str_at(&v, &["email", "userName", "userId"]).map(str::to_string);
            Some(CcCreds {
                key,
                account_hint,
                plan_hint: None,
                source: "commandcode_cli",
            })
        })
    } else {
        if cc_dir.exists() {
            saw_dir_or_entry = true;
        }
        None
    };

    let magpie_creds = load_magpie_creds(ctx, &mut saw_dir_or_entry);

    match (cli_creds, magpie_creds) {
        (Some(mut cli), Some(magpie)) => {
            if cli.account_hint.is_none() {
                cli.account_hint = magpie.account_hint;
            }
            if cli.plan_hint.is_none() {
                cli.plan_hint = magpie.plan_hint;
            }
            Ok(cli)
        }
        (Some(cli), None) => Ok(cli),
        (None, Some(magpie)) => Ok(magpie),
        (None, None) => {
            if saw_dir_or_entry {
                Err(FetchError::NotLoggedIn)
            } else {
                Err(FetchError::NotDetected)
            }
        }
    }
}

fn load_magpie_creds(ctx: &Ctx, saw_entry: &mut bool) -> Option<CcCreds> {
    for path in magpie_auth_paths(ctx) {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(root) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(entry) = root.get("commandcode-plan") else {
            continue;
        };
        *saw_entry = true;

        let item = match entry {
            Value::Array(arr) => arr.first()?,
            Value::Object(_) => entry,
            _ => continue,
        };

        let key = str_at(item, &["key", "access"])?;
        let meta = item.get("metadata");
        let account_hint = meta
            .and_then(|m| str_at(m, &["email", "userName", "userId"]))
            .or_else(|| str_at(item, &["accountId"]))
            .map(str::to_string);
        let plan_hint = meta
            .and_then(|m| str_at(m, &["plan"]))
            .map(str::to_string);

        return Some(CcCreds {
            key: key.to_string(),
            account_hint,
            plan_hint,
            source: "magpie_api",
        });
    }
    None
}

fn plan_of(id: Option<&str>) -> (&'static str, f64, bool) {
    let Some(id) = id else {
        return ("", 0.0, false);
    };
    let s = id.trim().to_lowercase().replace('_', "-");
    for &(prefix, name, monthly, is_units) in PLAN_NAMES {
        if s.starts_with(prefix) {
            return (name, monthly, is_units);
        }
    }
    ("", 0.0, false)
}

fn parse_subscription(v: &Value) -> Option<SubInfo> {
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let d = v.get("data")?;
    let plan_id = str_at(d, &["planId"]).map(str::to_string)?;
    let status = str_at(d, &["status"]).unwrap_or("");
    if status == "canceled" || status == "incomplete_expired" {
        return Some(SubInfo {
            plan_id: None,
            plan_name: Some("No plan".to_string()),
            until: None,
        });
    }
    let (mapped, _, _) = plan_of(Some(&plan_id));
    let plan_name = if mapped.is_empty() {
        plan_id.clone()
    } else {
        mapped.to_string()
    };
    let until = d.get("currentPeriodEnd").and_then(parse_time);
    Some(SubInfo {
        plan_id: Some(plan_id),
        plan_name: Some(plan_name),
        until,
    })
}

fn parse_allowance(v: &Value, sub: Option<&SubInfo>, source: &str) -> Snapshot {
    let mut snap = Snapshot::new(ProviderId::Commandcode, source);

    if let Some(wl) = v.get("windowLimits") {
        for (id, scope, mins, kind, key) in [
            ("five_hour", "5 hours", FIVE_HOUR_MINS, WindowKind::Session, "fiveHour"),
            ("weekly", "Weekly", WEEKLY_MINS, WindowKind::Weekly, "weekly"),
        ] {
            if let Some(x) = wl.get(key) {
                let used = x.get("used").and_then(num);
                let cap = x.get("cap").and_then(num);
                if let (Some(u), Some(c)) = (used, cap) {
                    if c > 0.0 {
                        let reset = x.get("resetAt").and_then(parse_time);
                        let p = pct(u.max(0.0), c).min(100.0);
                        snap.windows.push(
                            QuotaWindow::new(id, Some(mins), p)
                                .kind(kind)
                                .scope(scope)
                                .resets_at(reset),
                        );
                    }
                }
            }
        }
    }

    let credits_obj = v.get("credits");
    let raw_plan_id = credits_obj
        .and_then(|c| str_at(c, &["planId"]))
        .or_else(|| sub.and_then(|s| s.plan_id.as_deref()));

    let (mapped_name, plan_monthly, is_units) = plan_of(raw_plan_id);
    if let Some(sub_plan) = sub.and_then(|s| s.plan_name.as_ref()) {
        snap.plan = Some(sub_plan.clone());
    } else if !mapped_name.is_empty() {
        snap.plan = Some(mapped_name.to_string());
    }

    let mut monthly = 0.0_f64;
    let mut left = 0.0_f64;
    let mut known = false;
    if let Some(c) = credits_obj {
        for (idx, field) in ["monthlyCredits", "purchasedCredits", "freeCredits"]
            .iter()
            .enumerate()
        {
            if let Some(n) = c.get(*field).and_then(num) {
                let n = n.max(0.0);
                if idx == 0 {
                    monthly = n;
                }
                left += n;
                known = true;
            }
        }
    }

    let until = sub.and_then(|s| s.until);
    if known && plan_monthly > 0.0 && is_units {
        let pool = plan_monthly.max(monthly);
        let used = (pool - monthly).max(0.0);
        let p = pct(used, pool).min(100.0);
        let win = QuotaWindow::new("credits", Some(MONTH_MINS), p)
            .kind(WindowKind::Monthly)
            .scope("Credits")
            .amounts(used, pool, "credits")
            .resets_at(until);
        let idx = snap
            .windows
            .iter()
            .position(|w| w.id == "weekly")
            .unwrap_or(snap.windows.len());
        snap.windows.insert(idx, win);

        let extra = left - monthly;
        if extra > 0.0 {
            snap.balances.push(Balance {
                id: "extra".to_string(),
                amount: extra,
                currency: "USD".to_string(),
                total: None,
                unlimited: false,
            });
        }
    } else if known && plan_monthly > 0.0 {
        let pool = plan_monthly.max(monthly) + left - monthly;
        let used = (pool - left).max(0.0);
        let p = pct(used, pool).min(100.0);
        let win = QuotaWindow::new("credits", Some(MONTH_MINS), p)
            .kind(WindowKind::Monthly)
            .scope("Credits")
            .amounts(used, pool, "USD")
            .resets_at(until);
        let idx = snap
            .windows
            .iter()
            .position(|w| w.id == "weekly")
            .unwrap_or(snap.windows.len());
        snap.windows.insert(idx, win);
    } else if known && snap.windows.is_empty() {
        snap.balances.push(Balance {
            id: "credits".to_string(),
            amount: left,
            currency: "USD".to_string(),
            total: None,
            unlimited: false,
        });
    }

    snap
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use tempfile::TempDir;

    fn test_ctx(home: &std::path::Path) -> Ctx {
        Ctx::with(home.to_path_buf(), HashMap::new(), HashMap::new(), OsString::new())
    }

    #[test]
    fn not_detected_without_credentials() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(dir.path());
        let err = load_creds(&ctx).unwrap_err();
        assert!(matches!(err, FetchError::NotDetected));
    }

    #[test]
    fn loads_cli_and_magpie_plugin_auth() {
        let dir = TempDir::new().unwrap();
        let magpie_dir = dir.path().join(".config/magpie");
        std::fs::create_dir_all(&magpie_dir).unwrap();
        std::fs::write(
            magpie_dir.join("plugin-auth.json"),
            serde_json::to_string(&json!({
                "commandcode-plan": {
                    "type": "api",
                    "key": "cc_test_key",
                    "metadata": {
                        "email": "coder@example.com",
                        "plan": "GOAT"
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let ctx = test_ctx(dir.path());
        let creds = load_creds(&ctx).unwrap();
        assert_eq!(creds.key, "cc_test_key");
        assert_eq!(creds.account_hint.as_deref(), Some("coder@example.com"));
        assert_eq!(creds.plan_hint.as_deref(), Some("GOAT"));
    }

    #[test]
    fn parses_multi_window_and_goat_credits() {
        let credits = json!({
            "windowLimits": {
                "fiveHour": { "used": 3.5, "cap": 14, "resetAt": 1796083200 },
                "weekly": { "used": "17.5", "cap": "35", "resetAt": "2026-11-25T00:00:00Z" }
            },
            "credits": {
                "planId": "individual_goat_monthly",
                "monthlyCredits": 53.6,
                "purchasedCredits": 5.0,
                "freeCredits": 0
            }
        });
        let sub = SubInfo {
            plan_id: Some("individual-goat".into()),
            plan_name: Some("GOAT".into()),
            until: DateTime::parse_from_rfc3339("2026-12-01T00:00:00Z")
                .ok()
                .map(|d| d.with_timezone(&Utc)),
        };

        let snap = parse_allowance(&credits, Some(&sub), "magpie_api");
        assert_eq!(snap.provider, ProviderId::Commandcode);
        assert_eq!(snap.plan.as_deref(), Some("GOAT"));
        assert_eq!(snap.windows.len(), 3);
        assert_eq!(snap.windows[0].id, "five_hour");
        assert_eq!(snap.windows[0].kind, WindowKind::Session);
        assert!((snap.windows[0].used_percent - 25.0).abs() < 1e-6);

        assert_eq!(snap.windows[1].id, "credits");
        assert_eq!(snap.windows[1].kind, WindowKind::Monthly);
        assert_eq!(snap.windows[1].limit, Some(70.0));
        assert!((snap.windows[1].used.unwrap() - 16.4).abs() < 1e-6);
        assert_eq!(snap.windows[1].unit.as_deref(), Some("credits"));

        assert_eq!(snap.windows[2].id, "weekly");
        assert_eq!(snap.windows[2].kind, WindowKind::Weekly);
        assert!((snap.windows[2].used_percent - 50.0).abs() < 1e-6);

        assert_eq!(snap.balances.len(), 1);
        assert_eq!(snap.balances[0].amount, 5.0);
        assert_eq!(snap.balances[0].currency, "USD");
    }

    #[test]
    fn parses_pro_and_go_and_ultra_plans() {
        let go = parse_allowance(
            &json!({
                "credits": { "planId": "individual-go", "monthlyCredits": 4, "purchasedCredits": 0, "freeCredits": 0 }
            }),
            None,
            "commandcode_cli",
        );
        assert_eq!(go.plan.as_deref(), Some("Go"));
        assert_eq!(go.windows[0].limit, Some(10.0));
        assert_eq!(go.windows[0].used, Some(6.0));

        let pro = parse_allowance(
            &json!({
                "credits": { "planId": "individual-pro", "monthlyCredits": 15, "purchasedCredits": 0, "freeCredits": 0 }
            }),
            None,
            "commandcode_cli",
        );
        assert_eq!(pro.plan.as_deref(), Some("Pro"));
        assert_eq!(pro.windows[0].limit, Some(30.0));

        let ultra = parse_allowance(
            &json!({
                "credits": { "planId": "individual-ultra", "monthlyCredits": 200, "purchasedCredits": 50, "freeCredits": 0 }
            }),
            None,
            "commandcode_cli",
        );
        assert_eq!(ultra.plan.as_deref(), Some("Ultra"));
        assert_eq!(ultra.windows[0].limit, Some(350.0));
        assert_eq!(ultra.windows[0].used, Some(100.0));
        assert_eq!(ultra.windows[0].unit.as_deref(), Some("USD"));
    }
}
