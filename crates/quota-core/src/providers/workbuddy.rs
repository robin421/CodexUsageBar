//! WorkBuddy (Tencent CodeBuddy desktop agent) quota provider.
//!
//! Reads local desktop sign-in files (`workbuddy-desktop.info` for China build,
//! `workbuddy-desktop-ai.info` for international build) under
//! `CodeBuddyExtension/Data/Public/auth/` and queries WorkBuddy's billing meter APIs.

use std::path::PathBuf;

use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use serde_json::{json, Value};

use crate::ctx::Ctx;
use crate::error::{FetchError, Result};
use crate::model::{Balance, ProviderId, QuotaWindow, Snapshot, WindowKind};

const APP_VERSION: &str = "5.9.3";
const SUMMARY_PATH: &str = "/billing/meter/get-user-resource-summary";
const ENTERPRISE_USAGE_PATH: &str = "/billing/meter/get-enterprise-user-usage";
const PAID_PACKAGES_PATH: &str = "/billing/meter/get-user-resource-paid-packages";
const FREE_PACKAGES_PATH: &str = "/billing/meter/get-user-resource-free-packages";
const REFRESH_PATH: &str = "/v2/plugin/auth/token/refresh";

const PAID_CODES: &[&str] = &[
    "TCACA_code_002_AkiJS3ZHF5",
    "TCACA_code_005_maRGyrHhw1",
    "TCACA_code_003_FAnt7lcmRT",
    "TCACA_code_023_4xbGhMrE6q",
    "TCACA_code_026_BaESVICNoi",
    "TCACA_code_027_0FCGVA6vSa",
    "TCACA_code_043_6oIkxT7Z0S",
    "TCACA_code_009_0XmEQc2xOf",
    "TCACA_code_038_OhvqZtiPKr",
    "TCACA_code_036_lupO5WgNdG",
];

const FREE_CODES: &[&str] = &[
    "TCACA_code_001_PqouKr6QWV",
    "TCACA_code_008_cfWoLwvjU4",
    "TCACA_code_035_ArVxJcGDsm",
    "TCACA_code_006_DbXS0lrypC",
    "TCACA_code_039_KRcQj7wUat",
    "TCACA_code_040_mi9rCYg46x",
    "TCACA_code_007_nzdH5h4Nl0",
    "TCACA_code_028_NtpWi0jzXs",
    "TCACA_code_037_WxOD3MpI2o",
    "TCACA_code_029_6wCGEWquYy",
    "TCACA_code_030_BjSt89qTvr",
];

const PLAN_CODES: &[&str] = &[
    "TCACA_code_002_AkiJS3ZHF5",
    "TCACA_code_005_maRGyrHhw1",
    "TCACA_code_003_FAnt7lcmRT",
    "TCACA_code_023_4xbGhMrE6q",
    "TCACA_code_026_BaESVICNoi",
    "TCACA_code_027_0FCGVA6vSa",
    "TCACA_code_043_6oIkxT7Z0S",
    "TCACA_code_035_ArVxJcGDsm",
    "TCACA_code_039_KRcQj7wUat",
    "TCACA_code_040_mi9rCYg46x",
];

const FREE_MON_CODE: &str = "TCACA_code_008_cfWoLwvjU4";

#[derive(Clone, Copy, Debug)]
struct SiteConfig {
    id: &'static str,
    label: &'static str,
    endpoint: &'static str,
    default_host: &'static str,
    auth_file: &'static str,
    is_ai: bool,
}

const SITES: [SiteConfig; 2] = [
    SiteConfig {
        id: "workbuddy",
        label: "WorkBuddy (CN)",
        endpoint: "https://copilot.tencent.com",
        default_host: "copilot.tencent.com",
        auth_file: "workbuddy-desktop.info",
        is_ai: false,
    },
    SiteConfig {
        id: "workbuddy-ai",
        label: "WorkBuddy AI",
        endpoint: "https://www.workbuddy.ai",
        default_host: "www.workbuddy.ai",
        auth_file: "workbuddy-desktop-ai.info",
        is_ai: true,
    },
];

#[derive(Clone, Debug)]
struct DesktopAuth {
    site: SiteConfig,
    access_token: String,
    refresh_token: Option<String>,
    uid: String,
    display_name: String,
    domain: String,
    enterprise_id: Option<String>,
    is_desktop: bool,
}

