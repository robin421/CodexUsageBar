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
const BILLING_ENDPOINT: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const GROK_CLIENT_VERSION: &str = "1.0.89";

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

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct BillingCreditsResp {
    config: Option<BillingConfig>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct BillingConfig {
    current_period: Option<BillingPeriod>,
    credit_usage_percent: Option<f64>,
    billing_period_end: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct BillingPeriod {
    #[serde(rename = "type")]
    period_type: Option<String>,
    end: Option<String>,
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

    let (settings_res, billing_opt) = tokio::join!(
        ctx.http
            .get(SETTINGS_ENDPOINT)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", "xai-grok/1.0")
            .send(),
        fetch_billing(ctx, &access_token),
    );

    let resp = settings_res?;

    if resp.status().as_u16() == 401 {
        // Attempt a fresh refresh if possible
        if let Some(refresh_token) = &entry.refresh_token {
            let issuer = entry.oidc_issuer.as_deref().unwrap_or(DEFAULT_ISSUER);
            let client_id = entry.oidc_client_id.as_deref().unwrap_or(DEFAULT_CLIENT_ID);
            if let Ok(new_token) = refresh_grok_token(ctx, issuer, client_id, refresh_token).await {
                let (retry_res, retry_billing) = tokio::join!(
                    ctx.http
                        .get(SETTINGS_ENDPOINT)
                        .header("Authorization", format!("Bearer {new_token}"))
                        .header("User-Agent", "xai-grok/1.0")
                        .send(),
                    fetch_billing(ctx, &new_token),
                );
                let retry = retry_res?;

                if retry.status().is_success() {
                    let settings: GrokSettings = retry.json().await?;
                    return build_snapshot(settings, retry_billing, entry.email);
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
    build_snapshot(settings, billing_opt, entry.email)
}

async fn fetch_billing(ctx: &Ctx, token: &str) -> Option<BillingCreditsResp> {
    let resp = ctx
        .http
        .get(BILLING_ENDPOINT)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "xai-grok/1.0")
        .header("x-grok-client-version", GROK_CLIENT_VERSION)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    resp.json::<BillingCreditsResp>().await.ok()
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

fn parse_iso(s: &str) -> Option<chrono::DateTime<Utc>> {
    parse_time(&serde_json::Value::String(s.to_owned()))
}

fn build_snapshot(
    settings: GrokSettings,
    billing: Option<BillingCreditsResp>,
    email: Option<String>,
) -> Result<Snapshot> {
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

    let window = match billing
        .as_ref()
        .and_then(|b| b.config.as_ref())
        .and_then(|cfg| cfg.credit_usage_percent.map(|pct| (cfg, pct)))
    {
        Some((cfg, usage_pct)) => {
            let used_percent = usage_pct.clamp(0.0, 100.0);
            let period = cfg.current_period.as_ref();
            let (window_mins, kind) = match period.and_then(|p| p.period_type.as_deref()) {
                Some("USAGE_PERIOD_TYPE_WEEKLY") => (Some(10080), WindowKind::Weekly),
                Some("USAGE_PERIOD_TYPE_MONTHLY") => (Some(43200), WindowKind::Monthly),
                Some("USAGE_PERIOD_TYPE_DAILY") => (Some(1440), WindowKind::Daily),
                _ => (None, WindowKind::Monthly),
            };
            let resets_at = period
                .and_then(|p| p.end.as_deref())
                .and_then(parse_iso)
                .or_else(|| cfg.billing_period_end.as_deref().and_then(parse_iso));

            QuotaWindow::new("access", window_mins, used_percent)
                .scope(model)
                .kind(kind)
                .resets_at(resets_at)
        }
        None => QuotaWindow::new("access", None, 0.0)
            .scope(model)
            .kind(WindowKind::Monthly),
    };

    snap.windows.push(window);

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
    fn builds_snapshot_from_settings_fallback() {
        let settings = GrokSettings {
            subscription_tier_display: Some("SuperGrok".into()),
            subscription_tier: None,
            allow_access: Some(true),
            default_model: Some("grok-4.7".into()),
        };

        let s = build_snapshot(settings, None, Some("user@example.com".into())).expect("built");
        assert_eq!(s.provider, ProviderId::Grok);
        assert_eq!(s.plan.as_deref(), Some("SuperGrok"));
        assert_eq!(s.account.as_deref(), Some("user@example.com"));
        assert_eq!(s.windows.len(), 1);
        assert_eq!(s.windows[0].id, "access");
        assert_eq!(s.windows[0].scope.as_deref(), Some("grok-4.7"));
        assert_eq!(s.windows[0].used_percent, 0.0);
        assert_eq!(s.windows[0].window_mins, None);
        assert_eq!(s.windows[0].kind, WindowKind::Monthly);
        assert_eq!(s.windows[0].resets_at, None);
    }

    #[test]
    fn parses_billing_credits_and_builds_weekly_snapshot() {
        let billing_json = r#"{
            "config": {
                "currentPeriod": {
                    "type": "USAGE_PERIOD_TYPE_WEEKLY",
                    "start": "2026-11-15T04:08:48.831412+00:00",
                    "end": "2026-11-22T04:08:48.831412+00:00"
                },
                "creditUsagePercent": 3.0,
                "onDemandCap": {"val": 0},
                "onDemandUsed": {"val": 0},
                "productUsage": [
                    {"product": "GrokBuild", "usagePercent": 2.0},
                    {"product": "GrokChat", "usagePercent": 1.0}
                ],
                "isUnifiedBillingUser": true,
                "prepaidBalance": {"val": 0},
                "billingPeriodStart": "2026-11-15T04:08:48.831412+00:00",
                "billingPeriodEnd": "2026-11-22T04:08:48.831412+00:00"
            }
        }"#;

        let billing: BillingCreditsResp =
            serde_json::from_str(billing_json).expect("valid billing json");
        let settings = GrokSettings {
            subscription_tier_display: Some("SuperGrok".into()),
            subscription_tier: None,
            allow_access: Some(true),
            default_model: Some("grok-4.7".into()),
        };

        let s = build_snapshot(settings, Some(billing), Some("user@example.com".into()))
            .expect("built");
        assert_eq!(s.windows.len(), 1);
        let w = &s.windows[0];
        assert_eq!(w.id, "access");
        assert_eq!(w.scope.as_deref(), Some("grok-4.7"));
        assert!((w.used_percent - 3.0).abs() < f64::EPSILON);
        assert_eq!(w.window_mins, Some(10080));
        assert_eq!(w.kind, WindowKind::Weekly);
        assert_eq!(
            w.resets_at,
            parse_iso("2026-11-22T04:08:48.831412+00:00")
        );
    }

    #[test]
    fn builds_snapshot_billing_period_variants_and_fallbacks() {
        let monthly: BillingCreditsResp = serde_json::from_str(
            r#"{
                "config": {
                    "currentPeriod": { "type": "USAGE_PERIOD_TYPE_MONTHLY" },
                    "creditUsagePercent": 142.5,
                    "billingPeriodEnd": "2026-12-01T00:00:00Z"
                }
            }"#,
        )
        .expect("monthly json");
        let settings_m = GrokSettings {
            subscription_tier_display: None,
            subscription_tier: Some("GrokPro".into()),
            allow_access: Some(true),
            default_model: None,
        };
        let sm = build_snapshot(settings_m, Some(monthly), None).expect("built monthly");
        assert_eq!(sm.windows[0].window_mins, Some(43200));
        assert_eq!(sm.windows[0].kind, WindowKind::Monthly);
        assert_eq!(sm.windows[0].used_percent, 100.0);
        assert_eq!(sm.windows[0].resets_at, parse_iso("2026-12-01T00:00:00Z"));

        let daily: BillingCreditsResp = serde_json::from_str(
            r#"{
                "config": {
                    "currentPeriod": {
                        "type": "USAGE_PERIOD_TYPE_DAILY",
                        "end": "2026-11-20T00:00:00Z"
                    },
                    "creditUsagePercent": -5.0
                }
            }"#,
        )
        .expect("daily json");
        let settings_d = GrokSettings {
            subscription_tier_display: None,
            subscription_tier: None,
            allow_access: Some(true),
            default_model: Some("grok-4.7".into()),
        };
        let sd = build_snapshot(settings_d, Some(daily), None).expect("built daily");
        assert_eq!(sd.windows[0].window_mins, Some(1440));
        assert_eq!(sd.windows[0].kind, WindowKind::Daily);
        assert_eq!(sd.windows[0].used_percent, 0.0);
        assert_eq!(sd.windows[0].resets_at, parse_iso("2026-11-20T00:00:00Z"));
    }

    #[test]
    fn rejects_disallowed_access() {
        let settings = GrokSettings {
            subscription_tier_display: Some("Free".into()),
            subscription_tier: None,
            allow_access: Some(false),
            default_model: None,
        };

        let err = build_snapshot(settings, None, None).unwrap_err();
        match err {
            FetchError::Unsupported(msg) => assert!(msg.contains("no access")),
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }
}
