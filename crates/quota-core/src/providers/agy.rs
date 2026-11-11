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
    let accounts = load_all_accounts(ctx);
    if accounts.is_empty() {
        let agy_dir = ctx.home.join(".gemini/antigravity-cli");
        let gemini_dir = ctx.home.join(".gemini");
        if agy_dir.exists() || gemini_dir.exists() || ctx.find_bin("agy").is_some() {
            return Err(FetchError::NotLoggedIn);
        }
        return Err(FetchError::NotDetected);
    }

    let is_multi_account = accounts.len() > 1;

    // 1. Try local Language Server if ANTIGRAVITY_LS_ADDRESS is in the environment
    let mut ls_snap = None;
    let mut ls_account = None;

    if let Ok(ls_addr) = std::env::var("ANTIGRAVITY_LS_ADDRESS") {
        if !ls_addr.is_empty() {
            let csrf = std::env::var("ANTIGRAVITY_CSRF_TOKEN").unwrap_or_default();
            if let Ok(snap) = fetch_from_ls(ctx, &ls_addr, &csrf).await {
                ls_account = get_ls_account_email(ctx, &ls_addr, &csrf).await;
                ls_snap = Some(snap);
            }
        }
    }

    let mut combined = Snapshot::new(ProviderId::Agy, if ls_snap.is_some() { "antigravity_ls" } else { "oauth" });
    let mut account_names = Vec::new();
    let mut has_pro = false;

    if let Some(mut ls_s) = ls_snap {
        let active_email = ls_account.clone().unwrap_or_else(|| {
            accounts.first().map(|(e, _)| e.clone()).unwrap_or_else(|| "default".into())
        });
        let short_id = active_email.split('@').next().unwrap_or(&active_email).to_string();
        account_names.push(active_email.clone());

        if ls_s.plan.as_deref() == Some("Pro") {
            has_pro = true;
        }

        for mut w in ls_s.windows.drain(..) {
            w.account = Some(active_email.clone());
            if is_multi_account {
                let base = w.scope.clone().unwrap_or_else(|| "Default".into());
                w.scope = Some(format!("{base} ({short_id})"));
                w.id = format!("{short_id}_{}", w.id);
            }
            combined.windows.push(w);
        }
    }

    for (email, token_info) in &accounts {
        if ls_account.as_deref() == Some(email.as_str()) {
            continue;
        }

        if let Ok(mut snap) = fetch_ccpa_for_account(ctx, email, token_info).await {
            account_names.push(email.clone());
            if snap.plan.as_deref() == Some("Pro") {
                has_pro = true;
            }
            let short_id = email.split('@').next().unwrap_or(email);
            for mut w in snap.windows.drain(..) {
                w.account = Some(email.clone());
                if is_multi_account {
                    let base = w.scope.clone().unwrap_or_else(|| "Default".into());
                    w.scope = Some(format!("{base} ({short_id})"));
                    w.id = format!("{short_id}_{}", w.id);
                }
                combined.windows.push(w);
            }
        }
    }

    // Ensure every account has all standard models (Gemini 3.8 Flash, Argon 4, Claude & GPT)
    for (email, _) in &accounts {
        ensure_account_models_complete(&mut combined, email, is_multi_account);
    }

    if combined.windows.is_empty() {
        return Err(FetchError::parse("no active quota buckets found"));
    }

    combined.account = Some(account_names.join(", "));
    combined.plan = Some(if has_pro { "Pro".into() } else { "Antigravity".into() });

    // Sort windows: shorter windows (session 5h) first, then weekly, grouped by scope
    combined.windows.sort_by_key(|w| (w.account.clone(), w.scope.clone(), w.window_mins.unwrap_or(u32::MAX)));

    Ok(combined)
}

