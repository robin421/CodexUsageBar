//! Codex (ChatGPT plan) quota.
//!
//! 1. OAuth: `~/.codex/auth.json` access token -> `GET chatgpt.com/backend-api/wham/usage`.
//! 2. Fallback: the official `codex app-server` JSON-RPC (`account/read` + `account/rateLimits/read`),
//!    which lets the CLI refresh its own credentials.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::ctx::{read_json, send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{Balance, ProviderId, QuotaWindow, Snapshot};
use crate::util::{jwt_claims, num, num_at, parse_time, str_at, title_case};

const DEFAULT_BASE: &str = "https://chatgpt.com/backend-api";

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let home = codex_home(ctx);
    let auth = read_json(&home.join("auth.json")).ok();
    let cli = ctx.find_bin("codex");
    if auth.is_none() && cli.is_none() {
        return Err(FetchError::NotDetected);
    }

    let oauth_err = match &auth {
        Some(a) if a.get("tokens").is_some() => match fetch_oauth(ctx, a, &home).await {
            Ok(s) => return Ok(s),
            Err(e) => e,
        },
        Some(a) if str_at(a, &["OPENAI_API_KEY"]).is_some() => {
            return Err(FetchError::Unsupported("api_key_mode".into()))
        }
        _ => FetchError::NotLoggedIn,
    };
    // Network/server failures will not be fixed by the CLI path either.
    if !oauth_err.is_auth() && !matches!(oauth_err, FetchError::Parse(_)) {
        return Err(oauth_err);
    }
    match cli {
        Some(bin) => app_server(ctx, bin).await,
        None => Err(oauth_err),
    }
}

