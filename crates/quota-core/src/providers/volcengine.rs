//! Volcengine / Doubao Ark (`volcengine.rs`) quota provider.
//!
//! Supports `AccessKeyId:SecretAccessKey` (or `VOLC_ACCESSKEY` + `VOLC_SECRETKEY`)
//! with Volcengine OpenAPI v4 HMAC-SHA256 signing, as well as single-token API key fallback.

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::ctx::{send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{Balance, ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{hmac_sha256, num_at, pct, sha256_hex, str_at, time_at};

const OPENAPI_HOST: &str = "open.volcengineapi.com";
const OPENAPI_ENDPOINT: &str = "https://open.volcengineapi.com/";
const ARK_SERVICE: &str = "ark";
const ARK_REGION: &str = "cn-beijing";
const API_VERSION: &str = "2024-01-01";

#[derive(Clone, Debug, PartialEq, Eq)]
enum VolcCreds {
    AkSk { ak: String, sk: String },
    ApiKey(String),
}

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let creds = load_creds(ctx)?;
    let now = Utc::now();

    match creds {
        VolcCreds::AkSk { ak, sk } => {
            let mut last_err = None;
            for action in ["GetCodingPlan", "ListCodingPlans"] {
                let body = "{}";
                let query = format!("Action={action}&Version={API_VERSION}");
                let (x_date, payload_hash, auth) =
                    sign_v4(&ak, &sk, OPENAPI_HOST, ARK_REGION, ARK_SERVICE, "POST", "/", &query, body.as_bytes(), now);
                let url = format!("{OPENAPI_ENDPOINT}?{query}");
                let req = ctx
                    .post(&url)
                    .header("Host", OPENAPI_HOST)
                    .header("X-Date", x_date)
                    .header("X-Content-Sha256", payload_hash)
                    .header("Authorization", auth)
                    .header("Content-Type", "application/json")
                    .body(body);
                match send_json(req).await {
                    Ok(v) => return Ok(parse_response(&v, "openapi_v4")),
                    Err(e @ FetchError::Http { status: 401 | 403, .. }) => return Err(e),
                    Err(e) => last_err = Some(e),
                }
            }
            Err(last_err.unwrap_or_else(|| FetchError::parse("volcengine: empty response")))
        }
        VolcCreds::ApiKey(key) => {
            let req = ctx
                .post(&format!("https://ark.cn-beijing.volces.com/api/v3/coding-plan/usage"))
                .header("Authorization", format!("Bearer {key}"))
                .json(&json!({}));
            let v = send_json(req).await?;
            Ok(parse_response(&v, "api_key"))
        }
    }
}

fn load_creds(ctx: &Ctx) -> Result<VolcCreds> {
    if let Some(raw) = ctx.api_key(ProviderId::Volcengine) {
        if let Some((ak, sk)) = raw.split_once(':') {
            let ak = ak.trim();
            let sk = sk.trim();
            if !ak.is_empty() && !sk.is_empty() {
                return Ok(VolcCreds::AkSk {
                    ak: ak.to_string(),
                    sk: sk.to_string(),
                });
            }
        }
        if let Some(sk) = ctx.env("VOLC_SECRETKEY") {
            return Ok(VolcCreds::AkSk {
                ak: raw,
                sk: sk.to_string(),
            });
        }
        return Ok(VolcCreds::ApiKey(raw));
    }

    if let (Some(ak), Some(sk)) = (ctx.env("VOLC_ACCESSKEY"), ctx.env("VOLC_SECRETKEY")) {
        return Ok(VolcCreds::AkSk {
            ak: ak.to_string(),
            sk: sk.to_string(),
        });
    }

    Err(FetchError::MissingKey)
}

/// Volcengine OpenAPI v4 HMAC-SHA256 signature.
/// Returns `(x_date, payload_sha256_hex, authorization_header)`.
fn sign_v4(
    ak: &str,
    sk: &str,
    host: &str,
    region: &str,
    service: &str,
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    now: DateTime<Utc>,
) -> (String, String, String) {
    let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = now.format("%Y%m%d").to_string();
    let payload_hash = sha256_hex(body);

    let canonical_headers = format!("host:{host}\nx-content-sha256:{payload_hash}\nx-date:{x_date}\n");
    let signed_headers = "host;x-content-sha256;x-date";
    let canonical_request =
        format!("{method}\n{path}\n{query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}");

    let scope = format!("{short_date}/{region}/{service}/request");
    let string_to_sign = format!(
        "HMAC-SHA256\n{x_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let k_date = hmac_sha256(sk.as_bytes(), short_date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"request");
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    let auth = format!(
        "HMAC-SHA256 Credential={ak}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );
    (x_date, payload_hash, auth)
}

fn parse_window_obj(
    obj: &Value,
    id: &str,
    default_mins: Option<u32>,
    kind: WindowKind,
    scope: &str,
) -> Option<QuotaWindow> {
    let limit = num_at(obj, &["limit", "Limit", "total", "Total", "quota", "Quota", "cap", "Cap"]);
    let used = num_at(obj, &["used", "Used", "usage", "Usage", "consumed", "Consumed"]);
    let remaining = num_at(obj, &["remaining", "Remaining", "left", "Left", "available", "Available"]);
    let pct_val = num_at(obj, &["usedPercent", "UsedPercent", "used_percent", "percent"]);

    let (final_used, final_limit, final_pct) = match (used, limit, remaining, pct_val) {
        (Some(u), Some(l), _, _) if l > 0.0 => (Some(u), Some(l), pct(u, l)),
        (None, Some(l), Some(r), _) if l > 0.0 => {
            let u = (l - r).max(0.0);
            (Some(u), Some(l), pct(u, l))
        }
        (_, _, _, Some(p)) => (used, limit, p),
        _ => return None,
    };

    let resets = time_at(obj, &["resetsAt", "ResetsAt", "resetAt", "ResetAt", "resetTime", "ResetTime", "endTime", "EndTime"]);
    let mut w = QuotaWindow::new(id, default_mins, final_pct)
        .kind(kind)
        .scope(scope)
        .resets_at(resets);
    if let (Some(u), Some(l)) = (final_used, final_limit) {
        let unit = str_at(obj, &["unit", "Unit"]).unwrap_or("credits");
        w = w.amounts(u, l, unit);
    }
    Some(w)
}

fn parse_response(v: &Value, source: &str) -> Snapshot {
    let mut snap = Snapshot::new(ProviderId::Volcengine, source);
    let root = v.get("Result").or_else(|| v.get("data")).unwrap_or(v);

    snap.plan = str_at(root, &["PlanName", "planName", "PackageName", "packageName", "Tier", "tier", "plan"])
        .map(str::to_string);
    snap.account = str_at(root, &["AccountName", "accountName", "AccountId", "accountId", "email"])
        .map(str::to_string);

    for (key, id, mins, kind, scope) in [
        ("fiveHour", "five_hour", Some(5 * 60), WindowKind::Session, "5 hours"),
        ("FiveHour", "five_hour", Some(5 * 60), WindowKind::Session, "5 hours"),
        ("weekly", "weekly", Some(7 * 24 * 60), WindowKind::Weekly, "Weekly"),
        ("Weekly", "weekly", Some(7 * 24 * 60), WindowKind::Weekly, "Weekly"),
        ("monthly", "monthly", Some(30 * 24 * 60), WindowKind::Monthly, "Monthly"),
        ("Monthly", "monthly", Some(30 * 24 * 60), WindowKind::Monthly, "Monthly"),
    ] {
        if let Some(obj) = root.get(key) {
            if !snap.windows.iter().any(|w| w.id == id) {
                if let Some(w) = parse_window_obj(obj, id, mins, kind, scope) {
                    snap.windows.push(w);
                }
            }
        }
    }

    if let Some(items) = root
        .get("Quotas")
        .or_else(|| root.get("quotas"))
        .or_else(|| root.get("Windows"))
        .or_else(|| root.get("windows"))
        .or_else(|| root.get("Items"))
        .or_else(|| root.get("items"))
        .and_then(Value::as_array)
    {
        for (i, item) in items.iter().enumerate() {
            let period = str_at(item, &["period", "Period", "window", "Window", "type", "Type"])
                .unwrap_or("")
                .to_lowercase();
            let (id, mins, kind, default_scope) = if period.contains("5h") || period.contains("hour") {
                ("five_hour", Some(5 * 60), WindowKind::Session, "5 hours")
            } else if period.contains("week") || period.contains("7d") {
                ("weekly", Some(7 * 24 * 60), WindowKind::Weekly, "Weekly")
            } else if period.contains("month") || period.contains("30d") {
                ("monthly", Some(30 * 24 * 60), WindowKind::Monthly, "Monthly")
            } else {
                ("quota", None, WindowKind::Other, "Quota")
            };
            let scope = str_at(item, &["name", "Name", "label", "Label"]).unwrap_or(default_scope);
            let win_id = if snap.windows.iter().any(|w| w.id == id) {
                format!("{id}_{i}")
            } else {
                id.to_string()
            };
            if let Some(w) = parse_window_obj(item, &win_id, mins, kind, scope) {
                snap.windows.push(w);
            }
        }
    }

    if let Some(bal) = num_at(root, &["Balance", "balance", "AvailableBalance", "availableBalance"]) {
        snap.balances.push(Balance {
            id: "balance".to_string(),
            amount: bal,
            currency: str_at(root, &["Currency", "currency"]).unwrap_or("CNY").to_string(),
            total: None,
            unlimited: false,
        });
    }

    snap
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use tempfile::TempDir;

    fn test_ctx(home: &std::path::Path, keys: HashMap<ProviderId, String>, env: HashMap<String, String>) -> Ctx {
        Ctx::with(home.to_path_buf(), env, keys, OsString::new())
    }

    #[test]
    fn missing_key_when_unconfigured() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(dir.path(), HashMap::new(), HashMap::new());
        let err = load_creds(&ctx).unwrap_err();
        assert!(matches!(err, FetchError::MissingKey));
    }

    #[test]
    fn signs_v4_deterministically() {
        let now = DateTime::parse_from_rfc3339("2026-11-19T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (x_date, hash, auth) = sign_v4(
            "AKIDTEST",
            "SECRETKEYTEST",
            OPENAPI_HOST,
            ARK_REGION,
            ARK_SERVICE,
            "POST",
            "/",
            "Action=GetCodingPlan&Version=2024-01-01",
            b"{}",
            now,
        );
        assert_eq!(x_date, "20261119T120000Z");
        assert_eq!(hash, "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a");
        assert!(auth.starts_with("HMAC-SHA256 Credential=AKIDTEST/20261119/cn-beijing/ark/request, SignedHeaders=host;x-content-sha256;x-date, Signature="));
        assert_eq!(auth.split("Signature=").nth(1).unwrap().len(), 64);
    }

    #[test]
    fn parses_coding_plan_windows_and_plan_name() {
        let body = json!({
            "Result": {
                "PlanName": "Doubao Coding Pro",
                "FiveHour": { "Used": 20.0, "Limit": 100.0, "ResetAt": 1796083200 },
                "Weekly": { "Used": 150.0, "Limit": 500.0, "ResetAt": "2026-11-25T00:00:00Z" },
                "Monthly": { "Used": 400.0, "Limit": 2000.0, "Unit": "requests" },
                "Balance": 48.5,
                "Currency": "CNY"
            }
        });

        let snap = parse_response(&body, "openapi_v4");
        assert_eq!(snap.provider, ProviderId::Volcengine);
        assert_eq!(snap.plan.as_deref(), Some("Doubao Coding Pro"));
        assert_eq!(snap.windows.len(), 3);
        assert_eq!(snap.windows[0].id, "five_hour");
        assert!((snap.windows[0].used_percent - 20.0).abs() < 1e-6);
        assert_eq!(snap.windows[1].id, "weekly");
        assert!((snap.windows[1].used_percent - 30.0).abs() < 1e-6);
        assert_eq!(snap.windows[2].id, "monthly");
        assert_eq!(snap.windows[2].unit.as_deref(), Some("requests"));
        assert_eq!(snap.balances.len(), 1);
        assert_eq!(snap.balances[0].amount, 48.5);
    }
}
