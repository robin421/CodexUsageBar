//! Antigravity (`agy`) quota fetching via Language Server Connect-RPC or Cloud Code PA endpoint.
//!
//! Antigravity manages Google, Claude, and GPT model quotas in weekly and 5-hour rolling windows.
//! When running within an active Antigravity session, it connects to the local Language Server
//! via `$ANTIGRAVITY_LS_ADDRESS` and `$ANTIGRAVITY_CSRF_TOKEN`.
//! Otherwise, it refreshes OAuth credentials stored in `~/.gemini/antigravity-cli/accounts/`
//! and calls `https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary`.

use std::path::PathBuf;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ctx::Ctx;
use crate::error::{FetchError, Result};
use crate::model::{ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::parse_time;

const CCPA_QUOTA_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary";
const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

fn agy_client_credentials() -> (String, String) {
    const CID_XOR: &[u8] = &[
        107, 106, 109, 107, 106, 106, 108, 106, 108, 106, 111, 99, 107, 119, 46, 55, 50, 41, 41, 51,
        52, 104, 50, 104, 107, 54, 57, 40, 63, 104, 105, 111, 44, 46, 53, 54, 53, 48, 50, 110, 61,
        110, 106, 105, 63, 42, 116, 59, 42, 42, 41, 116, 61, 53, 53, 61, 54, 63, 47, 41, 63, 40, 57,
        53, 52, 46, 63, 52, 46, 116, 57, 53, 55,
    ];
    const SEC_XOR: &[u8] = &[
        29, 21, 25, 9, 10, 2, 119, 17, 111, 98, 28, 13, 8, 110, 98, 108, 22, 62, 22, 16, 107, 55, 22,
        24, 98, 41, 2, 25, 110, 32, 108, 43, 30, 27, 60,
    ];
    let id_bytes: Vec<u8> = CID_XOR.iter().map(|b| b ^ 0x5A).collect();
    let sec_bytes: Vec<u8> = SEC_XOR.iter().map(|b| b ^ 0x5A).collect();
    (
        String::from_utf8(id_bytes).unwrap_or_default(),
        String::from_utf8(sec_bytes).unwrap_or_default(),
    )
}

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
struct StoredToken {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expiry: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Deserialize, Debug, Clone)]
struct AccountFile {
    token: Option<StoredToken>,
    id_token_email: Option<String>,
}

#[derive(Serialize)]
struct GoogleRefreshReq<'a> {
    client_id: &'a str,
    client_secret: &'a str,
    refresh_token: &'a str,
    grant_type: &'a str,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GoogleTokenResp {
    access_token: String,
    expires_in: Option<i64>,
}

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    // 1. Try local Language Server if ANTIGRAVITY_LS_ADDRESS is in the environment
    if let Ok(ls_addr) = std::env::var("ANTIGRAVITY_LS_ADDRESS") {
        if !ls_addr.is_empty() {
            let csrf = std::env::var("ANTIGRAVITY_CSRF_TOKEN").unwrap_or_default();
            if let Ok(snap) = fetch_from_ls(ctx, &ls_addr, &csrf).await {
                return Ok(snap);
            }
        }
    }

    // 2. Fall back to direct Google Cloud Code PA endpoint using stored OAuth credentials
    fetch_from_cloudcode_pa(ctx).await
}

async fn fetch_from_ls(ctx: &Ctx, ls_addr: &str, csrf: &str) -> Result<Snapshot> {
    let url = format!("http://{ls_addr}/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary");
    let resp = ctx
        .http
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("x-codeium-csrf-token", csrf)
        .body("{}")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(FetchError::Http {
            status: resp.status().as_u16(),
            body: resp.text().await.unwrap_or_default(),
        });
    }

    let body: Value = resp.json().await?;
    let mut snap = parse_quota_summary(&body)?;
    snap.source = "antigravity_ls".into();

    // Fill account email if present in accounts/settings
    if snap.account.is_none() {
        if let Some((email, _)) = load_account_creds(ctx) {
            snap.account = Some(email);
        }
    }

    Ok(snap)
}

async fn fetch_from_cloudcode_pa(ctx: &Ctx) -> Result<Snapshot> {
    let agy_dir = ctx.home.join(".gemini/antigravity-cli");
    let gemini_dir = ctx.home.join(".gemini");

    let (email, token_info) = match load_account_creds(ctx) {
        Some(creds) => creds,
        None => {
            if agy_dir.exists() || gemini_dir.exists() || ctx.find_bin("agy").is_some() {
                return Err(FetchError::NotLoggedIn);
            }
            return Err(FetchError::NotDetected);
        }
    };

    let access_token = ensure_valid_token(ctx, &token_info).await?;

    let resp = ctx
        .http
        .post(CCPA_QUOTA_ENDPOINT)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", "Antigravity/1.5.3")
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await?;

    if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
        // Attempt one fresh refresh in case token was invalidated server-side
        if let Some(refresh_token) = &token_info.refresh_token {
            if let Ok(new_token) = refresh_google_token(ctx, refresh_token).await {
                let retry_resp = ctx
                    .http
                    .post(CCPA_QUOTA_ENDPOINT)
                    .header("Authorization", format!("Bearer {new_token}"))
                    .header("User-Agent", "Antigravity/1.5.3")
                    .header("Content-Type", "application/json")
                    .body("{}")
                    .send()
                    .await?;

                if retry_resp.status().is_success() {
                    let body: Value = retry_resp.json().await?;
                    let mut snap = parse_quota_summary(&body)?;
                    snap.account = Some(email);
                    return Ok(snap);
                }
            }
        }
        return Err(FetchError::AuthExpired);
    }

    if !resp.status().is_success() {
        return Err(FetchError::Http {
            status: resp.status().as_u16(),
            body: resp.text().await.unwrap_or_default(),
        });
    }

    let body: Value = resp.json().await?;
    let mut snap = parse_quota_summary(&body)?;
    snap.account = Some(email);
    Ok(snap)
}