fn ensure_account_models_complete(
    combined: &mut Snapshot,
    email: &str,
    is_multi_account: bool,
) {
    let short_id = email.split('@').next().unwrap_or(email);

    // Prefer an existing weekly reset time from this account, or fallback to any known weekly reset
    let existing_weekly_reset = combined
        .windows
        .iter()
        .find(|w| w.account.as_deref() == Some(email) && w.kind == WindowKind::Weekly && w.resets_at.is_some())
        .or_else(|| combined.windows.iter().find(|w| w.kind == WindowKind::Weekly && w.resets_at.is_some()))
        .and_then(|w| w.resets_at);

    let has_flash_weekly = combined.windows.iter().any(|w| {
        w.account.as_deref() == Some(email)
            && w.scope.as_deref().is_some_and(|s| s.contains("Flash") || s.contains("3.8"))
            && w.kind == WindowKind::Weekly
    });
    let has_flash_5h = combined.windows.iter().any(|w| {
        w.account.as_deref() == Some(email)
            && w.scope.as_deref().is_some_and(|s| s.contains("Flash") || s.contains("3.8"))
            && w.kind == WindowKind::Session
    });
    let has_argon = combined.windows.iter().any(|w| {
        w.account.as_deref() == Some(email)
            && w.scope.as_deref().is_some_and(|s| s.contains("Argon"))
    });
    let has_claude_weekly = combined.windows.iter().any(|w| {
        w.account.as_deref() == Some(email)
            && w.scope.as_deref().is_some_and(|s| s.contains("Claude"))
            && w.kind == WindowKind::Weekly
    });
    let has_claude_5h = combined.windows.iter().any(|w| {
        w.account.as_deref() == Some(email)
            && w.scope.as_deref().is_some_and(|s| s.contains("Claude"))
            && w.kind == WindowKind::Session
    });

    let fmt_scope = |base: &str| -> String {
        if is_multi_account {
            format!("{base} ({short_id})")
        } else {
            base.to_string()
        }
    };

    if !has_flash_5h {
        let mut w = QuotaWindow::new(format!("{short_id}_gemini-5h"), Some(300), 0.0)
            .kind(WindowKind::Session)
            .scope(fmt_scope("Gemini 3.8 Flash"));
        w.account = Some(email.to_string());
        combined.windows.push(w);
    }
    if !has_flash_weekly {
        let mut w = QuotaWindow::new(format!("{short_id}_gemini-weekly"), Some(10080), 0.0)
            .kind(WindowKind::Weekly)
            .resets_at(existing_weekly_reset)
            .scope(fmt_scope("Gemini 3.8 Flash"));
        w.account = Some(email.to_string());
        combined.windows.push(w);
    }
    if !has_argon {
        // When Google does not return an Argon 4 bucket, the account has no Argon 4 quota.
        // Set used_percent to 100.0 (0% remaining) and no reset countdown.
        let mut w = QuotaWindow::new(format!("{short_id}_gemini-argon-wtu-weekly"), Some(10080), 100.0)
            .kind(WindowKind::Weekly)
            .scope(fmt_scope("Argon 4"));
        w.account = Some(email.to_string());
        combined.windows.push(w);
    }
    if !has_claude_5h {
        let mut w = QuotaWindow::new(format!("{short_id}_3p-5h"), Some(300), 0.0)
            .kind(WindowKind::Session)
            .scope(fmt_scope("Claude & GPT"));
        w.account = Some(email.to_string());
        combined.windows.push(w);
    }
    if !has_claude_weekly {
        let mut w = QuotaWindow::new(format!("{short_id}_3p-weekly"), Some(10080), 0.0)
            .kind(WindowKind::Weekly)
            .resets_at(existing_weekly_reset)
            .scope(fmt_scope("Claude & GPT"));
        w.account = Some(email.to_string());
        combined.windows.push(w);
    }
}

async fn get_ls_account_email(ctx: &Ctx, ls_addr: &str, csrf: &str) -> Option<String> {
    // 1. Try GetUserStatus (direct email in userStatus.email)
    let url_status = format!("http://{ls_addr}/exa.language_server_pb.LanguageServerService/GetUserStatus");
    if let Ok(resp) = ctx
        .http
        .post(&url_status)
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("x-codeium-csrf-token", csrf)
        .body("{}")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(body) = resp.json::<Value>().await {
                if let Some(email) = body
                    .get("userStatus")
                    .and_then(|u| u.get("email"))
                    .and_then(|e| e.as_str())
                {
                    if !email.is_empty() {
                        return Some(email.to_string());
                    }
                }
            }
        }
    }

    // 2. Fallback on GetLoadCodeAssist
    let url = format!("http://{ls_addr}/exa.language_server_pb.LanguageServerService/GetLoadCodeAssist");
    let resp = ctx
        .http
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("x-codeium-csrf-token", csrf)
        .body("{}")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: Value = resp.json().await.ok()?;
    let uri = body
        .get("response")
        .and_then(|r| r.get("currentTier"))
        .and_then(|t| t.get("upgradeSubscriptionUri"))
        .and_then(|u| u.as_str())?;
    let re = regex::Regex::new(r"Email=([^&]+)").ok()?;
    let caps = re.captures(uri)?;
    let raw = caps.get(1)?.as_str();
    Some(decode_percent(raw))
}

