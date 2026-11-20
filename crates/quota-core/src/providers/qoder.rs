//! Qoder / Qoder CN (`qoder.rs`) quota provider.
//!
//! Checks `ctx.api_key(ProviderId::Qoder)` first (Cookie or Bearer Token from Settings),
//! then falls back to `~/.qoder/.auth/user` (international) and `~/.qoder-cn/.auth/user` (CN).

use serde_json::Value;

use crate::ctx::{send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{num_at, pct, str_at, time_at};

const INTL_URLS: &[&str] = &[
    "https://qoder.com/sash/api/v1/me/campaigns",
    "https://openapi.qoder.sh/api/v1/me/credits",
];
const CN_URLS: &[&str] = &[
    "https://qoder.cn/sash/api/v1/me/campaigns",
    "https://openapi.qoder.com.cn/api/v1/me/credits",
];

#[derive(Clone, Debug, PartialEq, Eq)]
enum QoderSite {
    Intl,
    Cn,
    Auto,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct QoderCreds {
    auth: String,
    site: QoderSite,
    account_hint: Option<String>,
    source: &'static str,
}

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let creds = load_creds(ctx)?;
    let urls: Vec<&str> = match creds.site {
        QoderSite::Intl => INTL_URLS.to_vec(),
        QoderSite::Cn => CN_URLS.to_vec(),
        QoderSite::Auto => INTL_URLS.iter().chain(CN_URLS.iter()).copied().collect(),
    };

    let is_cookie = creds.auth.contains('=')
        && (creds.auth.contains(';') || creds.auth.to_lowercase().starts_with("cookie:"));
    let cookie_val = creds
        .auth
        .strip_prefix("Cookie:")
        .or_else(|| creds.auth.strip_prefix("cookie:"))
        .unwrap_or(&creds.auth)
        .trim();
    let bearer_val = creds
        .auth
        .strip_prefix("Bearer ")
        .or_else(|| creds.auth.strip_prefix("bearer "))
        .unwrap_or(&creds.auth)
        .trim();

    let mut last_err = None;
    for url in urls {
        let mut req = ctx.get(url);
        req = if is_cookie {
            req.header("Cookie", cookie_val)
        } else {
            req.header("Authorization", format!("Bearer {bearer_val}"))
        };
        match send_json(req).await {
            Ok(v) => {
                let mut snap = parse_qoder(&v, creds.source);
                if snap.account.is_none() {
                    snap.account = creds.account_hint.clone();
                }
                return Ok(snap);
            }
            Err(e @ FetchError::Http { status: 401 | 403, .. }) => {
                last_err = Some(e);
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or(FetchError::MissingKey))
}

fn load_creds(ctx: &Ctx) -> Result<QoderCreds> {
    if let Some(key) = ctx.api_key(ProviderId::Qoder) {
        return Ok(QoderCreds {
            auth: key,
            site: QoderSite::Auto,
            account_hint: None,
            source: "api_key",
        });
    }

    let mut saw_encrypted = false;
    for (rel, site, source) in [
        (".qoder/.auth/user", QoderSite::Intl, "qoder_local"),
        (".qoder-cn/.auth/user", QoderSite::Cn, "qoder_cn_local"),
    ] {
        let path = ctx.home.join(rel);
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            if let Some(tok) = str_at(&v, &["accessToken", "access_token", "token", "cookie"]) {
                let account_hint = str_at(&v, &["email", "username", "userName", "name"]).map(str::to_string);
                return Ok(QoderCreds {
                    auth: tok.to_string(),
                    site,
                    account_hint,
                    source,
                });
            }
        }
        saw_encrypted = true;
    }

    if saw_encrypted {
        Err(FetchError::Unsupported(
            "Qoder local auth file is encrypted; paste your Cookie or Token in Settings".to_string(),
        ))
    } else {
        Err(FetchError::MissingKey)
    }
}

fn parse_window_obj(
    obj: &Value,
    id: &str,
    mins: Option<u32>,
    kind: WindowKind,
    default_scope: &str,
) -> Option<QuotaWindow> {
    let limit = num_at(obj, &["limit", "total", "quota", "cap", "maxCredits", "totalCredits"]);
    let used = num_at(obj, &["used", "consumed", "usedCredits"]);
    let remaining = num_at(obj, &["remaining", "left", "available", "remainingCredits"]);
    let pct_val = num_at(obj, &["usedPercent", "used_percent", "percent"]);

    let (final_used, final_limit, final_pct) = match (used, limit, remaining, pct_val) {
        (Some(u), Some(l), _, _) if l > 0.0 => (Some(u), Some(l), pct(u, l)),
        (None, Some(l), Some(r), _) if l > 0.0 => {
            let u = (l - r).max(0.0);
            (Some(u), Some(l), pct(u, l))
        }
        (_, _, _, Some(p)) => (used, limit, p),
        _ => return None,
    };

    let resets = time_at(obj, &["resetAt", "resetsAt", "resetTime", "expireAt", "endTime"]);
    let scope = str_at(obj, &["name", "title", "label"]).unwrap_or(default_scope);
    let mut w = QuotaWindow::new(id, mins, final_pct)
        .kind(kind)
        .scope(scope)
        .resets_at(resets);
    if let (Some(u), Some(l)) = (final_used, final_limit) {
        let unit = str_at(obj, &["unit"]).unwrap_or("credits");
        w = w.amounts(u, l, unit);
    }
    Some(w)
}

fn parse_qoder(v: &Value, source: &str) -> Snapshot {
    let mut snap = Snapshot::new(ProviderId::Qoder, source);
    let root = v.get("data").or_else(|| v.get("result")).unwrap_or(v);

    snap.plan = str_at(root, &["plan", "planName", "tier", "subscriptionTier"]).map(str::to_string);
    snap.account = str_at(root, &["email", "username", "userName"]).map(str::to_string);

    for (key, id, mins, kind, scope) in [
        ("daily", "daily", Some(24 * 60), WindowKind::Daily, "Daily"),
        ("dailyCredits", "daily", Some(24 * 60), WindowKind::Daily, "Daily"),
        ("monthly", "monthly", Some(30 * 24 * 60), WindowKind::Monthly, "Monthly"),
        ("monthlyCredits", "monthly", Some(30 * 24 * 60), WindowKind::Monthly, "Monthly"),
        ("credits", "credits", Some(30 * 24 * 60), WindowKind::Monthly, "Credits"),
    ] {
        if let Some(obj) = root.get(key) {
            if !snap.windows.iter().any(|w| w.id == id) {
                if let Some(w) = parse_window_obj(obj, id, mins, kind, scope) {
                    snap.windows.push(w);
                }
            }
        }
    }

    let campaigns = root
        .get("campaigns")
        .or_else(|| root.get("items"))
        .or_else(|| root.as_array().map(|_| root))
        .and_then(Value::as_array);
    if let Some(arr) = campaigns {
        for (i, item) in arr.iter().enumerate() {
            let period = str_at(item, &["period", "cycle", "type"]).unwrap_or("").to_lowercase();
            let (id, mins, kind, default_scope) = if period.contains("day") || period.contains("daily") {
                ("daily", Some(24 * 60), WindowKind::Daily, "Daily")
            } else {
                ("monthly", Some(30 * 24 * 60), WindowKind::Monthly, "Credits")
            };
            let win_id = if snap.windows.iter().any(|w| w.id == id) {
                format!("{id}_{i}")
            } else {
                id.to_string()
            };
            if let Some(w) = parse_window_obj(item, &win_id, mins, kind, default_scope) {
                snap.windows.push(w);
            }
        }
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

    fn test_ctx(home: &std::path::Path, keys: HashMap<ProviderId, String>) -> Ctx {
        Ctx::with(home.to_path_buf(), HashMap::new(), keys, OsString::new())
    }

    #[test]
    fn missing_key_when_unconfigured_and_no_local_file() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(dir.path(), HashMap::new());
        let err = load_creds(&ctx).unwrap_err();
        assert!(matches!(err, FetchError::MissingKey));
    }

    #[test]
    fn loads_readable_local_qoder_and_qoder_cn_auth() {
        let dir = TempDir::new().unwrap();
        let auth_dir = dir.path().join(".qoder-cn/.auth");
        std::fs::create_dir_all(&auth_dir).unwrap();
        std::fs::write(
            auth_dir.join("user"),
            serde_json::to_string(&json!({
                "accessToken": "qoder_cn_tok",
                "email": "dev@qoder.cn"
            }))
            .unwrap(),
        )
        .unwrap();

        let ctx = test_ctx(dir.path(), HashMap::new());
        let creds = load_creds(&ctx).unwrap();
        assert_eq!(creds.auth, "qoder_cn_tok");
        assert_eq!(creds.site, QoderSite::Cn);
        assert_eq!(creds.account_hint.as_deref(), Some("dev@qoder.cn"));
    }

    #[test]
    fn encrypted_local_auth_returns_friendly_unsupported_error() {
        let dir = TempDir::new().unwrap();
        let auth_dir = dir.path().join(".qoder/.auth");
        std::fs::create_dir_all(&auth_dir).unwrap();
        std::fs::write(auth_dir.join("user"), b"\x00\x01\x02encrypted-blob").unwrap();

        let ctx = test_ctx(dir.path(), HashMap::new());
        let err = load_creds(&ctx).unwrap_err();
        match err {
            FetchError::Unsupported(msg) => assert!(msg.contains("Settings")),
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn parses_daily_and_monthly_credits_and_campaigns() {
        let body = json!({
            "data": {
                "plan": "Qoder Pro",
                "email": "coder@qoder.com",
                "dailyCredits": { "used": 15.0, "limit": 50.0, "resetAt": 1796083200 },
                "monthlyCredits": { "remaining": 800.0, "total": 1000.0, "resetAt": "2026-12-01T00:00:00Z" }
            }
        });
        let snap = parse_qoder(&body, "qoder_local");
        assert_eq!(snap.provider, ProviderId::Qoder);
        assert_eq!(snap.plan.as_deref(), Some("Qoder Pro"));
        assert_eq!(snap.account.as_deref(), Some("coder@qoder.com"));
        assert_eq!(snap.windows.len(), 2);
        assert_eq!(snap.windows[0].id, "daily");
        assert!((snap.windows[0].used_percent - 30.0).abs() < 1e-6);
        assert_eq!(snap.windows[1].id, "monthly");
        assert!((snap.windows[1].used_percent - 20.0).abs() < 1e-6);
    }
}
