//! Gemini CLI daily per-model quotas via the Code Assist API.
//!
//! Reads `~/.gemini/oauth_creds.json`. An expired access token is refreshed in memory using the
//! OAuth client id shipped inside the installed Gemini CLI (Google refresh tokens do not rotate,
//! so the CLI's stored credentials stay valid and are never rewritten).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::{json, Value};

use crate::ctx::{read_json, send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{ProviderId, QuotaWindow, Snapshot};
use crate::util::{jwt_claims, num, num_at, str_at, time_at};

const BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let dir = ctx.home.join(".gemini");
    let creds = match read_json(&dir.join("oauth_creds.json")) {
        Ok(c) => c,
        Err(FetchError::NotDetected) if dir.exists() || ctx.find_bin("gemini").is_some() => {
            return Err(match auth_type(&dir).as_deref() {
                Some(t) if t != "oauth-personal" => FetchError::Unsupported("api_key_mode".into()),
                _ => FetchError::NotLoggedIn,
            })
        }
        Err(e) => return Err(e),
    };
    if auth_type(&dir).is_some_and(|t| t != "oauth-personal") {
        return Err(FetchError::Unsupported("api_key_mode".into()));
    }

    let mut token = str_at(&creds, &["access_token"]).map(str::to_string);
    let expired = num_at(&creds, &["expiry_date"]).is_some_and(|ms| (ms as i64) < Utc::now().timestamp_millis() + 60_000);
    if expired || token.is_none() {
        let refresh = str_at(&creds, &["refresh_token"]).ok_or(FetchError::AuthExpired)?;
        token = Some(refresh_token(ctx, refresh).await?);
    }
    let token = token.unwrap();

    let assist = send_json(
        ctx.post(&format!("{BASE}:loadCodeAssist"))
            .bearer_auth(&token)
            .json(&json!({"metadata": {"ideType": "GEMINI_CLI", "pluginType": "GEMINI"}})),
    )
    .await
    .map_err(map_auth)?;
    let project = match &assist["cloudaicompanionProject"] {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o.get("id").and_then(Value::as_str).map(str::to_string),
        _ => None,
    };
    let body = match &project {
        Some(p) => json!({"project": p}),
        None => json!({}),
    };
    let quota = send_json(ctx.post(&format!("{BASE}:retrieveUserQuota")).bearer_auth(&token).json(&body))
        .await
        .map_err(map_auth)?;

    let mut snap = parse_quota(&quota)?;
    snap.plan = plan(&assist);
    snap.account = str_at(&creds, &["id_token"])
        .and_then(jwt_claims)
        .and_then(|c| c.get("email").and_then(Value::as_str).map(str::to_string));
    Ok(snap)
}

fn map_auth(e: FetchError) -> FetchError {
    match e {
        FetchError::Http { status: 401, .. } => FetchError::AuthExpired,
        other => other,
    }
}

