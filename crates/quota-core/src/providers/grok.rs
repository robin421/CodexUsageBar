//! Grok CLI (`grok`) status and tier fetching.
//!
//! Grok stores credentials in `~/.grok/auth.json` (keyed by `{issuer}::{client_id}` or raw map).
//! Expired tokens are refreshed via `https://auth.x.ai/oauth2/token`.
//! Settings and subscription tier are fetched from `https://cli-chat-proxy.grok.com/v1/settings`.

use std::collections::HashMap;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::ctx::Ctx;
use crate::error::{FetchError, Result};
use crate::model::{ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::parse_time;

const DEFAULT_ISSUER: &str = "https://auth.x.ai";
const DEFAULT_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const SETTINGS_ENDPOINT: &str = "https://cli-chat-proxy.grok.com/v1/settings";

#[derive(Deserialize, Debug, Clone)]
struct GrokAuthEntry {
    key: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<String>,
    email: Option<String>,
    oidc_issuer: Option<String>,
    oidc_client_id: Option<String>,
}

#[derive(Serialize)]
struct GrokRefreshReq<'a> {
    grant_type: &'a str,
    client_id: &'a str,
    refresh_token: &'a str,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct GrokRefreshResp {
    access_token: String,
    expires_in: Option<i64>,
}

#[derive(Deserialize, Debug)]
struct GrokSettings {
    subscription_tier_display: Option<String>,
    subscription_tier: Option<String>,
    allow_access: Option<bool>,
    default_model: Option<String>,
}

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let auth_file = ctx.home.join(".grok/auth.json");
    if !auth_file.exists() {
        let grok_dir = ctx.home.join(".grok");
        if grok_dir.exists() || ctx.find_bin("grok").is_some() {
            return Err(FetchError::NotLoggedIn);
        }
        return Err(FetchError::NotDetected);
    }

    let raw = std::fs::read_to_string(&auth_file)
        .map_err(|e| FetchError::parse(format!("cannot read ~/.grok/auth.json: {e}")))?;

    let entry = parse_auth_entry(&raw)
        .ok_or_else(|| FetchError::NotLoggedIn)?;

    let access_token = ensure_valid_token(ctx, &entry).await?;

    let resp = ctx
        .http
        .get(SETTINGS_ENDPOINT)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", "xai-grok/1.0")
        .send()
        .await?;

    if resp.status().as_u16() == 401 {
        // Attempt a fresh refresh if possible
        if let Some(refresh_token) = &entry.refresh_token {
            let issuer = entry.oidc_issuer.as_deref().unwrap_or(DEFAULT_ISSUER);
            let client_id = entry.oidc_client_id.as_deref().unwrap_or(DEFAULT_CLIENT_ID);
            if let Ok(new_token) = refresh_grok_token(ctx, issuer, client_id, refresh_token).await {
                let retry = ctx
                    .http
                    .get(SETTINGS_ENDPOINT)
                    .header("Authorization", format!("Bearer {new_token}"))
                    .header("User-Agent", "xai-grok/1.0")
                    .send()
                    .await?;

                if retry.status().is_success() {
                    let settings: GrokSettings = retry.json().await?;
                    return build_snapshot(settings, entry.email);
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

    let settings: GrokSettings = resp.json().await?;
    build_snapshot(settings, entry.email)
}

fn parse_auth_entry(raw: &str) -> Option<GrokAuthEntry> {
    // 1. Try keyed map `{"https://auth.x.ai::...": { ... }}`
    if let Ok(map) = serde_json::from_str::<HashMap<String, GrokAuthEntry>>(raw) {
        if let Some((_, entry)) = map.into_iter().find(|(k, e)| k.contains("auth.x.ai") || e.key.is_some()) {
            return Some(entry);
        }
    }

    // 2. Try single flat object
    if let Ok(entry) = serde_json::from_str::<GrokAuthEntry>(raw) {
        if entry.key.is_some() || entry.refresh_token.is_some() {
            return Some(entry);
        }
    }

    None
}

async fn ensure_valid_token(ctx: &Ctx, entry: &GrokAuthEntry) -> Result<String> {
    let is_expired = match &entry.expires_at {
        Some(exp) => parse_time(&serde_json::Value::String(exp.clone())).is_some_and(|t| t <= Utc::now() + Duration::seconds(60)),
        None => false,
    };

    if !is_expired {
        if let Some(key) = &entry.key {
            if !key.is_empty() {
                return Ok(key.clone());
            }
        }
    }

    if let Some(refresh) = &entry.refresh_token {
        let issuer = entry.oidc_issuer.as_deref().unwrap_or(DEFAULT_ISSUER);
        let client_id = entry.oidc_client_id.as_deref().unwrap_or(DEFAULT_CLIENT_ID);
        return refresh_grok_token(ctx, issuer, client_id, refresh).await;
    }

    entry
        .key
        .clone()
        .ok_or_else(|| FetchError::parse("missing grok session token"))
}

async fn refresh_grok_token(ctx: &Ctx, issuer: &str, client_id: &str, refresh_token: &str) -> Result<String> {
    let url = format!("{issuer}/oauth2/token");
    let req = GrokRefreshReq {
        grant_type: "refresh_token",
        client_id,
        refresh_token,
    };

    let resp = ctx
        .http
        .post(&url)
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

    let data: GrokRefreshResp = resp.json().await?;
    Ok(data.access_token)
}

fn build_snapshot(settings: GrokSettings, email: Option<String>) -> Result<Snapshot> {
    let tier = settings
        .subscription_tier_display
        .or(settings.subscription_tier)
        .unwrap_or_else(|| "SuperGrok".into());

    if settings.allow_access == Some(false) {
        return Err(FetchError::Unsupported(format!("no access for tier {tier}")));
    }

    let mut snap = Snapshot::new(ProviderId::Grok, "oauth");
    snap.account = email;
    snap.plan = Some(tier);

    let model = settings.default_model.unwrap_or_else(|| "grok-4.7".into());

    // SuperGrok provides active model access window
    snap.windows.push(
        QuotaWindow::new("access", None, 0.0)
            .scope(model)
            .kind(WindowKind::Monthly),
    );

    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_auth_json_map() {
        let json_str = r#"{
            "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828": {
                "key": "test-key-123",
                "refresh_token": "test-refresh-456",
                "email": "user@example.com",
                "expires_at": "2026-11-08T13:06:42Z",
                "oidc_issuer": "https://auth.x.ai",
                "oidc_client_id": "b1a00492-073a-47ea-816f-4c329264a828"
            }
        }"#;

        let entry = parse_auth_entry(json_str).expect("parsed");
        assert_eq!(entry.email.as_deref(), Some("user@example.com"));
        assert_eq!(entry.key.as_deref(), Some("test-key-123"));
    }

    #[test]
    fn builds_snapshot_from_settings() {
        let settings = GrokSettings {
            subscription_tier_display: Some("SuperGrok".into()),
            subscription_tier: None,
            allow_access: Some(true),
            default_model: Some("grok-4.7".into()),
        };

        let s = build_snapshot(settings, Some("user@example.com".into())).expect("built");
        assert_eq!(s.provider, ProviderId::Grok);
        assert_eq!(s.plan.as_deref(), Some("SuperGrok"));
        assert_eq!(s.account.as_deref(), Some("user@example.com"));
        assert_eq!(s.windows.len(), 1);
        assert_eq!(s.windows[0].scope.as_deref(), Some("grok-4.7"));
    }

    #[test]
    fn rejects_disallowed_access() {
        let settings = GrokSettings {
            subscription_tier_display: Some("Free".into()),
            subscription_tier: None,
            allow_access: Some(false),
            default_model: None,
        };

        let err = build_snapshot(settings, None).unwrap_err();
        match err {
            FetchError::Unsupported(msg) => assert!(msg.contains("no access")),
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }
}
