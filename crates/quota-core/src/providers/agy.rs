//! Antigravity (`agy`) quota fetching via Language Server Connect-RPC or Cloud Code PA endpoint.
//!
//! Antigravity manages Google, Claude, and GPT model quotas in weekly and 5-hour rolling windows.
//! When running within an active Antigravity session, it connects to the local Language Server
//! via `$ANTIGRAVITY_LS_ADDRESS` and `$ANTIGRAVITY_CSRF_TOKEN`.
//! Otherwise, it refreshes OAuth credentials stored in `~/.gemini/antigravity-cli/accounts/`
//! and calls `https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ctx::Ctx;
use crate::error::{FetchError, Result};
use crate::model::{ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::parse_time;

const CCPA_QUOTA_ENDPOINT: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:retrieveUserQuotaSummary";
const CCPA_DAILY_QUOTA_ENDPOINT: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:retrieveUserQuota";
const CCPA_USER_AGENT: &str = "antigravity/1.5.12";
const CCPA_DAILY_USER_AGENT: &str = "antigravity/1.5.12 darwin/arm64";
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

#[derive(Deserialize, Serialize, Debug, Clone)]
#[allow(dead_code)]
struct StoredToken {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expiry: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
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

    // 1. Try local Language Server (supporting environment vars, cached active LS, and auto-discovery)
    let (ls_snap, ls_account) = match resolve_and_fetch_ls(ctx).await {
        Some((snap, email)) => (Some(snap), email),
        None => (None, None),
    };

    let mut combined = Snapshot::new(ProviderId::Agy, if ls_snap.is_some() { "antigravity_ls" } else { "oauth" });
    let mut account_names = Vec::new();
    let mut has_pro = false;
    let mut last_err: Option<FetchError> = None;

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

        match fetch_ccpa_for_account(ctx, email, token_info).await {
            Ok(mut snap) => {
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
            Err(err) => {
                last_err = Some(err);
            }
        }
    }

    let active_account = detect_active_account(ctx);
    finalize_combined_snapshot(
        combined,
        &account_names,
        has_pro,
        is_multi_account,
        last_err,
        active_account.as_deref(),
    )
}

fn finalize_combined_snapshot(
    mut combined: Snapshot,
    succeeded_accounts: &[String],
    has_pro: bool,
    _is_multi_account: bool,
    last_err: Option<FetchError>,
    active_account: Option<&str>,
) -> Result<Snapshot> {
    if succeeded_accounts.is_empty() {
        return Err(last_err.unwrap_or_else(|| FetchError::parse("no active quota buckets found")));
    }

    if combined.windows.is_empty() {
        return Err(last_err.unwrap_or_else(|| FetchError::parse("no active quota buckets found")));
    }

    let mut ordered_accounts = succeeded_accounts.to_vec();
    if let Some(active) = active_account {
        ordered_accounts.sort_by_key(|a| a != active);
    }
    combined.account = Some(ordered_accounts.join(", "));
    combined.plan = Some(if has_pro { "Pro".into() } else { "Antigravity".into() });

    // Sort windows: active account first, then by account, scope, and shorter windows first.
    combined.windows.sort_by_key(|w| {
        (
            w.account.as_deref() != active_account,
            w.account.clone(),
            w.scope.clone(),
            w.window_mins.unwrap_or(u32::MAX),
        )
    });

    Ok(combined)
}

#[cfg(test)]
fn is_argon_scope(scope: &str) -> bool {
    scope.contains("Argon")
}

#[cfg(test)]
fn is_gemini_scope(scope: &str) -> bool {
    !is_argon_scope(scope)
        && (scope.contains("Gemini")
            || scope.contains("Neon")
            || scope.contains("Hydrogen")
            || scope.contains("Flash"))
}

/// Format a concise, live-updating display scope from LS `displayName` and `description`.
pub fn format_dynamic_scope(group_name: &str, description: Option<&str>) -> String {
    let fallback = {
        let trimmed = group_name.trim();
        if trimmed.is_empty() {
            "Antigravity".to_string()
        } else {
            trimmed.to_string()
        }
    };

    let Some(desc) = description else {
        return fallback;
    };

    let marker = "models within this group:";
    let lower = desc.to_ascii_lowercase();
    let Some(idx) = lower.find(marker) else {
        return fallback;
    };

    let after = &desc[idx + marker.len()..];
    let models: Vec<String> = after
        .split(',')
        .map(|s| {
            let base = match s.find('(') {
                Some(p) => &s[..p],
                None => s,
            };
            base.trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();

    if models.is_empty() {
        return fallback;
    }
    if models.len() == 1 {
        return models[0].clone();
    }

    let first_words: Vec<&str> = models[0].split_whitespace().collect();
    let prefix2 = if first_words.len() >= 2 {
        Some(format!("{} {} ", first_words[0], first_words[1]))
    } else {
        None
    };
    let prefix1 = first_words.first().map(|w| format!("{w} "));

    let mut parts = Vec::with_capacity(models.len());
    parts.push(models[0].clone());

    for m in models.iter().skip(1) {
        let shortened = if let Some(ref p2) = prefix2 {
            if let Some(rest) = m.strip_prefix(p2.as_str()) {
                rest.trim()
            } else if let Some(ref p1) = prefix1 {
                m.strip_prefix(p1.as_str()).map(str::trim).unwrap_or(m.as_str())
            } else {
                m.as_str()
            }
        } else if let Some(ref p1) = prefix1 {
            m.strip_prefix(p1.as_str()).map(str::trim).unwrap_or(m.as_str())
        } else {
            m.as_str()
        };

        if !shortened.is_empty() {
            parts.push(shortened.to_string());
        }
    }

    parts.join(" · ")
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

fn last_ls_file(ctx: &Ctx) -> PathBuf {
    ctx.home.join(".gemini/antigravity-cli/last_ls.json")
}

fn save_last_ls(ctx: &Ctx, addr: &str, csrf: &str) {
    let path = last_ls_file(ctx);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let info = serde_json::json!({
        "ls_address": addr,
        "csrf_token": csrf,
        "updated_at": Utc::now().to_rfc3339(),
    });
    let _ = std::fs::write(path, serde_json::to_string_pretty(&info).unwrap_or_default());
}

fn load_last_ls(ctx: &Ctx) -> Option<(String, String)> {
    let path = last_ls_file(ctx);
    let content = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&content).ok()?;
    let addr = v.get("ls_address")?.as_str()?.to_string();
    let csrf = v.get("csrf_token").and_then(|c| c.as_str()).unwrap_or_default().to_string();
    if addr.is_empty() {
        None
    } else {
        Some((addr, csrf))
    }
}

fn detect_latest_ls_port_from_log(ctx: &Ctx) -> Option<u16> {
    let log_path = ctx.home.join(".gemini/antigravity-cli/cli.log");
    let content = std::fs::read_to_string(log_path).ok()?;
    for line in content.lines().take(50) {
        if line.contains("Language server listening on random port at") && line.contains("for HTTP") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            for w in parts.windows(2) {
                if w[1] == "for" {
                    if let Ok(port) = w[0].parse::<u16>() {
                        return Some(port);
                    }
                }
            }
        }
    }
    None
}

async fn resolve_and_fetch_ls(ctx: &Ctx) -> Option<(Snapshot, Option<String>)> {
    let mut candidates: Vec<(String, String)> = Vec::new();

    // 1. From environment variable (if currently set)
    if let Ok(addr) = std::env::var("ANTIGRAVITY_LS_ADDRESS") {
        let trimmed = addr.trim().to_string();
        if !trimmed.is_empty() {
            let csrf = std::env::var("ANTIGRAVITY_CSRF_TOKEN").unwrap_or_default().trim().to_string();
            candidates.push((trimmed, csrf));
        }
    }

    // 2. From persisted last_ls.json (cross-session / desktop GUI fallback)
    if let Some((addr, csrf)) = load_last_ls(ctx) {
        if !candidates.iter().any(|(a, _)| a == &addr) {
            candidates.push((addr, csrf));
        }
    }

    // 3. From cli.log (in case language server port changed dynamically)
    if let Some(port) = detect_latest_ls_port_from_log(ctx) {
        let addr = format!("localhost:{port}");
        let known_csrf = candidates
            .iter()
            .find_map(|(_, c)| if !c.is_empty() { Some(c.clone()) } else { None })
            .or_else(|| load_last_ls(ctx).map(|(_, c)| c).filter(|c| !c.is_empty()))
            .unwrap_or_default();
        if !known_csrf.is_empty() && !candidates.iter().any(|(a, _)| a == &addr) {
            candidates.push((addr, known_csrf));
        }
    }

    // Try candidates in order
    for (addr, csrf) in candidates {
        if let Ok(snap) = fetch_from_ls(ctx, &addr, &csrf).await {
            let email = get_ls_account_email(ctx, &addr, &csrf).await;
            save_last_ls(ctx, &addr, &csrf);
            return Some((snap, email));
        }
    }

    None
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

#[cfg(test)]
fn has_gemini_window(snap: &Snapshot) -> bool {
    snap.windows
        .iter()
        .any(|w| w.scope.as_deref().is_some_and(is_gemini_scope))
}

async fn fetch_daily_quotas(ctx: &Ctx, access_token: &str) -> Vec<QuotaWindow> {
    let resp = match ctx
        .http
        .post(CCPA_DAILY_QUOTA_ENDPOINT)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", CCPA_DAILY_USER_AGENT)
        .header("Content-Type", "application/json")
        .body(r#"{"project":"aicode-consumers"}"#)
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    if !resp.status().is_success() {
        return Vec::new();
    }

    let body: Value = match resp.json().await {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    parse_daily_quotas(&body)
}

fn normalize_daily_model_base(model_id: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "-xhigh",
        "-high",
        "-medium",
        "-low",
        "-minimal",
        "-max",
        "-none",
        "-thinking",
        "-tiered",
        "-lite",
    ];
    let mut base = model_id.trim().to_ascii_lowercase();
    loop {
        let mut stripped = false;
        for suffix in SUFFIXES {
            if let Some(rest) = base.strip_suffix(suffix) {
                if !rest.is_empty() {
                    base = rest.to_string();
                    stripped = true;
                    break;
                }
            }
        }
        if !stripped {
            break;
        }
    }
    base
}

fn format_model_id_scope(model_id: &str) -> String {
    let parts: Vec<String> = model_id
        .split('-')
        .filter(|p| !p.is_empty())
        .map(|p| match p.to_ascii_lowercase().as_str() {
            "gpt" => "GPT".to_string(),
            "oss" => "OSS".to_string(),
            "120b" => "120B".to_string(),
            _ => {
                let mut chars = p.chars();
                match chars.next() {
                    Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                    None => String::new(),
                }
            }
        })
        .collect();
    if parts.is_empty() {
        "Gemini Models".to_string()
    } else {
        parts.join(" ")
    }
}

fn gemini_version_key(base_id: &str) -> (u32, u32, u8) {
    let mut major = 0u32;
    let mut minor = 0u32;
    for part in base_id.split('-') {
        if let Some((maj_s, min_s)) = part.split_once('.') {
            if let (Ok(maj), Ok(min)) = (maj_s.parse::<u32>(), min_s.parse::<u32>()) {
                major = maj;
                minor = min;
                break;
            }
        } else if let Ok(maj) = part.parse::<u32>() {
            major = maj;
            break;
        }
    }
    let importance = if base_id.contains("neon")
        || base_id.contains("hydrogen")
        || base_id.contains("orchestrator")
        || base_id.contains("sonar")
    {
        3
    } else if base_id.contains("pro") {
        2
    } else if base_id.contains("image") {
        0
    } else {
        1
    };
    (major, minor, importance)
}

fn join_gemini_scopes(base_ids: &[String]) -> String {
    let mut sorted = base_ids.to_vec();
    sorted.sort_by(|a, b| {
        let ka = gemini_version_key(a);
        let kb = gemini_version_key(b);
        kb.cmp(&ka).then_with(|| a.cmp(b))
    });

    let formatted: Vec<String> = sorted
        .into_iter()
        .take(4)
        .map(|id| format_model_id_scope(&id))
        .collect();

    if formatted.is_empty() {
        return "Gemini".to_string();
    }
    if formatted.len() == 1 {
        return formatted[0].clone();
    }

    let first_words: Vec<&str> = formatted[0].split_whitespace().collect();
    let prefix2 = if first_words.len() >= 2 && first_words[0] == "Gemini" {
        Some(format!("{} {} ", first_words[0], first_words[1]))
    } else {
        None
    };
    let prefix1 = if first_words.first() == Some(&"Gemini") {
        Some("Gemini ".to_string())
    } else {
        None
    };

    let mut parts = Vec::with_capacity(formatted.len());
    parts.push(formatted[0].clone());
    for m in formatted.iter().skip(1) {
        let shortened = if let Some(ref p2) = prefix2 {
            if let Some(rest) = m.strip_prefix(p2.as_str()) {
                rest.trim()
            } else if let Some(ref p1) = prefix1 {
                m.strip_prefix(p1.as_str()).map(str::trim).unwrap_or(m.as_str())
            } else {
                m.as_str()
            }
        } else if let Some(ref p1) = prefix1 {
            m.strip_prefix(p1.as_str()).map(str::trim).unwrap_or(m.as_str())
        } else {
            m.as_str()
        };
        if !shortened.is_empty() && !parts.iter().any(|p| p == shortened) {
            parts.push(shortened.to_string());
        }
    }
    parts.join(" · ")
}

pub fn parse_daily_quotas(body: &Value) -> Vec<QuotaWindow> {
    let Some(buckets) = body
        .get("buckets")
        .or_else(|| body.get("response").and_then(|r| r.get("buckets")))
        .and_then(|b| b.as_array())
    else {
        return Vec::new();
    };

    let mut grouped: BTreeMap<String, (f64, Option<chrono::DateTime<Utc>>)> = BTreeMap::new();

    for b in buckets {
        let raw_model_id = match b.get("modelId").and_then(|m| m.as_str()) {
            Some(id) if !id.trim().is_empty() => id,
            _ => continue,
        };
        if raw_model_id.trim().to_ascii_lowercase().starts_with("tab_") {
            continue;
        }
        let remaining_fraction = match b.get("remainingFraction").and_then(|f| f.as_f64()) {
            Some(f) if f.is_finite() => f,
            _ => continue,
        };
        let reset_time = b.get("resetTime").and_then(parse_time);
        let base_id = normalize_daily_model_base(raw_model_id);
        if base_id.is_empty() || base_id.starts_with("tab_") {
            continue;
        }

        grouped
            .entry(base_id)
            .and_modify(|(min_rem, best_reset)| {
                if remaining_fraction < *min_rem {
                    *min_rem = remaining_fraction;
                    *best_reset = reset_time.or(*best_reset);
                } else if (remaining_fraction - *min_rem).abs() < f64::EPSILON && best_reset.is_none() {
                    *best_reset = reset_time;
                }
            })
            .or_insert((remaining_fraction, reset_time));
    }

    let mut out = Vec::new();
    let mut gemini_pools: BTreeMap<(i64, Option<i64>), (f64, Option<chrono::DateTime<Utc>>, Vec<String>)> =
        BTreeMap::new();

    for (base_id, (min_remaining, reset_time)) in grouped {
        let used_percent = ((1.0 - min_remaining) * 100.0).clamp(0.0, 100.0);
        if base_id.contains("argon") {
            let scope = if base_id == "gemini-argon" {
                "Gemini 4 Argon".to_string()
            } else {
                format_model_id_scope(&base_id)
            };
            out.push(
                QuotaWindow::new(format!("daily-{base_id}"), Some(300), used_percent)
                    .kind(WindowKind::Session)
                    .scope(scope)
                    .resets_at(reset_time),
            );
        } else if base_id.starts_with("gemini") {
            let pool_key = ((used_percent * 100.0).round() as i64, reset_time.map(|t| t.timestamp()));
            gemini_pools
                .entry(pool_key)
                .and_modify(|(_, _, ids)| ids.push(base_id.clone()))
                .or_insert_with(|| (used_percent, reset_time, vec![base_id]));
        } else {
            let scope = format_model_id_scope(&base_id);
            out.push(
                QuotaWindow::new(format!("daily-{base_id}"), Some(300), used_percent)
                    .kind(WindowKind::Session)
                    .scope(scope)
                    .resets_at(reset_time),
            );
        }
    }

    for (_, (used_percent, reset_time, base_ids)) in gemini_pools {
        let id = if base_ids.len() == 1 {
            format!("daily-{}", base_ids[0])
        } else {
            let mut sorted = base_ids.clone();
            sorted.sort_by(|a, b| gemini_version_key(b).cmp(&gemini_version_key(a)).then_with(|| a.cmp(b)));
            format!("daily-{}", sorted[0])
        };
        let scope = join_gemini_scopes(&base_ids);
        out.push(
            QuotaWindow::new(id, Some(300), used_percent)
                .kind(WindowKind::Session)
                .scope(scope)
                .resets_at(reset_time),
        );
    }

    out
}

/// Compatibility helper returning the tightest Gemini daily quota window if present.
#[cfg(test)]
pub fn parse_daily_gemini_quota(body: &Value) -> Option<QuotaWindow> {
    parse_daily_quotas(body)
        .into_iter()
        .filter(|w| {
            w.scope
                .as_deref()
                .is_some_and(|s| s.to_ascii_lowercase().contains("gemini"))
        })
        .max_by(|a, b| a.used_percent.total_cmp(&b.used_percent))
}

fn normalize_scope_for_match(s: &str) -> String {
    s.to_ascii_lowercase()
        .chars()
        .map(|c| if c == '.' || c == '-' || c == '_' { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn scope_already_covered(existing_windows: &[QuotaWindow], candidate_scope: &str) -> bool {
    let norm_candidate = normalize_scope_for_match(candidate_scope);
    let short_candidate = norm_candidate
        .strip_prefix("gemini ")
        .or_else(|| norm_candidate.strip_prefix("claude "))
        .unwrap_or(norm_candidate.as_str());

    existing_windows.iter().any(|existing| {
        let Some(ref es) = existing.scope else {
            return false;
        };
        let norm_es = normalize_scope_for_match(es);
        norm_es == norm_candidate
            || norm_es.contains(&norm_candidate)
            || (short_candidate.len() >= 3 && norm_es.contains(short_candidate))
    })
}

async fn fetch_ccpa_for_account(ctx: &Ctx, email: &str, token_info: &StoredToken) -> Result<Snapshot> {
    let mut active_token = ensure_valid_token(ctx, token_info).await?;

    let mut resp = ctx
        .http
        .post(CCPA_QUOTA_ENDPOINT)
        .header("Authorization", format!("Bearer {active_token}"))
        .header("User-Agent", CCPA_USER_AGENT)
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
                    .header("User-Agent", CCPA_USER_AGENT)
                    .header("Content-Type", "application/json")
                    .body("{}")
                    .send()
                    .await?;

                if retry_resp.status().is_success() {
                    active_token = new_token;
                    resp = retry_resp;
                } else {
                    return Err(FetchError::AuthExpired);
                }
            } else {
                return Err(FetchError::AuthExpired);
            }
        } else {
            return Err(FetchError::AuthExpired);
        }
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

    let daily_windows = fetch_daily_quotas(ctx, &active_token).await;
    for w in daily_windows {
        let Some(ref scope) = w.scope else {
            continue;
        };
        if !scope_already_covered(&snap.windows, scope) {
            snap.windows.push(w);
        }
    }

    Ok(snap)
}

fn token_completeness_score(token: &StoredToken) -> u8 {
    let mut score = 0;
    if token.refresh_token.as_deref().is_some_and(|s| !s.is_empty()) {
        score += 2;
    }
    if token.access_token.as_deref().is_some_and(|s| !s.is_empty()) {
        score += 1;
    }
    score
}

fn should_replace_token(existing: &StoredToken, candidate: &StoredToken) -> bool {
    let existing_exp = existing
        .expiry
        .as_deref()
        .and_then(|s| parse_time(&Value::String(s.to_string())));
    let candidate_exp = candidate
        .expiry
        .as_deref()
        .and_then(|s| parse_time(&Value::String(s.to_string())));

    match (existing_exp, candidate_exp) {
        (Some(e), Some(c)) if c != e => c > e,
        (None, Some(_)) => true,
        (Some(_), None) => false,
        _ => token_completeness_score(candidate) > token_completeness_score(existing),
    }
}

fn merge_account_token(map: &mut BTreeMap<String, StoredToken>, email: String, token: StoredToken) {
    match map.get(&email) {
        Some(existing) if !should_replace_token(existing, &token) => {}
        _ => {
            map.insert(email, token);
        }
    }
}

fn read_google_accounts_active(home: &std::path::Path) -> Option<String> {
    let active_json = home.join(".gemini/google_accounts.json");
    std::fs::read_to_string(&active_json)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .and_then(|v| v.get("active").and_then(|a| a.as_str()).map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
}

/// Detect the currently active Antigravity account email.
/// Priority: 1) macOS Keychain (`service="gemini", account="antigravity"`), 2) `~/.gemini/google_accounts.json`.
pub fn detect_active_account_from_home(home: &std::path::Path) -> Option<String> {
    #[cfg(target_os = "macos")]
    if let Some((email, _)) = read_keychain_account() {
        let trimmed = email.trim();
        if !trimmed.is_empty() && trimmed != "antigravity-user" {
            return Some(trimmed.to_string());
        }
    }

    read_google_accounts_active(home)
}

pub fn detect_active_account(ctx: &Ctx) -> Option<String> {
    detect_active_account_from_home(&ctx.home)
}

fn cached_accounts_dir(ctx: &Ctx) -> PathBuf {
    ctx.app_config_dir().join("codexusagebar/agy_accounts")
}

fn sanitize_account_filename(email: &str) -> String {
    let sanitized: String = email
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '@' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "account".to_string()
    } else {
        sanitized
    }
}

fn persist_cached_account(ctx: &Ctx, email: &str, token: &StoredToken) {
    let trimmed = email.trim();
    if trimmed.is_empty() || trimmed == "antigravity-user" {
        return;
    }

    let dir = cached_accounts_dir(ctx);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(format!("{}.json", sanitize_account_filename(trimmed)));

    if path.exists() {
        if let Some((_, existing_token)) = read_account_file(&path) {
            if !should_replace_token(&existing_token, token) {
                return;
            }
        }
    }

    let payload = AccountFile {
        token: Some(token.clone()),
        id_token_email: Some(trimmed.to_string()),
    };
    let Ok(json) = serde_json::to_string_pretty(&payload) else {
        return;
    };
    if std::fs::write(&path, json).is_err() {
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

fn scan_accounts_dir(dir: &std::path::Path, accounts_map: &mut BTreeMap<String, StoredToken>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                if let Some((email, token)) = read_account_file(&path) {
                    merge_account_token(accounts_map, email, token);
                }
            }
        }
    }
}

fn load_all_accounts(ctx: &Ctx) -> Vec<(String, StoredToken)> {
    #[cfg(target_os = "macos")]
    let keychain = read_keychain_account();
    #[cfg(not(target_os = "macos"))]
    let keychain: Option<(String, StoredToken)> = None;

    load_all_accounts_with_keychain(ctx, keychain)
}

fn load_all_accounts_with_keychain(
    ctx: &Ctx,
    keychain: Option<(String, StoredToken)>,
) -> Vec<(String, StoredToken)> {
    let mut accounts_map: BTreeMap<String, StoredToken> = BTreeMap::new();

    let accounts_dir = ctx.home.join(".gemini/antigravity-cli/accounts");
    let cache_dir = cached_accounts_dir(ctx);

    // 1. Scan antigravity-cli accounts directory and CodexUsageBar persistent cache
    scan_accounts_dir(&accounts_dir, &mut accounts_map);
    scan_accounts_dir(&cache_dir, &mut accounts_map);

    // 2. Also check active account from google_accounts.json if not present
    let google_active_email = read_google_accounts_active(&ctx.home);

    if let Some(ref active) = google_active_email {
        let file = accounts_dir.join(format!("{active}.json"));
        if let Some((email, token)) = read_account_file(&file) {
            merge_account_token(&mut accounts_map, email, token);
        }
    }

    // 3. Merge macOS Keychain credential (preferring newer token over stale JSON files)
    //    and persist to cache so subsequent Keychain account switches don't lose prior accounts.
    let mut keychain_email: Option<String> = None;
    if let Some((email, token)) = keychain {
        if !email.trim().is_empty() && email != "antigravity-user" {
            keychain_email = Some(email.clone());
        }
        merge_account_token(&mut accounts_map, email.clone(), token.clone());
        persist_cached_account(ctx, &email, &token);
    }

    let active_email = keychain_email.or(google_active_email);
    let mut accounts: Vec<(String, StoredToken)> = accounts_map.into_iter().collect();

    // Put active account first if present
    if let Some(ref active) = active_email {
        if let Some(idx) = accounts.iter().position(|(e, _)| e == active) {
            let active_acc = accounts.remove(idx);
            accounts.insert(0, active_acc);
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
        let description = group.get("description").and_then(|d| d.as_str());
        let scope = format_dynamic_scope(group_name, description);

        let buckets = group.get("buckets").and_then(|b| b.as_array());
        if let Some(buckets) = buckets {
            for b in buckets {
                let bucket_id = b.get("bucketId").and_then(|s| s.as_str()).unwrap_or("quota");
                let is_argon = bucket_id.contains("argon") || group_name.contains("Argon");

                // Never skip Argon even if disabled
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

                snap.windows.push(
                    QuotaWindow::new(bucket_id, window_mins, used_percent)
                        .kind(kind)
                        .scope(scope.as_str())
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
        assert_eq!(g_5h.scope.as_deref(), Some("Gemini Models"));

        let p_weekly = s.windows.iter().find(|w| w.id == "3p-weekly").unwrap();
        assert_eq!(p_weekly.remaining_percent(), 0.0);
        assert_eq!(p_weekly.kind, WindowKind::Weekly);
        assert_eq!(p_weekly.scope.as_deref(), Some("Claude and GPT models"));
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
        assert_eq!(s.windows[0].scope.as_deref(), Some("Gemini 4 Argon"));
        assert_eq!(s.windows[0].id, "gemini-argon-wtu-weekly");
        assert_eq!(s.windows[0].remaining_percent(), 0.0);
        assert_eq!(s.plan.as_deref(), Some("Pro"));
    }

    #[test]
    fn finalize_does_not_fabricate_missing_argon_or_gemini_weekly() {
        let mut snap = Snapshot::new(ProviderId::Agy, "test");
        let email = "robin421@gmail.com";
        let mut claude_w = QuotaWindow::new("robin421_3p-weekly", Some(10080), 76.0)
            .kind(WindowKind::Weekly)
            .scope("Claude & GPT (robin421)");
        claude_w.account = Some(email.to_string());
        snap.windows.push(claude_w);

        let snap = finalize_combined_snapshot(snap, &[email.to_string()], true, true, None, Some(email)).expect("ok");

        assert_eq!(snap.windows.len(), 1);
        assert!(snap.windows.iter().all(|w| !w.scope.as_deref().is_some_and(is_argon_scope)));
        assert!(snap.windows.iter().all(|w| w.scope.as_deref() != Some("Gemini Models")));
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

    #[test]
    fn finalize_returns_err_when_no_account_succeeded() {
        let combined = Snapshot::new(ProviderId::Agy, "oauth");
        let err = finalize_combined_snapshot(
            combined,
            &[],
            false,
            true,
            Some(FetchError::AuthExpired),
            None,
        )
        .expect_err("must not fabricate fake windows when all accounts fail");
        assert!(matches!(err, FetchError::AuthExpired));
    }

    #[test]
    fn finalize_returns_parse_err_when_no_account_succeeded_and_no_last_err() {
        let combined = Snapshot::new(ProviderId::Agy, "oauth");
        let err = finalize_combined_snapshot(combined, &[], false, false, None, None)
            .expect_err("must return parse error when no accounts succeeded");
        assert!(matches!(err, FetchError::Parse(_)));
    }

    #[test]
    fn finalize_only_completes_succeeded_accounts() {
        let mut combined = Snapshot::new(ProviderId::Agy, "oauth");
        let mut w = QuotaWindow::new("ok_3p-weekly", Some(10080), 40.0)
            .kind(WindowKind::Weekly)
            .scope("Claude & GPT (ok)");
        w.account = Some("ok@example.com".to_string());
        combined.windows.push(w);

        let succeeded = vec!["ok@example.com".to_string()];
        let snap = finalize_combined_snapshot(
            combined,
            &succeeded,
            true,
            true,
            Some(FetchError::AuthExpired),
            None,
        )
        .expect("should succeed for valid account");

        assert_eq!(snap.account.as_deref(), Some("ok@example.com"));
        assert_eq!(snap.plan.as_deref(), Some("Pro"));
        assert!(snap.windows.iter().all(|w| w.account.as_deref() == Some("ok@example.com")));
        assert_eq!(snap.windows.len(), 1, "must not invent Gemini or Claude rows the provider did not return");
        assert!(snap.windows.iter().all(|w| w.scope.as_deref() != Some("Gemini Models")));
    }

    #[test]
    fn finalize_places_active_account_before_lexicographically_smaller_accounts() {
        let mut combined = Snapshot::new(ProviderId::Agy, "oauth");
        for (email, short) in [
            ("robin421@gmail.com", "robin421"),
            ("robinsonwen421@gmail.com", "robinsonwen421"),
            ("togxida@gmail.com", "togxida"),
        ] {
            let mut w = QuotaWindow::new(format!("{short}_3p-weekly"), Some(10080), 80.0)
                .kind(WindowKind::Weekly)
                .scope(format!("Claude & GPT ({short})"));
            w.account = Some(email.to_string());
            combined.windows.push(w);
        }

        let succeeded = vec![
            "robin421@gmail.com".to_string(),
            "robinsonwen421@gmail.com".to_string(),
            "togxida@gmail.com".to_string(),
        ];
        let snap = finalize_combined_snapshot(
            combined,
            &succeeded,
            true,
            true,
            None,
            Some("robinsonwen421@gmail.com"),
        )
        .expect("should succeed");

        assert_eq!(
            snap.account.as_deref(),
            Some("robinsonwen421@gmail.com, robin421@gmail.com, togxida@gmail.com")
        );
        let first_account = snap.windows.first().and_then(|w| w.account.as_deref());
        assert_eq!(first_account, Some("robinsonwen421@gmail.com"));

        let mut seen_accounts = Vec::new();
        for w in &snap.windows {
            if let Some(acc) = w.account.as_deref() {
                if seen_accounts.last().copied() != Some(acc) {
                    seen_accounts.push(acc);
                }
            }
        }
        assert_eq!(
            seen_accounts,
            vec![
                "robinsonwen421@gmail.com",
                "robin421@gmail.com",
                "togxida@gmail.com"
            ]
        );
    }

    #[test]
    fn merge_account_token_prefers_newer_expiry() {
        let mut map = BTreeMap::new();
        let stale_json_token = StoredToken {
            access_token: Some("old-access".into()),
            refresh_token: Some("old-refresh".into()),
            expiry: Some("2026-11-06T00:00:00Z".into()),
            expires_in: None,
        };
        let fresh_keychain_token = StoredToken {
            access_token: Some("new-access".into()),
            refresh_token: Some("new-refresh".into()),
            expiry: Some("2026-11-19T12:00:00Z".into()),
            expires_in: None,
        };

        merge_account_token(&mut map, "user@example.com".into(), stale_json_token);
        merge_account_token(&mut map, "user@example.com".into(), fresh_keychain_token);

        let chosen = map.get("user@example.com").expect("account present");
        assert_eq!(chosen.access_token.as_deref(), Some("new-access"));
        assert_eq!(chosen.refresh_token.as_deref(), Some("new-refresh"));
    }

    #[test]
    fn merge_account_token_keeps_newer_existing_over_older_candidate() {
        let mut map = BTreeMap::new();
        let fresh_token = StoredToken {
            access_token: Some("fresh-access".into()),
            refresh_token: Some("fresh-refresh".into()),
            expiry: Some("2026-11-19T12:00:00Z".into()),
            expires_in: None,
        };
        let older_token = StoredToken {
            access_token: Some("stale-access".into()),
            refresh_token: Some("stale-refresh".into()),
            expiry: Some("2026-11-06T00:00:00Z".into()),
            expires_in: None,
        };

        merge_account_token(&mut map, "user@example.com".into(), fresh_token);
        merge_account_token(&mut map, "user@example.com".into(), older_token);

        let chosen = map.get("user@example.com").expect("account present");
        assert_eq!(chosen.access_token.as_deref(), Some("fresh-access"));
    }

    #[test]
    fn merge_account_token_tiebreaks_by_refresh_and_access_token() {
        let mut map = BTreeMap::new();
        let access_only = StoredToken {
            access_token: Some("acc-only".into()),
            refresh_token: None,
            expiry: None,
            expires_in: None,
        };
        let with_refresh = StoredToken {
            access_token: Some("acc-full".into()),
            refresh_token: Some("ref-full".into()),
            expiry: None,
            expires_in: None,
        };

        merge_account_token(&mut map, "user@example.com".into(), access_only);
        merge_account_token(&mut map, "user@example.com".into(), with_refresh);

        let chosen = map.get("user@example.com").expect("account present");
        assert_eq!(chosen.access_token.as_deref(), Some("acc-full"));
        assert_eq!(chosen.refresh_token.as_deref(), Some("ref-full"));
    }

    #[test]
    fn parse_daily_quotas_extracts_all_models_and_deduplicates_thinking_tiers() {
        let body = serde_json::json!({
            "buckets": [
                {
                    "modelId": "claude-sonnet-4-6",
                    "remainingFraction": 0.10,
                    "resetTime": "2026-11-19T15:00:00Z"
                },
                {
                    "modelId": "gemini-4-neon-high",
                    "remainingFraction": 0.70,
                    "resetTime": "2026-11-19T14:00:00Z"
                },
                {
                    "modelId": "gemini-4-neon-low",
                    "remainingFraction": 0.40,
                    "resetTime": "2026-11-19T14:30:00Z"
                },
                {
                    "modelId": "gemini-3.8-flash-tiered",
                    "remainingFraction": 0.9398,
                    "resetTime": "2026-11-19T16:30:00Z"
                },
                {
                    "modelId": "gemini-3.8-flash-lite",
                    "remainingFraction": 0.85,
                    "resetTime": "2026-11-19T16:00:00Z"
                },
                {
                    "modelId": "gpt-oss-120b",
                    "remainingFraction": 0.50,
                    "resetTime": "2026-11-19T18:00:00Z"
                }
            ]
        });

        let windows = parse_daily_quotas(&body);
        assert_eq!(windows.len(), 4);

        let claude = windows.iter().find(|w| w.id == "daily-claude-sonnet-4-6").expect("claude window");
        assert_eq!(claude.scope.as_deref(), Some("Claude Sonnet 4 6"));
        assert!((claude.used_percent - 90.0).abs() < 0.01);

        let neon = windows.iter().find(|w| w.id == "daily-gemini-4-neon").expect("neon window");
        assert_eq!(neon.scope.as_deref(), Some("Gemini 4 Neon"));
        assert!((neon.used_percent - 60.0).abs() < 0.01);
        assert_eq!(
            neon.resets_at.map(|t| t.to_rfc3339()),
            Some("2026-11-19T14:30:00+00:00".to_string())
        );

        let flash = windows.iter().find(|w| w.id == "daily-gemini-3.8-flash").expect("flash window");
        assert_eq!(flash.scope.as_deref(), Some("Gemini 3.8 Flash"));
        assert!((flash.used_percent - 15.0).abs() < 0.01);

        let gpt = windows.iter().find(|w| w.id == "daily-gpt-oss-120b").expect("gpt window");
        assert_eq!(gpt.scope.as_deref(), Some("GPT OSS 120B"));
        assert!((gpt.used_percent - 50.0).abs() < 0.01);
    }

    #[test]
    fn parse_daily_quotas_filters_tab_and_merges_shared_gemini_pool() {
        let body = serde_json::json!({
            "buckets": [
                { "modelId": "tab_flash_lite_preview", "remainingFraction": 1.0, "resetTime": "2026-11-19T18:00:00Z" },
                { "modelId": "tab_jump_flash_lite_preview", "remainingFraction": 1.0, "resetTime": "2026-11-19T18:00:00Z" },
                { "modelId": "gemini-2.5-flash", "remainingFraction": 0.552, "resetTime": "2026-11-19T16:00:00Z" },
                { "modelId": "gemini-3.5-flash", "remainingFraction": 0.552, "resetTime": "2026-11-19T16:00:00Z" },
                { "modelId": "gemini-3.8-flash", "remainingFraction": 0.552, "resetTime": "2026-11-19T16:00:00Z" },
                { "modelId": "gemini-4-hydrogen-sonar", "remainingFraction": 0.552, "resetTime": "2026-11-19T16:00:00Z" },
                { "modelId": "gemini-4-argon", "remainingFraction": 0.0, "resetTime": "2026-11-25T00:00:00Z" }
            ]
        });

        let windows = parse_daily_quotas(&body);
        assert_eq!(windows.len(), 2, "tab_* must be filtered and shared Gemini pool merged into 1 + 1 argon");
        assert!(windows.iter().all(|w| !w.id.contains("tab_")));

        let argon = windows.iter().find(|w| w.scope.as_deref().is_some_and(|s| s.contains("Argon"))).expect("argon");
        assert!((argon.used_percent - 100.0).abs() < 0.01);

        let merged = windows.iter().find(|w| !w.scope.as_deref().unwrap_or("").contains("Argon")).expect("merged gemini");
        assert_eq!(
            merged.scope.as_deref(),
            Some("Gemini 4 Hydrogen Sonar · 3.8 Flash · 3.5 Flash · 2.5 Flash")
        );
        assert!((merged.used_percent - 44.8).abs() < 0.01);
    }

    #[test]
    fn parse_daily_gemini_quota_compatibility_picks_tightest_gemini() {
        let body = serde_json::json!({
            "buckets": [
                {
                    "modelId": "claude-sonnet-4-6",
                    "remainingFraction": 0.10,
                    "resetTime": "2026-11-19T15:00:00Z"
                },
                {
                    "modelId": "gemini-2.5-pro",
                    "remainingFraction": 0.20,
                    "resetTime": "2026-11-19T14:00:00Z"
                },
                {
                    "modelId": "gemini-3.8-flash-lite",
                    "remainingFraction": 0.85,
                    "resetTime": "2026-11-19T16:00:00Z"
                }
            ]
        });

        let w = parse_daily_gemini_quota(&body).expect("should parse tightest gemini window");
        assert_eq!(w.id, "daily-gemini-2.5-pro");
        assert_eq!(w.window_mins, Some(300));
        assert_eq!(w.kind, WindowKind::Session);
        assert_eq!(w.scope.as_deref(), Some("Gemini 2.5 Pro"));
        assert!((w.used_percent - 80.0).abs() < 0.01);

        let no_gemini = serde_json::json!({
            "buckets": [
                {
                    "modelId": "claude-opus-4-6",
                    "remainingFraction": 0.50
                }
            ]
        });
        assert!(parse_daily_gemini_quota(&no_gemini).is_none());
    }

    #[test]
    fn scope_already_covered_avoids_duplicate_models() {
        let existing = vec![
            QuotaWindow::new("g-wk", Some(10080), 10.0)
                .kind(WindowKind::Weekly)
                .scope("Gemini 4 Neon · Hydrogen · Orchestrator"),
            QuotaWindow::new("3p-wk", Some(10080), 20.0)
                .kind(WindowKind::Weekly)
                .scope("Claude Opus 5.5 · Sonnet 5.5 · GPT-OSS 120B"),
        ];
        assert!(scope_already_covered(&existing, "Gemini 4 Neon"));
        assert!(scope_already_covered(&existing, "Claude Sonnet 5.5"));
        assert!(scope_already_covered(&existing, "Claude Opus 5 5"));
        assert!(scope_already_covered(&existing, "GPT OSS 120B"));
        assert!(!scope_already_covered(&existing, "Gemini 4 Argon"));
        assert!(!scope_already_covered(&existing, "Gemini 3.8 Flash"));
    }

    #[test]
    fn has_gemini_window_detects_missing_and_present_gemini_scope() {
        let mut snap = Snapshot::new(ProviderId::Agy, "oauth");
        snap.windows.push(
            QuotaWindow::new("3p-weekly", Some(10080), 25.0)
                .kind(WindowKind::Weekly)
                .scope("Claude & GPT"),
        );
        // Argon contains "Gemini" in "Gemini 4 Argon" but must NOT count as Gemini Models group
        snap.windows.push(
            QuotaWindow::new("gemini-argon-wtu-weekly", Some(10080), 100.0)
                .kind(WindowKind::Weekly)
                .scope("Gemini 4 Argon"),
        );
        assert!(!has_gemini_window(&snap));

        snap.windows.push(
            QuotaWindow::new("gemini-5h", Some(300), 6.02)
                .kind(WindowKind::Session)
                .scope("Gemini 4 Neon · Hydrogen · Orchestrator"),
        );
        assert!(has_gemini_window(&snap));
    }

    #[test]
    fn format_dynamic_scope_parses_live_descriptions_and_future_models() {
        assert_eq!(
            format_dynamic_scope(
                "Gemini Models",
                Some("Models within this group: Gemini 4 Neon, Gemini 4 Hydrogen, Orchestrator (Neon), Gemini 3.8 Flash")
            ),
            "Gemini 4 Neon · Hydrogen · Orchestrator · 3.8 Flash"
        );

        assert_eq!(
            format_dynamic_scope(
                "Claude and GPT models",
                Some("Models within this group: Claude Opus 5.5, Claude Sonnet 5.5, GPT-OSS 120B")
            ),
            "Claude Opus 5.5 · Sonnet 5.5 · GPT-OSS 120B"
        );

        assert_eq!(
            format_dynamic_scope(
                "Gemini Models",
                Some("models within this group: Gemini 5 Ultra, Gemini 5 Pro")
            ),
            "Gemini 5 Ultra · Pro"
        );

        assert_eq!(format_dynamic_scope("Gemini 4 Argon", None), "Gemini 4 Argon");
        assert_eq!(
            format_dynamic_scope("Gemini 4 Argon", Some("Quota bucket for Argon")),
            "Gemini 4 Argon"
        );
    }

    #[test]
    fn finalize_keeps_real_windows_without_inventing_siblings() {
        let mut snap = Snapshot::new(ProviderId::Agy, "antigravity_ls");
        let email = "togxida@gmail.com";

        let mut g_5h = QuotaWindow::new("togxida_gemini-5h", Some(300), 10.0)
            .kind(WindowKind::Session)
            .scope("Gemini 4 Neon · Hydrogen · Orchestrator (togxida)");
        g_5h.account = Some(email.to_string());
        let mut g_wk = QuotaWindow::new("togxida_gemini-weekly", Some(10080), 20.0)
            .kind(WindowKind::Weekly)
            .scope("Gemini 4 Neon · Hydrogen · Orchestrator (togxida)");
        g_wk.account = Some(email.to_string());
        let mut argon = QuotaWindow::new("togxida_gemini-argon-wtu-weekly", Some(10080), 0.0)
            .kind(WindowKind::Weekly)
            .scope("Gemini 4 Argon (togxida)");
        argon.account = Some(email.to_string());
        let mut c_5h = QuotaWindow::new("togxida_3p-5h", Some(300), 5.0)
            .kind(WindowKind::Session)
            .scope("Claude Opus 5.5 · Sonnet 5.5 · GPT-OSS 120B (togxida)");
        c_5h.account = Some(email.to_string());
        let mut c_wk = QuotaWindow::new("togxida_3p-weekly", Some(10080), 15.0)
            .kind(WindowKind::Weekly)
            .scope("Claude Opus 5.5 · Sonnet 5.5 · GPT-OSS 120B (togxida)");
        c_wk.account = Some(email.to_string());

        snap.windows.extend([g_5h, g_wk, argon, c_5h, c_wk]);
        let snap = finalize_combined_snapshot(snap, &[email.to_string()], true, false, None, Some(email)).expect("ok");

        assert_eq!(snap.windows.len(), 5, "real windows stay, missing siblings are not invented");
    }

    #[test]
    fn read_google_accounts_active_reads_fallback_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let gemini_dir = tmp.path().join(".gemini");
        std::fs::create_dir_all(&gemini_dir).unwrap();
        std::fs::write(
            gemini_dir.join("google_accounts.json"),
            r#"{"active": "robin421@gmail.com", "old": []}"#,
        )
        .unwrap();

        assert_eq!(
            read_google_accounts_active(tmp.path()).as_deref(),
            Some("robin421@gmail.com")
        );
    }

    #[test]
    fn persist_cached_account_survives_keychain_switch() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = Ctx::with(
            tmp.path().to_path_buf(),
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            std::ffi::OsString::new(),
        );

        let first_token = StoredToken {
            access_token: Some("access-1".into()),
            refresh_token: Some("refresh-1".into()),
            expiry: Some("2026-11-19T10:00:00Z".into()),
            expires_in: Some(3600),
        };
        let second_token = StoredToken {
            access_token: Some("access-2".into()),
            refresh_token: Some("refresh-2".into()),
            expiry: Some("2026-11-19T12:00:00Z".into()),
            expires_in: Some(3600),
        };

        // 1. Keychain has robinsonwen421@gmail.com -> auto-persisted to cache
        let initial = load_all_accounts_with_keychain(
            &ctx,
            Some(("robinsonwen421@gmail.com".into(), first_token)),
        );
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].0, "robinsonwen421@gmail.com");

        let cached_path = cached_accounts_dir(&ctx).join("robinsonwen421@gmail.com.json");
        assert!(cached_path.exists(), "cached account file should be created");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&cached_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        // 2. Keychain switches to robin421@gmail.com -> previous account still loaded from cache
        let after_switch = load_all_accounts_with_keychain(
            &ctx,
            Some(("robin421@gmail.com".into(), second_token)),
        );
        assert_eq!(after_switch.len(), 2);
        assert_eq!(after_switch[0].0, "robin421@gmail.com");
        assert_eq!(after_switch[1].0, "robinsonwen421@gmail.com");
        assert_eq!(after_switch[1].1.access_token.as_deref(), Some("access-1"));
    }

    #[test]
    fn persist_cached_account_respects_should_replace_token_and_sanitizes_filename() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = Ctx::with(
            tmp.path().to_path_buf(),
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            std::ffi::OsString::new(),
        );

        let newer = StoredToken {
            access_token: Some("new-access".into()),
            refresh_token: Some("new-refresh".into()),
            expiry: Some("2026-11-19T12:00:00Z".into()),
            expires_in: Some(3600),
        };
        let older = StoredToken {
            access_token: Some("old-access".into()),
            refresh_token: Some("old-refresh".into()),
            expiry: Some("2026-11-19T08:00:00Z".into()),
            expires_in: Some(3600),
        };

        persist_cached_account(&ctx, "robinsonwen421@gmail.com", &newer);
        persist_cached_account(&ctx, "robinsonwen421@gmail.com", &older);

        let cached_path = cached_accounts_dir(&ctx).join("robinsonwen421@gmail.com.json");
        let (email, stored) = read_account_file(&cached_path).expect("cached file readable");
        assert_eq!(email, "robinsonwen421@gmail.com");
        assert_eq!(stored.access_token.as_deref(), Some("new-access"));

        // Corrupted file should be overwritten cleanly
        std::fs::write(&cached_path, "{not-valid-json").unwrap();
        persist_cached_account(&ctx, "robinsonwen421@gmail.com", &older);
        let (_, repaired) = read_account_file(&cached_path).expect("repaired file readable");
        assert_eq!(repaired.access_token.as_deref(), Some("old-access"));

        // Filename sanitization prevents path traversal
        assert_eq!(
            sanitize_account_filename("../evil/user@example.com"),
            ".._evil_user@example.com"
        );
        assert_eq!(sanitize_account_filename(".."), "account");
        assert_eq!(sanitize_account_filename("."), "account");
        assert_eq!(sanitize_account_filename("   "), "account");
    }
}
