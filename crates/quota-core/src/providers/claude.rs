//! Claude Code subscription limits via the OAuth usage endpoint.
//!
//! Credentials come from `~/.claude/.credentials.json` (Linux/Windows) or the macOS Keychain item
//! `Claude Code-credentials`. Tokens are only read; when expired the user is asked to run `claude`
//! once, which refreshes them itself (we never rotate Claude Code's refresh token behind its back).

use std::path::PathBuf;

use chrono::Utc;
use serde_json::Value;

use crate::ctx::{read_json, send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{num, num_at, str_at, time_at, title_case};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let creds = load_credentials(ctx).await?;
    let oauth = &creds["claudeAiOauth"];
    let token = str_at(oauth, &["accessToken"]).ok_or(FetchError::NotLoggedIn)?;
    if let Some(exp) = num_at(oauth, &["expiresAt"]) {
        if (exp as i64) < Utc::now().timestamp_millis() {
            return Err(FetchError::AuthExpired);
        }
    }
    let req = ctx
        .get(USAGE_URL)
        .bearer_auth(token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Content-Type", "application/json");
    let body = send_json(req).await.map_err(|e| match e {
        FetchError::Http { status: 401, .. } => FetchError::AuthExpired,
        other => other,
    })?;
    let mut snap = parse_usage(&body)?;
    snap.plan = str_at(oauth, &["subscriptionType"]).map(plan_name);
    Ok(snap)
}

fn config_dir(ctx: &Ctx) -> PathBuf {
    ctx.env("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| ctx.home.join(".claude"))
}

async fn load_credentials(ctx: &Ctx) -> Result<Value> {
    let file = config_dir(ctx).join(".credentials.json");
    match read_json(&file) {
        Ok(v) => return Ok(v),
        Err(FetchError::NotDetected) => {}
        Err(e) => return Err(e),
    }
    if cfg!(target_os = "macos") {
        let out = ctx
            .command("/usr/bin/security")
            .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
            .output()
            .await
            .map_err(|_| FetchError::NotDetected)?;
        if out.status.success() {
            return serde_json::from_slice(&out.stdout).map_err(|e| FetchError::parse(e.to_string()));
        }
    }
    if ctx.find_bin("claude").is_some() || config_dir(ctx).exists() {
        Err(FetchError::NotLoggedIn)
    } else {
        Err(FetchError::NotDetected)
    }
}

pub fn parse_usage(body: &Value) -> Result<Snapshot> {
    let obj = body.as_object().ok_or_else(|| FetchError::parse("usage is not an object"))?;
    let mut snap = Snapshot::new(ProviderId::Claude, "oauth");
    for (key, w) in obj {
        let Some(used) = w.get("utilization").and_then(num) else { continue };
        let resets = time_at(w, &["resets_at"]);
        let window = match key.as_str() {
            "five_hour" => QuotaWindow::new(key, Some(300), used),
            "seven_day" => QuotaWindow::new(key, Some(10080), used),
            "extra_usage" => {
                if w["is_enabled"].as_bool() != Some(true) {
                    continue;
                }
                QuotaWindow::new(key, None, used).kind(WindowKind::Monthly).scope("Extra usage")
            }
            k if k.starts_with("seven_day_") => {
                let scope = match &k["seven_day_".len()..] {
                    "oauth_apps" => "OAuth apps".to_string(),
                    other => title_case(other),
                };
                QuotaWindow::new(key, Some(10080), used).scope(scope)
            }
            _ => continue,
        };
        snap.windows.push(window.resets_at(resets));
    }
    if snap.windows.is_empty() {
        return Err(FetchError::parse("no usage windows"));
    }
    snap.windows.sort_by_key(|w| (w.scope.is_some(), w.window_mins.unwrap_or(u32::MAX), w.id.clone()));
    Ok(snap)
}

fn plan_name(raw: &str) -> String {
    match raw {
        "pro" => "Claude Pro".into(),
        "max" => "Claude Max".into(),
        "team" => "Claude Team".into(),
        "enterprise" => "Claude Enterprise".into(),
        other => title_case(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_usage_windows() {
        let body = json!({
            "five_hour": {"utilization": 23.0, "resets_at": "2025-11-08T15:00:00+00:00"},
            "seven_day": {"utilization": 61.5, "resets_at": "2025-11-12T08:00:00+00:00"},
            "seven_day_opus": {"utilization": 92.0, "resets_at": "2025-11-12T08:00:00+00:00"},
            "seven_day_oauth_apps": null,
            "extra_usage": {"is_enabled": false, "utilization": null}
        });
        let s = parse_usage(&body).unwrap();
        let ids: Vec<_> = s.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["five_hour", "seven_day", "seven_day_opus"]);
        assert_eq!(s.windows[2].scope.as_deref(), Some("Opus"));
        assert_eq!(s.tightest().unwrap().id, "seven_day_opus");
        assert!(s.windows[0].resets_at.is_some());
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_usage(&json!({"five_hour": null})).is_err());
    }
}