enum AuthLoad {
    Found(DesktopAuth),
    Encrypted,
    Missing,
}

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let mut auths = Vec::new();
    let mut saw_encrypted = false;

    for site in SITES {
        match load_desktop_auth(ctx, site) {
            AuthLoad::Found(auth) => auths.push(auth),
            AuthLoad::Encrypted => {
                saw_encrypted = true;
                if let Some(magpie_auth) = load_magpie_auth(ctx, site) {
                    auths.push(magpie_auth);
                }
            }
            AuthLoad::Missing => {
                if let Some(magpie_auth) = load_magpie_auth(ctx, site) {
                    auths.push(magpie_auth);
                }
            }
        }
    }

    if auths.is_empty() {
        return Err(if saw_encrypted {
            FetchError::Unsupported("WorkBuddy desktop credentials are encrypted".into())
        } else {
            FetchError::NotDetected
        });
    }

    let is_multi = auths.len() > 1;
    let mut combined = Snapshot::new(ProviderId::Workbuddy, "workbuddy_desktop");
    let mut account_names = Vec::new();
    let mut plans = Vec::new();
    let mut last_err: Option<FetchError> = None;

    for auth in &mut auths {
        match fetch_account(ctx, auth).await {
            Ok(mut snap) => {
                let acct_label = if is_multi {
                    format!("{} · {}", auth.site.label, auth.display_name)
                } else {
                    auth.display_name.clone()
                };
                account_names.push(acct_label.clone());
                if let Some(plan) = snap.plan.take() {
                    if !plans.contains(&plan) {
                        plans.push(plan);
                    }
                }
                for mut w in snap.windows.drain(..) {
                    if is_multi {
                        w.account = Some(acct_label.clone());
                        w.id = format!("{}-{}", auth.site.id, w.id);
                    }
                    combined.windows.push(w);
                }
                for mut b in snap.balances.drain(..) {
                    if is_multi {
                        b.id = format!("{}-{}", auth.site.id, b.id);
                    }
                    combined.balances.push(b);
                }
            }
            Err(err) => {
                last_err = Some(err);
            }
        }
    }

    if account_names.is_empty() {
        return Err(last_err.unwrap_or_else(|| FetchError::parse("no WorkBuddy quota windows found")));
    }

    combined.account = Some(account_names.join(", "));
    if !plans.is_empty() {
        combined.plan = Some(plans.join(" / "));
    }
    Ok(combined)
}

fn auth_dir(ctx: &Ctx) -> PathBuf {
    let base = if cfg!(target_os = "macos") {
        ctx.home.join("Library/Application Support/CodeBuddyExtension")
    } else if cfg!(windows) {
        ctx.local_app_data().join("CodeBuddyExtension")
    } else {
        ctx.xdg_data().join("CodeBuddyExtension")
    };
    base.join("Data/Public/auth")
}