fn auth_type(dir: &Path) -> Option<String> {
    let s = read_json(&dir.join("settings.json")).ok()?;
    s.pointer("/security/auth/selectedType")
        .or_else(|| s.get("selectedAuthType"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn plan(assist: &Value) -> Option<String> {
    if let Some(name) = assist.pointer("/paidTier/name").and_then(Value::as_str) {
        return Some(name.to_string());
    }
    match assist.pointer("/currentTier/id").and_then(Value::as_str)? {
        "free-tier" => Some("Free".into()),
        "standard-tier" => Some("Standard".into()),
        "legacy-tier" => Some("Legacy".into()),
        other => assist.pointer("/currentTier/name").and_then(Value::as_str).map(str::to_string).or(Some(other.into())),
    }
}

pub fn parse_quota(body: &Value) -> Result<Snapshot> {
    let buckets = body["buckets"].as_array().filter(|b| !b.is_empty()).ok_or_else(|| FetchError::parse("no quota buckets"))?;
    // Several buckets can exist per model (token types); keep the tightest one.
    let mut per_model: BTreeMap<String, (f64, Option<chrono::DateTime<Utc>>)> = BTreeMap::new();
    for b in buckets {
        let (Some(model), Some(frac)) = (str_at(b, &["modelId"]), b.get("remainingFraction").and_then(num)) else {
            continue;
        };
        let reset = time_at(b, &["resetTime"]);
        let e = per_model.entry(model.to_string()).or_insert((frac, reset));
        if frac < e.0 {
            *e = (frac, reset);
        }
    }
    let mut snap = Snapshot::new(ProviderId::Gemini, "oauth");
    for (model, (frac, reset)) in per_model {
        snap.windows.push(
            QuotaWindow::new(format!("model:{model}"), Some(1440), (1.0 - frac) * 100.0)
                .scope(model)
                .resets_at(reset),
        );
    }
    // Pro models first, then Flash, then Flash Lite.
    let rank = |m: &str| if m.contains("flash-lite") { 2 } else if m.contains("flash") { 1 } else { 0 };
    snap.windows.sort_by_key(|w| (rank(w.scope.as_deref().unwrap_or("")), w.scope.clone()));
    Ok(snap)
}

async fn refresh_token(ctx: &Ctx, refresh: &str) -> Result<String> {
    let (id, secret) = oauth_client(ctx).ok_or(FetchError::AuthExpired)?;
    let resp = send_json(ctx.http.post(TOKEN_URL).form(&[
        ("client_id", id.as_str()),
        ("client_secret", secret.as_str()),
        ("refresh_token", refresh),
        ("grant_type", "refresh_token"),
    ]))
    .await
    .map_err(|_| FetchError::AuthExpired)?;
    str_at(&resp, &["access_token"]).map(str::to_string).ok_or(FetchError::AuthExpired)
}

/// Gemini CLI's public OAuth client, read from the user's install (or env override).
fn oauth_client(ctx: &Ctx) -> Option<(String, String)> {
    if let (Some(id), Some(secret)) = (ctx.env("GEMINI_OAUTH_CLIENT_ID"), ctx.env("GEMINI_OAUTH_CLIENT_SECRET")) {
        return Some((id.into(), secret.into()));
    }
    let root = package_root(&ctx.find_bin("gemini")?)?;
    let candidates = [
        root.join("node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js"),
        root.join("../gemini-cli-core/dist/src/code_assist/oauth2.js"),
    ];
    for c in candidates {
        if let Some(found) = std::fs::read_to_string(&c).ok().and_then(|t| extract_client(&t)) {
            return Some(found);
        }
    }
    let bundle = std::fs::read_dir(root.join("bundle")).ok()?;
    bundle
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "js"))
        .find_map(|p| std::fs::read_to_string(p).ok().and_then(|t| extract_client(&t)))
}

fn package_root(bin: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(bin).unwrap_or_else(|_| bin.to_path_buf());
    for dir in real.ancestors().chain(bin.ancestors()) {
        for cand in [
            dir.to_path_buf(),
            dir.join("node_modules/@google/gemini-cli"),
            dir.join("lib/node_modules/@google/gemini-cli"),
            dir.join("libexec/lib/node_modules/@google/gemini-cli"),
        ] {
            let is_pkg = read_json(&cand.join("package.json"))
                .ok()
                .is_some_and(|p| p["name"] == "@google/gemini-cli");
            if is_pkg {
                return Some(cand);
            }
        }
    }
    None
}

pub fn extract_client(source: &str) -> Option<(String, String)> {
    let id = regex::Regex::new(r#"OAUTH_CLIENT_ID\s*=\s*['"]([\w\-.]+)['"]"#).ok()?;
    let secret = regex::Regex::new(r#"OAUTH_CLIENT_SECRET\s*=\s*['"]([\w\-]+)['"]"#).ok()?;
    Some((id.captures(source)?[1].to_string(), secret.captures(source)?[1].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_buckets_keeping_tightest_per_model() {
        let body = json!({"buckets": [
            {"modelId": "gemini-2.5-flash", "remainingFraction": 0.9, "resetTime": "2025-11-09T00:00:00Z", "tokenType": "REQUESTS"},
            {"modelId": "gemini-2.5-pro", "remainingFraction": 0.55, "resetTime": "2025-11-09T00:00:00Z", "tokenType": "REQUESTS"},
            {"modelId": "gemini-2.5-pro", "remainingFraction": 0.30, "resetTime": "2025-11-09T00:00:00Z", "tokenType": "TOKENS"},
            {"modelId": "gemini-2.5-flash-lite", "remainingFraction": 1.0}
        ]});
        let s = parse_quota(&body).unwrap();
        let scopes: Vec<_> = s.windows.iter().map(|w| w.scope.clone().unwrap()).collect();
        assert_eq!(scopes, ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite"]);
        assert!((s.windows[0].remaining_percent() - 30.0).abs() < 1e-9);
    }

    #[test]
    fn extracts_client_from_source() {
        let src = "const OAUTH_CLIENT_ID = '123-abc.apps.googleusercontent.com';\nconst OAUTH_CLIENT_SECRET = 'GOCSPX-xyz';";
        let (id, secret) = extract_client(src).unwrap();
        assert_eq!(id, "123-abc.apps.googleusercontent.com");
        assert_eq!(secret, "GOCSPX-xyz");
    }

    #[test]
    fn plan_prefers_paid_tier() {
        assert_eq!(plan(&json!({"paidTier": {"name": "Google AI Pro"}, "currentTier": {"id": "free-tier"}})).as_deref(), Some("Google AI Pro"));
        assert_eq!(plan(&json!({"currentTier": {"id": "free-tier"}})).as_deref(), Some("Free"));
    }
}