fn decode_percent(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            if let (Some(h1), Some(h2)) = (bytes.next(), bytes.next()) {
                let hex_str = format!("{}{}", h1 as char, h2 as char);
                if let Ok(num) = u8::from_str_radix(&hex_str, 16) {
                    out.push(num as char);
                    continue;
                }
            }
        }
        out.push(b as char);
    }
    out
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
    Ok(snap)
}

async fn fetch_ccpa_for_account(ctx: &Ctx, email: &str, token_info: &StoredToken) -> Result<Snapshot> {
    let access_token = ensure_valid_token(ctx, token_info).await?;

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
                    snap.account = Some(email.to_string());
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
    snap.account = Some(email.to_string());
    Ok(snap)
}

fn load_all_accounts(ctx: &Ctx) -> Vec<(String, StoredToken)> {
    let mut accounts = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let accounts_dir = ctx.home.join(".gemini/antigravity-cli/accounts");

    // 1. Scan accounts directory
    if let Ok(entries) = std::fs::read_dir(&accounts_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Some((email, token)) = read_account_file(&path) {
                    if seen.insert(email.clone()) {
                        accounts.push((email, token));
                    }
                }
            }
        }
    }

    // 2. Also check active account from google_accounts.json if not present
    let active_json = ctx.home.join(".gemini/google_accounts.json");
    if let Ok(content) = std::fs::read_to_string(&active_json) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            if let Some(active) = v.get("active").and_then(|a| a.as_str()) {
                let file = accounts_dir.join(format!("{active}.json"));
                if let Some((email, token)) = read_account_file(&file) {
                    if seen.insert(email.clone()) {
                        accounts.push((email, token));
                    }
                }
            }
        }
    }

    // 3. Fallback on macOS Keychain
    #[cfg(target_os = "macos")]
    if let Some((email, token)) = read_keychain_account() {
        if seen.insert(email.clone()) {
            accounts.push((email, token));
        }
    }

    // Put active account first if present
    let active_json = ctx.home.join(".gemini/google_accounts.json");
    if let Ok(content) = std::fs::read_to_string(&active_json) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            if let Some(active) = v.get("active").and_then(|a| a.as_str()) {
                if let Some(idx) = accounts.iter().position(|(e, _)| e == active) {
                    let active_acc = accounts.remove(idx);
                    accounts.insert(0, active_acc);
                }
            }
        }
    }

    accounts
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
            "Gemini Models" | "Gemini" => "Gemini 3.8 Flash",
            "Claude and GPT models" | "Claude & GPT" => "Claude & GPT",
            "Gemini 4 Argon" | "Argon" => "Argon 4",
            other => other,
        };

        let buckets = group.get("buckets").and_then(|b| b.as_array());
        if let Some(buckets) = buckets {
            for b in buckets {
                let bucket_id = b.get("bucketId").and_then(|s| s.as_str()).unwrap_or("quota");
                let is_argon = bucket_id.contains("argon") || group_name.contains("Argon");

                // Never skip Argon 4 even if disabled
                if b.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false) && !is_argon {
                    continue;
                }

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

                if bucket_id.starts_with("3p-") || is_argon {
                    has_pro_bucket = true;
                }

                let final_scope = if is_argon { "Argon 4" } else { scope };

                snap.windows.push(
                    QuotaWindow::new(bucket_id, window_mins, used_percent)
                        .kind(kind)
                        .scope(final_scope)
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

pub fn update_accounts_payload(existing_json: Option<&str>, new_email: &str) -> Value {
    let mut old_accounts: Vec<String> = Vec::new();
    let mut prev_active: Option<String> = None;

    if let Some(content) = existing_json {
        if let Ok(v) = serde_json::from_str::<Value>(content) {
            if let Some(act) = v.get("active").and_then(|a| a.as_str()) {
                let trimmed = act.trim();
                if !trimmed.is_empty() {
                    prev_active = Some(trimmed.to_string());
                }
            }
            if let Some(arr) = v.get("old").and_then(|o| o.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        let trimmed = s.trim();
                        if !trimmed.is_empty() {
                            old_accounts.push(trimmed.to_string());
                        }
                    }
                }
            }
        }
    }

    if let Some(prev) = prev_active {
        if prev != new_email {
            old_accounts.insert(0, prev);
        }
    }

    let mut deduped_old = Vec::new();
    let mut seen = std::collections::HashSet::new();
    seen.insert(new_email.to_string());
    for acc in old_accounts {
        if seen.insert(acc.clone()) {
            deduped_old.push(acc);
        }
    }

    serde_json::json!({
        "active": new_email,
        "old": deduped_old
    })
}

