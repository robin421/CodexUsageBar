//! Baidu Qianfan Token Plan (`qianfan.rs`) quota provider.
//!
//! Queries `https://qianfan.baidubce.com/api/qianfan/charge/tokenPlanPersonal/resource`
//! and `/api/qianfan/charge/tokenPlanPersonal/pack/list` using console `Cookie` or Bearer token.

use serde_json::Value;

use crate::ctx::{send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{num_at, pct, str_at, time_at};

const RESOURCE_URL: &str =
    "https://qianfan.baidubce.com/api/qianfan/charge/tokenPlanPersonal/resource";
const PACK_LIST_URL: &str =
    "https://qianfan.baidubce.com/api/qianfan/charge/tokenPlanPersonal/pack/list";

#[derive(Clone, Debug, PartialEq, Eq)]
enum AuthHeader {
    Cookie(String),
    Bearer(String),
}

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let raw = ctx.api_key(ProviderId::Qianfan).ok_or(FetchError::MissingKey)?;
    let auth = classify_auth(&raw);

    let req_res = apply_auth(ctx.get(RESOURCE_URL), &auth);
    let req_pack = apply_auth(ctx.get(PACK_LIST_URL), &auth);

    let (res_r, pack_r) = tokio::join!(send_json(req_res), send_json(req_pack));
    let resource = res_r?;
    let pack_list = pack_r.ok();

    Ok(parse_qianfan(&resource, pack_list.as_ref()))
}

fn classify_auth(raw: &str) -> AuthHeader {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("Cookie:").or_else(|| trimmed.strip_prefix("cookie:")) {
        return AuthHeader::Cookie(rest.trim().to_string());
    }
    if trimmed.contains('=') && (trimmed.contains(';') || trimmed.contains("BDUSS") || trimmed.contains("BAIDUID")) {
        return AuthHeader::Cookie(trimmed.to_string());
    }
    let token = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .unwrap_or(trimmed)
        .trim();
    AuthHeader::Bearer(token.to_string())
}

fn apply_auth(req: reqwest::RequestBuilder, auth: &AuthHeader) -> reqwest::RequestBuilder {
    match auth {
        AuthHeader::Cookie(c) => req.header("Cookie", c),
        AuthHeader::Bearer(t) => req.header("Authorization", format!("Bearer {t}")),
    }
}

fn parse_window_entry(
    obj: &Value,
    default_id: &str,
    default_mins: Option<u32>,
    default_kind: WindowKind,
    default_scope: &str,
) -> Option<QuotaWindow> {
    let total = num_at(obj, &["total", "totalAmount", "quota", "limit", "totalQuota", "packQuota"]);
    let used = num_at(obj, &["used", "usedAmount", "consumed", "usedQuota"]);
    let remain = num_at(obj, &["remain", "remaining", "left", "available", "remainQuota"]);
    let pct_val = num_at(obj, &["usedPercent", "used_percent", "percent"]);

    let (final_used, final_limit, final_pct) = match (used, total, remain, pct_val) {
        (Some(u), Some(l), _, _) if l > 0.0 => (Some(u), Some(l), pct(u, l)),
        (None, Some(l), Some(r), _) if l > 0.0 => {
            let u = (l - r).max(0.0);
            (Some(u), Some(l), pct(u, l))
        }
        (_, _, _, Some(p)) => (used, total, p),
        _ => return None,
    };

    let resets = time_at(
        obj,
        &["resetTime", "resetAt", "resetsAt", "expireTime", "endTime", "nextResetTime"],
    );
    let scope = str_at(obj, &["name", "packName", "windowName", "label"]).unwrap_or(default_scope);
    let mut w = QuotaWindow::new(default_id, default_mins, final_pct)
        .kind(default_kind)
        .scope(scope)
        .resets_at(resets);
    if let (Some(u), Some(l)) = (final_used, final_limit) {
        let unit = str_at(obj, &["unit"]).unwrap_or("credits");
        w = w.amounts(u, l, unit);
    }
    Some(w)
}

