//! Alibaba Cloud Bailian / Qwen Token Plan (`bailian.rs`) quota provider.
//!
//! Supports `AccessKeyId:SecretAccessKey` (or `ALIBABA_CLOUD_ACCESS_KEY_ID` +
//! `ALIBABA_CLOUD_ACCESS_KEY_SECRET`) with ACS3-HMAC-SHA256 signing.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::ctx::{send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{Balance, ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{hmac_sha256, num_at, pct, sha256_hex, str_at, time_at};

const BAILIAN_HOST: &str = "bailian.cn-beijing.aliyuncs.com";
const BAILIAN_ENDPOINT: &str = "https://bailian.cn-beijing.aliyuncs.com/";
const BAILIAN_VERSION: &str = "2023-12-29";

#[derive(Clone, Debug, PartialEq, Eq)]
struct AliCreds {
    ak: String,
    sk: String,
}

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let creds = load_creds(ctx)?;
    let now = Utc::now();
    let nonce = format!("{:x}", now.timestamp_nanos_opt().unwrap_or(0));

    let mut last_err = None;
    for action in ["GetTokenPlanQuota", "GetCodingPlanQuota", "DescribeCodingPlan"] {
        let body = b"{}";
        let (x_date, payload_hash, auth) =
            sign_acs3(&creds.ak, &creds.sk, BAILIAN_HOST, action, BAILIAN_VERSION, &nonce, body, now);

        let req = ctx
            .post(BAILIAN_ENDPOINT)
            .header("Host", BAILIAN_HOST)
            .header("x-acs-action", action)
            .header("x-acs-version", BAILIAN_VERSION)
            .header("x-acs-date", x_date)
            .header("x-acs-signature-nonce", &nonce)
            .header("x-acs-content-sha256", payload_hash)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .body(body.as_slice());

        match send_json(req).await {
            Ok(v) => return Ok(parse_response(&v)),
            Err(e @ FetchError::Http { status: 401 | 403, .. }) => return Err(e),
            Err(e) => last_err = Some(e),
        }
    }

    Err(last_err.unwrap_or_else(|| FetchError::parse("bailian: empty response")))
}

fn load_creds(ctx: &Ctx) -> Result<AliCreds> {
    if let Some(raw) = ctx.api_key(ProviderId::Bailian) {
        if let Some((ak, sk)) = raw.split_once(':') {
            let ak = ak.trim();
            let sk = sk.trim();
            if !ak.is_empty() && !sk.is_empty() {
                return Ok(AliCreds {
                    ak: ak.to_string(),
                    sk: sk.to_string(),
                });
            }
        }
        if let Some(sk) = ctx.env("ALIBABA_CLOUD_ACCESS_KEY_SECRET") {
            return Ok(AliCreds {
                ak: raw,
                sk: sk.to_string(),
            });
        }
        return Err(FetchError::parse(
            "Bailian requires AccessKeyId:SecretAccessKey format",
        ));
    }

    if let (Some(ak), Some(sk)) = (
        ctx.env("ALIBABA_CLOUD_ACCESS_KEY_ID"),
        ctx.env("ALIBABA_CLOUD_ACCESS_KEY_SECRET"),
    ) {
        return Ok(AliCreds {
            ak: ak.to_string(),
            sk: sk.to_string(),
        });
    }

    Err(FetchError::MissingKey)
}