pub fn switch_active_account(ctx: &Ctx, email: &str) -> std::io::Result<()> {
    let path = ctx.home.join(".gemini/google_accounts.json");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let existing = std::fs::read_to_string(&path).ok();
    let payload = update_accounts_payload(existing.as_deref(), email);
    std::fs::write(path, serde_json::to_string_pretty(&payload)?)
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
        assert_eq!(g_5h.scope.as_deref(), Some("Gemini 3.8 Flash"));

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

    #[test]
    fn parses_gemini_4_argon_group() {
        let fixture = serde_json::json!({
            "response": {
                "groups": [
                    {
                        "displayName": "Gemini 4 Argon",
                        "buckets": [
                            {
                                "bucketId": "gemini-argon-wtu-weekly",
                                "displayName": "Gemini 4 Argon Weekly Limit",
                                "window": "weekly",
                                "remainingFraction": 0.0,
                                "disabled": true,
                                "resetTime": "2026-11-14T08:51:27Z"
                            }
                        ]
                    }
                ]
            }
        });

        let s = parse_quota_summary(&fixture).expect("parsed");
        assert_eq!(s.windows.len(), 1);
        assert_eq!(s.windows[0].scope.as_deref(), Some("Argon 4"));
        assert_eq!(s.windows[0].id, "gemini-argon-wtu-weekly");
        assert_eq!(s.windows[0].remaining_percent(), 0.0);
        assert_eq!(s.plan.as_deref(), Some("Pro"));
    }

    #[test]
    fn ensure_account_models_complete_fills_missing_argon_as_zero() {
        let mut snap = Snapshot::new(ProviderId::Agy, "test");
        let email = "robin421@gmail.com";
        // User has only Claude & GPT
        let mut claude_w = QuotaWindow::new("robin421_3p-weekly", Some(10080), 76.0)
            .kind(WindowKind::Weekly)
            .scope("Claude & GPT (robin421)");
        claude_w.account = Some(email.to_string());
        snap.windows.push(claude_w);

        ensure_account_models_complete(&mut snap, email, true);

        let argon = snap.windows.iter().find(|w| w.scope.as_deref() == Some("Argon 4 (robin421)")).expect("argon present");
        assert_eq!(argon.remaining_percent(), 0.0, "missing argon should have 0% remaining");
        assert!(argon.resets_at.is_none(), "unallocated argon should have no reset time");
    }

    #[test]
    fn update_accounts_payload_initial_creation() {
        let payload = update_accounts_payload(None, "user1@example.com");
        assert_eq!(payload["active"], "user1@example.com");
        assert_eq!(payload["old"], serde_json::json!([]));
    }

    #[test]
    fn update_accounts_payload_preserves_old_history() {
        let existing = r#"{
            "active": "user1@example.com",
            "old": ["user2@example.com", "user3@example.com"]
        }"#;
        let payload = update_accounts_payload(Some(existing), "user4@example.com");
        assert_eq!(payload["active"], "user4@example.com");
        let old: Vec<String> = serde_json::from_value(payload["old"].clone()).unwrap();
        assert_eq!(old, vec!["user1@example.com", "user2@example.com", "user3@example.com"]);
    }

    #[test]
    fn update_accounts_payload_switching_to_existing_old() {
        let existing = r#"{
            "active": "user1@example.com",
            "old": ["user2@example.com", "user3@example.com"]
        }"#;
        let payload = update_accounts_payload(Some(existing), "user2@example.com");
        assert_eq!(payload["active"], "user2@example.com");
        let old: Vec<String> = serde_json::from_value(payload["old"].clone()).unwrap();
        // user2 is now active, so old contains user1 and user3, without user2
        assert_eq!(old, vec!["user1@example.com", "user3@example.com"]);
    }

    #[test]
    fn update_accounts_payload_same_account_noop() {
        let existing = r#"{
            "active": "user1@example.com",
            "old": ["user2@example.com"]
        }"#;
        let payload = update_accounts_payload(Some(existing), "user1@example.com");
        assert_eq!(payload["active"], "user1@example.com");
        let old: Vec<String> = serde_json::from_value(payload["old"].clone()).unwrap();
        assert_eq!(old, vec!["user2@example.com"]);
    }
}