fn load_desktop_auth(ctx: &Ctx, site: SiteConfig) -> AuthLoad {
    let path = auth_dir(ctx).join(site.auth_file);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return AuthLoad::Missing,
    };
    let v: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return AuthLoad::Missing,
    };

    let auth = v.get("auth");
    let access_val = auth.and_then(|a| a.get("accessToken"));
    if access_val.and_then(|t| t.get("$wbEncrypted")).is_some() {
        return AuthLoad::Encrypted;
    }

    let Some(access_token) = access_val.and_then(|t| t.as_str()).map(str::trim).filter(|s| !s.is_empty()) else {
        return AuthLoad::Missing;
    };
    let acct = v.get("account");
    let Some(uid) = acct
        .and_then(|a| a.get("uid"))
        .and_then(|u| u.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return AuthLoad::Missing;
    };

    let display_name = acct
        .and_then(|a| a.get("nickname").and_then(|n| n.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            acct.and_then(|a| a.get("phoneNumber").and_then(|p| p.as_str()))
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or(uid)
        .to_string();

    let domain = auth
        .and_then(|a| a.get("domain"))
        .and_then(|d| d.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(site.default_host)
        .to_string();

    let enterprise_id = acct
        .and_then(|a| a.get("enterpriseId"))
        .and_then(|e| e.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    AuthLoad::Found(DesktopAuth {
        site,
        access_token: access_token.to_string(),
        refresh_token: None,
        uid: uid.to_string(),
        display_name,
        domain,
        enterprise_id,
        is_desktop: true,
    })
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

fn load_magpie_auth(ctx: &Ctx, site: SiteConfig) -> Option<DesktopAuth> {
    for path in magpie_auth_paths(ctx) {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(entry) = v.get(site.id) else {
            continue;
        };
        if entry.get("type").and_then(|t| t.as_str()) != Some("oauth") {
            continue;
        }
        let Some(access_token) = entry
            .get("access")
            .and_then(|a| a.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let Some(uid) = entry
            .get("uid")
            .and_then(|u| u.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let refresh_token = entry
            .get("refresh")
            .and_then(|r| r.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let display_name = entry
            .get("accountId")
            .and_then(|a| a.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(uid)
            .to_string();
        let domain = entry
            .get("domain")
            .and_then(|d| d.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(site.default_host)
            .to_string();
        let enterprise_id = entry
            .get("enterpriseId")
            .and_then(|e| e.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        return Some(DesktopAuth {
            site,
            access_token: access_token.to_string(),
            refresh_token,
            uid: uid.to_string(),
            display_name,
            domain,
            enterprise_id,
            is_desktop: false,
        });
    }
    None
}

fn is_cloud_hosted_domain(d: &str) -> bool {
    let lower = d.trim().to_ascii_lowercase();
    if lower.is_empty()
        || lower == "tencent.sso.copilot.tencent.com"
        || lower == "tencent.sso.codebuddy.cn"
    {
        return false;
    }
    lower.ends_with(".sso.copilot.tencent.com")
        || lower.ends_with(".copilot.qq.com")
        || lower.ends_with(".sso.codebuddy.cn")
}

fn endpoint_for(auth: &DesktopAuth) -> String {
    if is_cloud_hosted_domain(&auth.domain) {
        format!("https://{}", auth.domain.trim().to_ascii_lowercase())
    } else {
        auth.site.endpoint.to_string()
    }
}

async fn post_meter(ctx: &Ctx, auth: &DesktopAuth, path: &str, body: Value) -> Result<Value> {
    let url = format!("{}{}", endpoint_for(auth), path);
    let mut req = ctx
        .http
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("User-Agent", format!("WorkBuddy/{APP_VERSION}"))
        .header("Authorization", format!("Bearer {}", auth.access_token))
        .header("X-User-Id", &auth.uid)
        .header("X-Domain", &auth.domain)
        .header("X-Product", "SaaS")
        .header("X-IDE-Type", "WorkBuddy");

    if let Some(ref eid) = auth.enterprise_id {
        req = req.header("X-Enterprise-Id", eid).header("X-Tenant-Id", eid);
    }

    let resp = req.json(&body).send().await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(FetchError::AuthExpired);
    }

    let env: Value = serde_json::from_str(&text).map_err(|_| {
        if !status.is_success() {
            FetchError::Http {
                status: status.as_u16(),
                body: text.chars().take(200).collect(),
            }
        } else {
            FetchError::parse("invalid JSON from WorkBuddy billing API")
        }
    })?;

    let code = env
        .get("code")
        .and_then(|c| {
            c.as_i64()
                .or_else(|| c.as_str().and_then(|s| s.parse::<i64>().ok()))
        })
        .unwrap_or(0);

    if !status.is_success() || code != 0 {
        let msg = env
            .get("msg")
            .and_then(|m| m.as_str())
            .unwrap_or("WorkBuddy API error");
        if !status.is_success() {
            return Err(FetchError::Http {
                status: status.as_u16(),
                body: msg.to_string(),
            });
        }
        return Err(FetchError::parse(format!("{msg} (code {code})")));
    }

    Ok(env.get("data").cloned().unwrap_or(Value::Null))
}

async fn refresh_oauth_token(ctx: &Ctx, auth: &mut DesktopAuth) -> Result<()> {
    let Some(ref refresh_tok) = auth.refresh_token else {
        return Err(FetchError::AuthExpired);
    };
    let url = format!("{}{}", endpoint_for(auth), REFRESH_PATH);
    let resp = ctx
        .http
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("User-Agent", format!("WorkBuddy/{APP_VERSION}"))
        .header("X-Refresh-Token", refresh_tok)
        .json(&json!({}))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(FetchError::AuthExpired);
    }

    let env: Value = resp.json().await.map_err(|_| FetchError::AuthExpired)?;
    let code = env
        .get("code")
        .and_then(|c| c.as_i64().or_else(|| c.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(0);
    if code != 0 {
        return Err(FetchError::AuthExpired);
    }
    let data = env.get("data").unwrap_or(&Value::Null);
    let Some(new_access) = data
        .get("accessToken")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Err(FetchError::AuthExpired);
    };
    auth.access_token = new_access.to_string();
    if let Some(new_refresh) = data
        .get("refreshToken")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        auth.refresh_token = Some(new_refresh.to_string());
    }
    Ok(())
}

async fn fetch_account(ctx: &Ctx, auth: &mut DesktopAuth) -> Result<Snapshot> {
    let summary = match post_meter(ctx, auth, SUMMARY_PATH, json!({})).await {
        Ok(s) => s,
        Err(FetchError::AuthExpired) if !auth.is_desktop && auth.refresh_token.is_some() => {
            refresh_oauth_token(ctx, auth).await?;
            post_meter(ctx, auth, SUMMARY_PATH, json!({})).await?
        }
        Err(e) => return Err(e),
    };
    let mut snap = parse_summary(&summary, auth.site.is_ai);

    if snap.windows.is_empty() && auth.enterprise_id.is_some() {
        if let Ok(ent) = post_meter(ctx, auth, ENTERPRISE_USAGE_PATH, json!({})).await {
            if let Some(w) = parse_enterprise_usage(&ent) {
                snap.plan = Some("Enterprise".to_string());
                snap.windows.push(w);
            }
        }
    }

    if !snap.windows.is_empty() {
        let packages = fetch_packages(ctx, auth).await;
        if !packages.is_empty() {
            if let Some((resets_at, window_mins)) = parse_cycle(&summary, &packages, Utc::now()) {
                for w in &mut snap.windows {
                    if w.id == "credits" || w.id == "total-credits" {
                        w.resets_at = Some(resets_at);
                        if let Some(mins) = window_mins {
                            w.window_mins = Some(mins);
                        }
                    }
                }
            }
        }
    }

    Ok(snap)
}

async fn fetch_packages(ctx: &Ctx, auth: &DesktopAuth) -> Vec<Value> {
    let offset = FixedOffset::east_opt(8 * 3600).expect("UTC+8");
    let day = Utc::now().with_timezone(&offset).format("%Y-%m-%d").to_string();

    let paid_req = post_meter(
        ctx,
        auth,
        PAID_PACKAGES_PATH,
        json!({
            "PageNumber": 1,
            "PageSize": 200,
            "Status": [0, 3],
            "PackageCodes": PAID_CODES,
            "NeedRenewInfo": true,
        }),
    );
    let free_req = post_meter(
        ctx,
        auth,
        FREE_PACKAGES_PATH,
        json!({
            "PageNumber": 1,
            "PageSize": 200,
            "Status": [0, 3],
            "PackageCodes": FREE_CODES,
            "SlicePeriodStartTime": format!("{day} 00:00:00"),
            "SlicePeriodEndTime": format!("{day} 23:59:59"),
        }),
    );

    let (paid, free) = tokio::join!(paid_req, free_req);
    let mut out = Vec::new();
    for res in [paid, free] {
        if let Ok(data) = res {
            if let Some(accounts) = data.get("Accounts").and_then(|a| a.as_array()) {
                out.extend(accounts.iter().cloned());
            }
        }
    }
    out
}

fn parse_capacity(v: Option<&Value>) -> f64 {
    match v {
        None | Some(Value::Null) => 0.0,
        Some(Value::Number(n)) => n.as_f64().filter(|f| f.is_finite()).unwrap_or(0.0),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                0.0
            } else {
                trimmed.parse::<f64>().ok().filter(|f| f.is_finite()).unwrap_or(0.0)
            }
        }
        _ => 0.0,
    }
}

fn has_total_capacity(v: &Value) -> bool {
    match v.get("CycleTotalCapacity") {
        None | Some(Value::Null) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(_)) => true,
        _ => false,
    }
}

fn is_plan_package(summary: &Value, code: Option<&str>, is_ai: bool) -> bool {
    let Some(code) = code.filter(|s| !s.is_empty()) else {
        return false;
    };
    if summary
        .get("SubscriptionPackageCode")
        .and_then(|s| s.as_str())
        == Some(code)
    {
        return true;
    }
    PLAN_CODES.contains(&code) || (!is_ai && code == FREE_MON_CODE)
}

fn make_credit_window(id: &str, scope: &str, used: f64, total: f64) -> QuotaWindow {
    let used_percent = if total > 0.0 {
        ((used / total) * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    QuotaWindow::new(id, None, used_percent)
        .kind(WindowKind::Monthly)
        .scope(scope)
        .amounts(used, total, "credits")
}

pub fn parse_summary(summary: &Value, is_ai: bool) -> Snapshot {
    let mut snap = Snapshot::new(ProviderId::Workbuddy, "workbuddy_desktop");
    let is_paid = summary
        .get("IsPaidUser")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    snap.plan = Some(if is_paid { "Pro".into() } else { "Free plan".into() });

    let mut plan_total = 0.0;
    let mut plan_used = 0.0;
    let mut extra_total = 0.0;
    let mut extra_used = 0.0;
    let mut prepaid_left = 0.0;
    let mut bags = 0usize;

    let groups: Vec<&Value> = summary
        .get("CreditGroups")
        .and_then(|g| g.as_array())
        .map(|arr| arr.iter().filter(|g| has_total_capacity(g)).collect())
        .unwrap_or_default();

    for g in &groups {
        let total = parse_capacity(g.get("CycleTotalCapacity"));
        let used = parse_capacity(g.get("CycleUsedCapacity"));
        if g.get("Kind").and_then(|k| k.as_str()) == Some("plan_base") {
            plan_total += total;
            plan_used += used;
        } else {
            extra_total += total;
            extra_used += used;
        }
    }

    if let Some(packages) = summary.get("Packages").and_then(|p| p.as_array()) {
        for p in packages {
            if has_total_capacity(p) {
                if groups.is_empty() {
                    let total = parse_capacity(p.get("CycleTotalCapacity"));
                    let used = parse_capacity(p.get("CycleUsedCapacity"));
                    let code = p.get("PackageCode").and_then(|c| c.as_str());
                    if is_plan_package(summary, code, is_ai) {
                        plan_total += total;
                        plan_used += used;
                    } else {
                        extra_total += total;
                        extra_used += used;
                    }
                }
            } else if let Some(rem) = p.get("CycleRemainCapacity") {
                let non_empty = match rem {
                    Value::String(s) => !s.is_empty(),
                    Value::Number(_) => true,
                    _ => false,
                };
                if non_empty {
                    prepaid_left += parse_capacity(Some(rem));
                    bags += 1;
                }
            }
        }
    }

    if plan_total > 0.0 && extra_total > 0.0 {
        snap.windows
            .push(make_credit_window("credits", "Credits", plan_used, plan_total));
        snap.windows.push(make_credit_window(
            "extra-credits",
            "Extra credits",
            extra_used,
            extra_total,
        ));
        snap.windows.push(make_credit_window(
            "total-credits",
            "Total credits",
            plan_used + extra_used,
            plan_total + extra_total,
        ));
    } else {
        let total = plan_total + extra_total;
        let used = plan_used + extra_used;
        if total > 0.0 {
            snap.windows
                .push(make_credit_window("credits", "Credits", used, total));
        }
    }

    if bags > 0 && (plan_total + extra_total == 0.0 || prepaid_left > 0.0) {
        snap.balances.push(Balance {
            id: "prepaid-credits".into(),
            amount: prepaid_left,
            currency: "credits".into(),
            total: None,
            unlimited: false,
        });
    }

    snap
}

pub fn parse_enterprise_usage(data: &Value) -> Option<QuotaWindow> {
    let limit = parse_capacity(data.get("limitNum"));
    if limit <= 0.0 {
        return None;
    }
    let used = parse_capacity(data.get("credit"));
    let mut w = make_credit_window("credits", "Credits", used, limit);

    let reset = data
        .get("cycleResetTime")
        .and_then(|v| v.as_str())
        .and_then(parse_beijing_time);
    let start = data
        .get("cycleStartTime")
        .and_then(|v| v.as_str())
        .and_then(parse_beijing_time);

    if let Some(r) = reset {
        w.resets_at = Some(r);
        if let Some(s) = start {
            if r > s {
                let mins = (r - s).num_minutes().max(1) as u32;
                w.window_mins = Some(mins);
            }
        }
    }
    Some(w)
}

fn parse_beijing_time(s: &str) -> Option<DateTime<Utc>> {
    let trimmed = s.trim();
    let naive = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S").ok()?;
    let offset = FixedOffset::east_opt(8 * 3600)?;
    let dt = offset.from_local_datetime(&naive).single()?;
    Some(dt.with_timezone(&Utc))
}

pub fn parse_cycle(
    summary: &Value,
    packages: &[Value],
    now: DateTime<Utc>,
) -> Option<(DateTime<Utc>, Option<u32>)> {
    let sub_code = summary
        .get("SubscriptionPackageCode")
        .and_then(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let extract = |p: &Value| -> Option<(DateTime<Utc>, Option<u32>)> {
        let end = p
            .get("CycleEndTime")
            .and_then(|v| v.as_str())
            .and_then(parse_beijing_time)?;
        if end <= now {
            return None;
        }
        let start = p
            .get("CycleStartTime")
            .and_then(|v| v.as_str())
            .and_then(parse_beijing_time);
        let mins = start.and_then(|s| {
            if s < end {
                Some((end - s).num_minutes().max(1) as u32)
            } else {
                None
            }
        });
        Some((end, mins))
    };

    if let Some(code) = sub_code {
        for p in packages {
            if p.get("PackageCode").and_then(|c| c.as_str()) == Some(code) {
                return extract(p);
            }
        }
        return None;
    }

    for p in packages {
        let is_slice = p
            .get("CapacityType")
            .and_then(|t| {
                t.as_str()
                    .map(|s| s == "4")
                    .or_else(|| t.as_i64().map(|n| n == 4))
            })
            .unwrap_or(false);
        if is_slice {
            if let Some(res) = extract(p) {
                return Some(res);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;

    #[test]
    fn parse_summary_splits_plan_and_extra_credit_groups() {
        let summary = json!({
            "IsPaidUser": true,
            "CreditGroups": [
                {
                    "Kind": "plan_base",
                    "CycleTotalCapacity": "500",
                    "CycleUsedCapacity": "125.5"
                },
                {
                    "Kind": "addon",
                    "CycleTotalCapacity": "200",
                    "CycleUsedCapacity": "50"
                }
            ]
        });

        let snap = parse_summary(&summary, false);
        assert_eq!(snap.plan.as_deref(), Some("Pro"));
        assert_eq!(snap.windows.len(), 3);

        assert_eq!(snap.windows[0].id, "credits");
        assert_eq!(snap.windows[0].scope.as_deref(), Some("Credits"));
        assert!((snap.windows[0].used_percent - 25.1).abs() < 0.01);

        assert_eq!(snap.windows[1].id, "extra-credits");
        assert_eq!(snap.windows[1].scope.as_deref(), Some("Extra credits"));
        assert!((snap.windows[1].used_percent - 25.0).abs() < 0.01);

        assert_eq!(snap.windows[2].id, "total-credits");
        assert_eq!(snap.windows[2].scope.as_deref(), Some("Total credits"));
        assert!((snap.windows[2].used_percent - 25.07).abs() < 0.05);
        assert_eq!(snap.windows[2].limit, Some(700.0));
    }

    #[test]
    fn parse_summary_handles_free_plan_and_prepaid_balance() {
        let summary = json!({
            "IsPaidUser": false,
            "Packages": [
                {
                    "PackageCode": "TCACA_code_008_cfWoLwvjU4",
                    "CycleTotalCapacity": "500",
                    "CycleUsedCapacity": "100"
                },
                {
                    "PackageCode": "TCACA_prepaid",
                    "CycleRemainCapacity": "42.5"
                }
            ]
        });

        let snap = parse_summary(&summary, false);
        assert_eq!(snap.plan.as_deref(), Some("Free plan"));
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].id, "credits");
        assert!((snap.windows[0].used_percent - 20.0).abs() < 0.01);
        assert_eq!(snap.balances.len(), 1);
        assert!((snap.balances[0].amount - 42.5).abs() < 0.01);
    }

    #[test]
    fn parse_enterprise_usage_parses_limits_and_beijing_times() {
        let data = json!({
            "limitNum": "1000",
            "credit": "250",
            "cycleStartTime": "2026-11-01 00:00:00",
            "cycleResetTime": "2026-12-01 00:00:00"
        });

        let w = parse_enterprise_usage(&data).expect("should parse enterprise usage");
        assert_eq!(w.scope.as_deref(), Some("Credits"));
        assert!((w.used_percent - 25.0).abs() < 0.01);
        assert_eq!(w.used, Some(250.0));
        assert_eq!(w.limit, Some(1000.0));
        assert_eq!(w.window_mins, Some(30 * 24 * 60));
        assert!(w.resets_at.is_some());
    }

    #[test]
    fn parse_cycle_uses_subscription_or_slice_package() {
        let now = Utc.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).unwrap();
        let summary = json!({ "SubscriptionPackageCode": "TCACA_code_002_AkiJS3ZHF5" });
        let packages = vec![json!({
            "PackageCode": "TCACA_code_002_AkiJS3ZHF5",
            "CycleStartTime": "2026-11-01 00:00:00",
            "CycleEndTime": "2026-12-01 00:00:00"
        })];

        let (reset, mins) = parse_cycle(&summary, &packages, now).expect("should find sub cycle");
        assert_eq!(mins, Some(30 * 24 * 60));
        assert!(reset > now);

        let free_summary = json!({ "SubscriptionPackageCode": "" });
        let free_packages = vec![json!({
            "CapacityType": 4,
            "CycleStartTime": "2026-11-01 00:00:00",
            "CycleEndTime": "2026-12-01 00:00:00"
        })];
        let (free_reset, free_mins) =
            parse_cycle(&free_summary, &free_packages, now).expect("should fallback to slice");
        assert_eq!(free_mins, Some(30 * 24 * 60));
        assert!(free_reset > now);
    }

    #[tokio::test]
    async fn fetch_returns_not_detected_when_no_auth_files_exist() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = Ctx::with(dir.path().to_path_buf(), HashMap::new(), HashMap::new(), OsString::new());
        let err = fetch(&ctx).await.expect_err("should fail with NotDetected");
        assert_eq!(err, FetchError::NotDetected);
    }

    #[tokio::test]
    async fn fetch_returns_unsupported_when_tokens_are_encrypted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = Ctx::with(dir.path().to_path_buf(), HashMap::new(), HashMap::new(), OsString::new());
        let auth_path = auth_dir(&ctx).join("workbuddy-desktop.info");
        std::fs::create_dir_all(auth_path.parent().unwrap()).unwrap();
        std::fs::write(
            &auth_path,
            r#"{"auth":{"accessToken":{"$wbEncrypted":1,"envelope":"abc"}},"account":{"uid":"u1"}}"#,
        )
        .unwrap();

        let err = fetch(&ctx).await.expect_err("should fail with Unsupported");
        assert!(matches!(err, FetchError::Unsupported(_)));
    }

    #[test]
    fn load_magpie_auth_parses_cn_and_ai_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = Ctx::with(dir.path().to_path_buf(), HashMap::new(), HashMap::new(), OsString::new());
        let magpie_path = dir.path().join(".config/magpie/plugin-auth.json");
        std::fs::create_dir_all(magpie_path.parent().unwrap()).unwrap();
        std::fs::write(
            &magpie_path,
            r#"{
                "workbuddy": {
                    "type": "oauth",
                    "access": "cn-access",
                    "refresh": "cn-refresh",
                    "uid": "cn-uid",
                    "accountId": "cn-user",
                    "domain": "copilot.tencent.com"
                },
                "workbuddy-ai": {
                    "type": "oauth",
                    "access": "ai-access",
                    "refresh": "ai-refresh",
                    "uid": "ai-uid",
                    "accountId": "ai-user",
                    "domain": "www.workbuddy.ai",
                    "enterpriseId": "ent-1"
                }
            }"#,
        )
        .unwrap();

        let cn = load_magpie_auth(&ctx, SITES[0]).expect("should load CN magpie auth");
        assert_eq!(cn.access_token, "cn-access");
        assert_eq!(cn.refresh_token.as_deref(), Some("cn-refresh"));
        assert_eq!(cn.uid, "cn-uid");
        assert_eq!(cn.display_name, "cn-user");
        assert!(!cn.is_desktop);

        let ai = load_magpie_auth(&ctx, SITES[1]).expect("should load AI magpie auth");
        assert_eq!(ai.access_token, "ai-access");
        assert_eq!(ai.enterprise_id.as_deref(), Some("ent-1"));
        assert!(!ai.is_desktop);
    }
}