fn load_account_creds(ctx: &Ctx) -> Option<(String, StoredToken)> {
    let accounts_dir = ctx.home.join(".gemini/antigravity-cli/accounts");

    // 1. Check active account from google_accounts.json
    let active_json = ctx.home.join(".gemini/google_accounts.json");
    if let Ok(content) = std::fs::read_to_string(&active_json) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            if let Some(active) = v.get("active").and_then(|a| a.as_str()) {
                let file = accounts_dir.join(format!("{active}.json"));
                if let Some((email, token)) = read_account_file(&file) {
                    return Some((email, token));
                }
            }
        }
    }

    // 2. Scan accounts directory for any .json
    if let Ok(entries) = std::fs::read_dir(&accounts_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Some((email, token)) = read_account_file(&path) {
                    return Some((email, token));
                }
            }
        }
    }

    // 3. Fallback on macOS Keychain
    #[cfg(target_os = "macos")]
    if let Some((email, token)) = read_keychain_account() {
        return Some((email, token));
    }

    None
}

fn read_account_file(path: &PathBuf) -> Option<(String, StoredToken)> {
    let content = std::fs::read_to_string(path).ok()?;
    let acc: AccountFile = serde_json::from_str(&content).ok()?;
    let token = acc.token?;
    let email = acc.id_token_email.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("antigravity-user")
            .to_string()
    });
    Some((email, token))
}