fn codex_home(ctx: &Ctx) -> PathBuf {
    ctx.env("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| ctx.home.join(".codex"))
}

fn base_url(home: &std::path::Path) -> String {
    let Ok(text) = std::fs::read_to_string(home.join("config.toml")) else {
        return DEFAULT_BASE.into();
    };
    let re = regex::Regex::new(r#"(?m)^\s*chatgpt_base_url\s*=\s*"([^"]+)""#).unwrap();
    re.captures(&text)
        .map(|c| c[1].trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_BASE.into())
}

async fn fetch_oauth(ctx: &Ctx, auth: &Value, home: &std::path::Path) -> Result<Snapshot> {
    let tokens = &auth["tokens"];
    let token = str_at(tokens, &["access_token", "accessToken"]).ok_or(FetchError::NotLoggedIn)?;
    let account_id = str_at(tokens, &["account_id", "accountId"]);

    let base = base_url(home);
    let url = if base.contains("/backend-api") {
        format!("{base}/wham/usage")
    } else {
        format!("{base}/api/codex/usage")
    };
    let mut req = ctx.get(&url).bearer_auth(token);
    if let Some(id) = account_id {
        req = req.header("ChatGPT-Account-Id", id);
    }
    let body = send_json(req).await.map_err(|e| match e {
        FetchError::Http { status: 401, .. } => FetchError::AuthExpired,
        other => other,
    })?;
    let mut snap = parse_usage(&body)?;
    snap.account = str_at(tokens, &["id_token", "idToken"])
        .and_then(jwt_claims)
        .and_then(|c| c.get("email").and_then(Value::as_str).map(str::to_string));
    Ok(snap)
}

pub fn parse_usage(body: &Value) -> Result<Snapshot> {
    let mut snap = Snapshot::new(ProviderId::Codex, "oauth");
    snap.plan = str_at(body, &["plan_type", "planType"]).map(plan_name);

    let rl = &body["rate_limit"];
    if rl.is_null() && body.get("credits").is_none() {
        return Err(FetchError::parse("missing rate_limit"));
    }
    for (slot, key) in [("primary", "primary_window"), ("secondary", "secondary_window")] {
        if let Some(w) = oauth_window(&rl[key], slot) {
            if !snap.windows.iter().any(|x| x.window_mins == w.window_mins) {
                snap.windows.push(w);
            }
        }
    }
    if let Some(extra) = body["additional_rate_limits"].as_array() {
        for (i, item) in extra.iter().enumerate() {
            let name = str_at(item, &["limit_name", "metered_feature"]).unwrap_or("Extra");
            for key in ["primary_window", "secondary_window"] {
                if let Some(w) = oauth_window(&item["rate_limit"][key], &format!("extra{i}_{key}")) {
                    snap.windows.push(w.scope(title_case(name)));
                }
            }
        }
    }
    snap.windows.sort_by_key(|w| (w.scope.is_some(), w.window_mins.unwrap_or(u32::MAX)));
    snap.balances.extend(credits(&body["credits"], "has_credits"));
    Ok(snap)
}

fn oauth_window(w: &Value, id: &str) -> Option<QuotaWindow> {
    let used = num_at(w, &["used_percent", "usedPercent"])?;
    let mins = num_at(w, &["limit_window_seconds"]).map(|s| (s / 60.0).round() as u32);
    Some(QuotaWindow::new(id, mins, used).resets_at(w.get("reset_at").and_then(parse_time)))
}

fn credits(c: &Value, has_key: &str) -> Option<Balance> {
    let unlimited = c["unlimited"].as_bool().unwrap_or(false);
    let has = c[has_key].as_bool().unwrap_or(false);
    let amount = c.get("balance").and_then(num).unwrap_or(0.0);
    (unlimited || (has && amount > 0.0)).then(|| Balance {
        id: "reset_credits".into(),
        amount: if unlimited { 0.0 } else { amount },
        currency: "credits".into(),
        total: None,
        unlimited,
    })
}

pub fn plan_name(raw: &str) -> String {
    match raw.to_ascii_lowercase().as_str() {
        "plus" => "ChatGPT Plus".into(),
        "pro" => "ChatGPT Pro".into(),
        "prolite" => "ChatGPT Pro Lite".into(),
        "team" => "ChatGPT Team".into(),
        "go" => "ChatGPT Go".into(),
        "free" => "Free".into(),
        other => title_case(other),
    }
}

// ---------------------------------------------------------------------------------------------
// codex app-server (newline-delimited JSON-RPC over stdio)

async fn app_server(ctx: &Ctx, bin: PathBuf) -> Result<Snapshot> {
    let mut child = ctx
        .command(&bin)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| FetchError::Network(format!("failed to start codex app-server: {e}")))?;
    let mut stdin = child.stdin.take().expect("stdin");
    let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();

    let result = tokio::time::timeout(Duration::from_secs(15), async {
        let call = |id: u64, method: &str, params: Value| {
            let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            format!("{msg}\n")
        };
        stdin
            .write_all(call(1, "initialize", json!({"clientInfo": {"name": "CodexUsageBar", "version": env!("CARGO_PKG_VERSION")}})).as_bytes())
            .await
            .ok();
        read_response(&mut lines, 1).await?;
        let initialized = json!({"jsonrpc": "2.0", "method": "initialized", "params": {}});
        stdin.write_all(format!("{initialized}\n").as_bytes()).await.ok();
        stdin.write_all(call(2, "account/read", json!({"refreshToken": false})).as_bytes()).await.ok();
        let account = read_response(&mut lines, 2).await?;
        stdin.write_all(call(3, "account/rateLimits/read", json!({})).as_bytes()).await.ok();
        let limits = read_response(&mut lines, 3).await?;
        Ok::<_, FetchError>((account, limits))
    })
    .await;
    let _ = child.kill().await;

    let (account, limits) = result.map_err(|_| FetchError::Network("codex app-server timed out".into()))??;
    parse_app_server(&account, &limits)
}

async fn read_response<R: tokio::io::AsyncBufRead + Unpin>(
    lines: &mut tokio::io::Lines<R>,
    id: u64,
) -> Result<Value> {
    while let Some(line) = lines.next_line().await.map_err(|e| FetchError::Network(e.to_string()))? {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        if msg["id"].as_u64() != Some(id) {
            continue; // notifications or unrelated responses
        }
        if let Some(err) = msg.get("error") {
            let text = err["message"].as_str().unwrap_or("error").to_string();
            let lower = text.to_lowercase();
            if lower.contains("login") || lower.contains("auth") || lower.contains("unauthorized") {
                return Err(FetchError::AuthExpired);
            }
            return Err(FetchError::Network(format!("codex app-server: {text}")));
        }
        return Ok(msg["result"].clone());
    }
    Err(FetchError::Network("codex app-server closed unexpectedly".into()))
}

pub fn parse_app_server(account: &Value, limits: &Value) -> Result<Snapshot> {
    let acc = &account["account"];
    if acc.is_null() {
        return Err(FetchError::NotLoggedIn);
    }
    if acc["type"].as_str() == Some("apiKey") {
        return Err(FetchError::Unsupported("api_key_mode".into()));
    }
    let snapshot = limits["rateLimitsByLimitId"]
        .get("codex")
        .filter(|v| !v.is_null())
        .unwrap_or(&limits["rateLimits"]);

    let mut snap = Snapshot::new(ProviderId::Codex, "app-server");
    snap.account = str_at(acc, &["email"]).map(str::to_string);
    snap.plan = str_at(acc, &["planType"]).or_else(|| str_at(snapshot, &["planType"])).map(plan_name);
    for slot in ["primary", "secondary"] {
        let w = &snapshot[slot];
        let Some(used) = num_at(w, &["usedPercent"]) else { continue };
        let mins = num_at(w, &["windowDurationMins"]).map(|m| m as u32);
        if snap.windows.iter().any(|x| x.window_mins == mins) {
            continue;
        }
        snap.windows.push(QuotaWindow::new(slot, mins, used).resets_at(w.get("resetsAt").and_then(parse_time)));
    }
    snap.windows.sort_by_key(|w| w.window_mins.unwrap_or(u32::MAX));
    snap.balances.extend(credits(&snapshot["credits"], "hasCredits"));
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WindowKind;

    #[test]
    fn parses_oauth_usage() {
        let body = json!({
            "plan_type": "pro",
            "rate_limit": {
                "primary_window": {"used_percent": 18, "reset_at": 1762596000, "limit_window_seconds": 18000},
                "secondary_window": {"used_percent": 36, "reset_at": 1763000000, "limit_window_seconds": 604800}
            },
            "credits": {"has_credits": true, "unlimited": false, "balance": "3"},
            "additional_rate_limits": [
                {"limit_name": "codex_spark", "rate_limit": {"primary_window": {"used_percent": 90, "reset_at": 1762596000, "limit_window_seconds": 18000}}}
            ]
        });
        let s = parse_usage(&body).unwrap();
        assert_eq!(s.plan.as_deref(), Some("ChatGPT Pro"));
        assert_eq!(s.windows.len(), 3);
        assert_eq!(s.windows[0].kind, WindowKind::Session);
        assert_eq!(s.windows[0].remaining_percent(), 82.0);
        assert_eq!(s.windows[1].kind, WindowKind::Weekly);
        assert_eq!(s.windows[2].scope.as_deref(), Some("Codex Spark"));
        assert_eq!(s.balances[0].amount, 3.0);
        assert_eq!(s.tightest().unwrap().scope.as_deref(), Some("Codex Spark"));
    }

    #[test]
    fn parses_app_server_and_dedupes_windows() {
        let account = json!({"account": {"type": "chatgpt", "email": "a@b.c", "planType": "plus"}});
        let limits = json!({
            "rateLimitsByLimitId": {"codex": {
                "primary": {"usedPercent": 40, "windowDurationMins": 300, "resetsAt": 1762596000},
                "secondary": {"usedPercent": 41, "windowDurationMins": 300, "resetsAt": 1762596000},
                "credits": {"hasCredits": false, "unlimited": false, "balance": "0"}
            }}
        });
        let s = parse_app_server(&account, &limits).unwrap();
        assert_eq!(s.windows.len(), 1);
        assert!(s.balances.is_empty());
        assert_eq!(s.account.as_deref(), Some("a@b.c"));
    }

    #[test]
    fn app_server_api_key_mode() {
        let account = json!({"account": {"type": "apiKey"}});
        assert_eq!(
            parse_app_server(&account, &json!({})).unwrap_err(),
            FetchError::Unsupported("api_key_mode".into())
        );
        assert_eq!(parse_app_server(&json!({"account": null}), &json!({})).unwrap_err(), FetchError::NotLoggedIn);
    }
}