/// Alibaba Cloud ACS3-HMAC-SHA256 signature (`aliACSSign`).
/// Returns `(x_acs_date, x_acs_content_sha256, authorization_header)`.
fn sign_acs3(
    ak: &str,
    sk: &str,
    host: &str,
    action: &str,
    version: &str,
    nonce: &str,
    body: &[u8],
    now: DateTime<Utc>,
) -> (String, String, String) {
    let x_date = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let payload_hash = sha256_hex(body);
    let signed_headers =
        "host;x-acs-action;x-acs-content-sha256;x-acs-date;x-acs-signature-nonce;x-acs-version";

    let canonical_headers = format!(
        "host:{host}\nx-acs-action:{action}\nx-acs-content-sha256:{payload_hash}\nx-acs-date:{x_date}\nx-acs-signature-nonce:{nonce}\nx-acs-version:{version}\n"
    );
    let canonical_request =
        format!("POST\n/\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}");

    let string_to_sign = format!(
        "ACS3-HMAC-SHA256\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let signature = hex::encode(hmac_sha256(sk.as_bytes(), string_to_sign.as_bytes()));
    let auth = format!(
        "ACS3-HMAC-SHA256 Credential={ak},SignedHeaders={signed_headers},Signature={signature}"
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

    let resets = time_at(
        obj,
        &[
            "resetsAt", "ResetsAt", "resetAt", "ResetAt", "resetTime", "ResetTime", "endTime",
            "EndTime",
        ],
    );
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

fn parse_response(v: &Value) -> Snapshot {
    let mut snap = Snapshot::new(ProviderId::Bailian, "acs3");
    let root = v
        .get("Data")
        .or_else(|| v.get("data"))
        .or_else(|| v.get("Result"))
        .unwrap_or(v);

    snap.plan = str_at(
        root,
        &["PlanName", "planName", "PackageName", "packageName", "ComboName", "comboName", "plan"],
    )
    .map(str::to_string);
    snap.account = str_at(root, &["AccountName", "accountName", "Uid", "uid"]).map(str::to_string);

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
        .get("QuotaList")
        .or_else(|| root.get("quotaList"))
        .or_else(|| root.get("Quotas"))
        .or_else(|| root.get("quotas"))
        .or_else(|| root.get("Windows"))
        .or_else(|| root.get("windows"))
        .and_then(Value::as_array)
    {
        for (i, item) in items.iter().enumerate() {
            let period = str_at(item, &["period", "Period", "cycle", "Cycle", "type", "Type"])
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

    if let Some(bal) = num_at(root, &["Balance", "balance", "AvailableBalance"]) {
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
    use serde_json::json;
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
    fn signs_acs3_deterministically() {
        let now = DateTime::parse_from_rfc3339("2026-11-19T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (x_date, hash, auth) = sign_acs3(
            "LTAI_TEST",
            "SECRET_TEST",
            BAILIAN_HOST,
            "GetTokenPlanQuota",
            BAILIAN_VERSION,
            "nonce123",
            b"{}",
            now,
        );
        assert_eq!(x_date, "2026-11-19T12:00:00Z");
        assert_eq!(hash, "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a");
        assert!(auth.starts_with("ACS3-HMAC-SHA256 Credential=LTAI_TEST,SignedHeaders=host;x-acs-action;x-acs-content-sha256;x-acs-date;x-acs-signature-nonce;x-acs-version,Signature="));
        assert_eq!(auth.split("Signature=").nth(1).unwrap().len(), 64);
    }

    #[test]
    fn parses_bailian_quota_windows_and_plan() {
        let body = json!({
            "Data": {
                "PackageName": "Bailian Qwen Pro",
                "FiveHour": { "Used": 10.0, "Limit": 200.0, "ResetAt": 1796083200 },
                "Weekly": { "Used": 100.0, "Limit": 1000.0 },
                "Monthly": { "Remaining": 3500.0, "Total": 5000.0, "Unit": "credits" }
            }
        });
        let snap = parse_response(&body);
        assert_eq!(snap.provider, ProviderId::Bailian);
        assert_eq!(snap.plan.as_deref(), Some("Bailian Qwen Pro"));
        assert_eq!(snap.windows.len(), 3);
        assert_eq!(snap.windows[0].id, "five_hour");
        assert!((snap.windows[0].used_percent - 5.0).abs() < 1e-6);
        assert_eq!(snap.windows[1].id, "weekly");
        assert!((snap.windows[1].used_percent - 10.0).abs() < 1e-6);
        assert_eq!(snap.windows[2].id, "monthly");
        assert!((snap.windows[2].used_percent - 30.0).abs() < 1e-6);
        assert_eq!(snap.windows[2].used, Some(1500.0));
    }
}