#[cfg(target_os = "macos")]
fn read_keychain_account() -> Option<(String, StoredToken)> {
    use std::process::Command;
    let output = Command::new("security")
        .args(["find-generic-password", "-s", "gemini", "-a", "antigravity", "-w"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?.trim().to_string();
    let json_str = if let Some(stripped) = raw.strip_prefix("go-keyring-base64:") {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(stripped).ok()?;
        String::from_utf8(bytes).ok()?
    } else {
        raw
    };
    let acc: AccountFile = serde_json::from_str(&json_str).ok()?;
    let token = acc.token?;
    let email = acc.id_token_email.unwrap_or_else(|| "antigravity-user".into());
    Some((email, token))
}

async fn ensure_valid_token(ctx: &Ctx, token: &StoredToken) -> Result<String> {
    let is_expired = match &token.expiry {
        Some(exp) => parse_time(&Value::String(exp.clone())).is_some_and(|t| t <= Utc::now() + Duration::seconds(60)),
        None => false,
    };

    if !is_expired {
        if let Some(access) = &token.access_token {
            if !access.is_empty() {
                return Ok(access.clone());
            }
        }
    }

    if let Some(refresh) = &token.refresh_token {
        return refresh_google_token(ctx, refresh).await;
    }

    token
        .access_token
        .clone()
        .ok_or_else(|| FetchError::parse("missing access_token and refresh_token"))
}

async fn refresh_google_token(ctx: &Ctx, refresh_token: &str) -> Result<String> {
    let (client_id, client_secret) = agy_client_credentials();
    let req = GoogleRefreshReq {
        client_id: &client_id,
        client_secret: &client_secret,
        refresh_token,
        grant_type: "refresh_token",
    };

    let resp = ctx
        .http
        .post(GOOGLE_TOKEN_ENDPOINT)
        .form(&req)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        if status == 400 || status == 401 {
            return Err(FetchError::AuthExpired);
        }
        return Err(FetchError::Http { status, body });
    }

    let data: GoogleTokenResp = resp.json().await?;
    Ok(data.access_token)
}

pub fn parse_quota_summary(body: &Value) -> Result<Snapshot> {
    let mut snap = Snapshot::new(ProviderId::Agy, "oauth");

    // Support both root `groups` or Connect-RPC wrapped `response.groups`
    let groups = body
        .get("groups")
        .or_else(|| body.get("response").and_then(|r| r.get("groups")))
        .and_then(|g| g.as_array())
        .ok_or_else(|| FetchError::parse("missing groups array in quota summary"))?;

    let mut has_pro_bucket = false;

    for group in groups {
        let group_name = group.get("displayName").and_then(|d| d.as_str()).unwrap_or("Antigravity");
        let scope = match group_name {
            "Gemini Models" => "Gemini",
            "Claude and GPT models" => "Claude & GPT",
            "Gemini 4 Argon" => "Gemini Argon",
            other => other,
        };

        let buckets = group.get("buckets").and_then(|b| b.as_array());
        if let Some(buckets) = buckets {
            for b in buckets {
                // If a bucket is explicitly marked disabled, skip it
                if b.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false) {
                    continue;
                }

                let bucket_id = b.get("bucketId").and_then(|s| s.as_str()).unwrap_or("quota");
                let remaining_fraction = b.get("remainingFraction").and_then(|f| f.as_f64()).unwrap_or(1.0);
                let used_percent = ((1.0 - remaining_fraction) * 100.0).clamp(0.0, 100.0);

                let reset_time = b.get("resetTime").and_then(parse_time);
                let window_str = b.get("window").and_then(|w| w.as_str()).unwrap_or("");

                let (window_mins, kind) = match window_str {
                    "5h" => (Some(300), WindowKind::Session),
                    "weekly" => (Some(10080), WindowKind::Weekly),
                    "daily" => (Some(1440), WindowKind::Daily),
                    "monthly" => (Some(43200), WindowKind::Monthly),
                    _ => (None, WindowKind::Other),
                };

                if bucket_id.starts_with("3p-") || bucket_id.contains("argon") {
                    has_pro_bucket = true;
                }

                snap.windows.push(
                    QuotaWindow::new(bucket_id, window_mins, used_percent)
                        .kind(kind)
                        .scope(scope)
                        .resets_at(reset_time),
                );
            }
        }
    }

    if snap.windows.is_empty() {
        return Err(FetchError::parse("no active quota buckets found"));
    }

    // Sort windows: shorter windows (session 5h) first, then weekly, grouped by scope
    snap.windows.sort_by_key(|w| (w.scope.clone(), w.window_mins.unwrap_or(u32::MAX)));

    snap.plan = Some(if has_pro_bucket { "Pro".into() } else { "Antigravity".into() });

    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connect_rpc_ls_response() {
        let fixture = serde_json::json!({
            "response": {
                "groups": [
                    {
                        "displayName": "Gemini Models",
                        "buckets": [
                            {
                                "bucketId": "gemini-weekly",
                                "displayName": "Weekly Limit Remaining",
                                "window": "weekly",
                                "remainingFraction": 0.4606,
                                "resetTime": "2026-11-14T02:29:51Z"
                            },
                            {
                                "bucketId": "gemini-5h",
                                "displayName": "Five Hour Limit Remaining",
                                "window": "5h",
                                "remainingFraction": 0.7770,
                                "resetTime": "2026-11-08T13:30:29Z"
                            }
                        ]
                    },
                    {
                        "displayName": "Claude and GPT models",
                        "buckets": [
                            {
                                "bucketId": "3p-weekly",
                                "displayName": "Weekly Limit Remaining",
                                "window": "weekly",
                                "remainingFraction": 0.0,
                                "resetTime": "2026-11-10T16:13:41Z"
                            },
                            {
                                "bucketId": "3p-5h",
                                "displayName": "Five Hour Limit Remaining",
                                "window": "5h",
                                "remainingFraction": 0.4110,
                                "disabled": true,
                                "resetTime": "2026-11-08T10:11:28Z"
                            }
                        ]
                    }
                ]
            }
        });

        let s = parse_quota_summary(&fixture).expect("parsed");
        assert_eq!(s.provider, ProviderId::Agy);
        assert_eq!(s.plan.as_deref(), Some("Pro"));
        // 3 buckets because 3p-5h disabled is skipped
        assert_eq!(s.windows.len(), 3);

        let g_5h = s.windows.iter().find(|w| w.id == "gemini-5h").unwrap();
        assert!((g_5h.remaining_percent() - 77.70).abs() < 0.1);
        assert_eq!(g_5h.kind, WindowKind::Session);
        assert_eq!(g_5h.scope.as_deref(), Some("Gemini"));

        let p_weekly = s.windows.iter().find(|w| w.id == "3p-weekly").unwrap();
        assert_eq!(p_weekly.remaining_percent(), 0.0);
        assert_eq!(p_weekly.kind, WindowKind::Weekly);
        assert_eq!(p_weekly.scope.as_deref(), Some("Claude & GPT"));
    }

    #[test]
    fn parses_direct_ccpa_response() {
        let fixture = serde_json::json!({
            "groups": [
                {
                    "displayName": "Claude and GPT models",
                    "buckets": [
                        {
                            "bucketId": "3p-weekly",
                            "displayName": "Weekly Limit Remaining",
                            "window": "weekly",
                            "remainingFraction": 0.25,
                            "resetTime": "2026-11-10T12:00:00Z"
                        }
                    ]
                }
            ]
        });

        let s = parse_quota_summary(&fixture).expect("parsed");
        assert_eq!(s.windows.len(), 1);
        assert_eq!(s.windows[0].remaining_percent(), 25.0);
    }
}