fn parse_qianfan(resource: &Value, pack_list: Option<&Value>) -> Snapshot {
    let mut snap = Snapshot::new(ProviderId::Qianfan, "api");
    let res_root = resource.get("result").or_else(|| resource.get("data")).unwrap_or(resource);

    snap.plan = str_at(res_root, &["planName", "packageName", "tier", "plan"]).map(str::to_string);
    snap.account = str_at(res_root, &["accountName", "userName", "email"]).map(str::to_string);

    for (key, id, mins, kind, scope) in [
        ("fiveHour", "five_hour", Some(5 * 60), WindowKind::Session, "5 hours"),
        ("daily", "daily", Some(24 * 60), WindowKind::Daily, "Daily"),
        ("weekly", "weekly", Some(7 * 24 * 60), WindowKind::Weekly, "Weekly"),
        ("monthly", "monthly", Some(30 * 24 * 60), WindowKind::Monthly, "Monthly"),
    ] {
        if let Some(obj) = res_root.get(key) {
            if let Some(w) = parse_window_entry(obj, id, mins, kind, scope) {
                snap.windows.push(w);
            }
        }
    }

    if snap.windows.is_empty() {
        if let Some(w) = parse_window_entry(
            res_root,
            "token_plan",
            Some(30 * 24 * 60),
            WindowKind::Monthly,
            "Token Plan",
        ) {
            snap.windows.push(w);
        }
    }

    if let Some(packs) = pack_list
        .and_then(|v| {
            v.get("result")
                .or_else(|| v.get("data"))
                .unwrap_or(v)
                .get("list")
                .or_else(|| v.get("result").and_then(|r| r.get("items")))
                .or_else(|| v.get("data").and_then(Value::as_array).map(|_| v.get("data").unwrap()))
                .and_then(Value::as_array)
        })
    {
        for (i, pack) in packs.iter().enumerate() {
            if snap.plan.is_none() {
                snap.plan = str_at(pack, &["packName", "planName", "name"]).map(str::to_string);
            }
            let id = format!("pack_{i}");
            if let Some(w) = parse_window_entry(pack, &id, Some(30 * 24 * 60), WindowKind::Monthly, "Pack") {
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

    #[test]
    fn missing_key_when_unconfigured() {
        let dir = TempDir::new().unwrap();
        let ctx = Ctx::with(dir.path().to_path_buf(), HashMap::new(), HashMap::new(), OsString::new());
        let err = tokio::runtime::Runtime::new().unwrap().block_on(fetch(&ctx)).unwrap_err();
        assert!(matches!(err, FetchError::MissingKey));
    }

    #[test]
    fn detects_cookie_vs_bearer_header() {
        assert_eq!(
            classify_auth("BDUSS=abc123; BAIDUID=xyz"),
            AuthHeader::Cookie("BDUSS=abc123; BAIDUID=xyz".into())
        );
        assert_eq!(
            classify_auth("Cookie: BDUSS=abc"),
            AuthHeader::Cookie("BDUSS=abc".into())
        );
        assert_eq!(
            classify_auth("Bearer tok_123"),
            AuthHeader::Bearer("tok_123".into())
        );
        assert_eq!(classify_auth("tok_456"), AuthHeader::Bearer("tok_456".into()));
    }

    #[test]
    fn parses_resource_and_pack_list_windows() {
        let resource = json!({
            "result": {
                "planName": "Qianfan Personal Pro",
                "fiveHour": { "used": 25.0, "total": 100.0, "resetTime": 1796083200 },
                "monthly": { "remain": 750.0, "total": 1000.0, "resetTime": "2026-12-01T00:00:00Z" }
            }
        });
        let packs = json!({
            "result": {
                "list": [
                    { "packName": "Bonus Pack", "used": 10.0, "total": 200.0, "expireTime": "2026-12-15T00:00:00Z" }
                ]
            }
        });

        let snap = parse_qianfan(&resource, Some(&packs));
        assert_eq!(snap.provider, ProviderId::Qianfan);
        assert_eq!(snap.plan.as_deref(), Some("Qianfan Personal Pro"));
        assert_eq!(snap.windows.len(), 3);
        assert_eq!(snap.windows[0].id, "five_hour");
        assert!((snap.windows[0].used_percent - 25.0).abs() < 1e-6);
        assert_eq!(snap.windows[1].id, "monthly");
        assert!((snap.windows[1].used_percent - 25.0).abs() < 1e-6);
        assert_eq!(snap.windows[2].id, "pack_0");
        assert_eq!(snap.windows[2].scope.as_deref(), Some("Bonus Pack"));
    }
}
