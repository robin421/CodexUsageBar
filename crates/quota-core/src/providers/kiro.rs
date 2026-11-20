//! Kiro (`kiro.dev`) quota provider.
//!
//! Reads credentials from `~/.aws/sso/cache/kiro-auth-token.json` (Kiro IDE) and/or
//! `$XDG_CONFIG_HOME/magpie/plugin-auth.json` (`~/.config/magpie/plugin-auth.json`),
//! refreshes expired OAuth tokens, and queries `Get-Usage-Limits` / `GetUsageLimits`.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::ctx::{send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{num_at, parse_time, pct, str_at, time_at};

const DEFAULT_REGION: &str = "us-east-1";
const CREDIT_REGIONS: &[&str] = &["us-east-1", "eu-central-1"];
const BUILDER_ID_PROFILE: &str = "arn:aws:codewhisperer:us-east-1:638616132270:profile/AAAACCCCXXXX";
const DESKTOP_UA: &str = "Kiro-Desktop/0.2.13 (darwin; arm64)";
const EARLY_REFRESH_SECS: i64 = 120;
const MONTH_MINS: u32 = 30 * 24 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthMethod {
    Social,
    Idc,
    ExternalIdp,
    ApiKey,
}

#[derive(Clone, Debug)]
struct KiroCreds {
    access: String,
    refresh: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    region: String,
    profile: Option<String>,
    method: AuthMethod,
    builder_id: bool,
    client_id: Option<String>,
    client_secret: Option<String>,
    token_url: Option<String>,
    account_hint: Option<String>,
    source: &'static str,
}

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let candidates = load_all_creds(ctx)?;
    let mut last_err = FetchError::NotLoggedIn;

    for mut creds in candidates {
        if creds.method != AuthMethod::ApiKey && needs_refresh(&creds) {
            match try_refresh(ctx, &creds).await {
                Ok(Some(refreshed)) => creds = refreshed,
                Ok(None) => {}
                Err(e) => {
                    last_err = e;
                    continue;
                }
            }
        }

        if creds.profile.is_none() {
            creds.profile = resolve_profile(ctx, &creds).await;
        }

        match call_usage_limits(ctx, &creds).await {
            Ok(body) => return Ok(parse_usage(&body, &creds, Utc::now())),
            Err(FetchError::Http { status: 401 | 403, .. }) if creds.method != AuthMethod::ApiKey => {
                match try_refresh(ctx, &creds).await {
                    Ok(Some(mut refreshed)) => {
                        if refreshed.profile.is_none() {
                            refreshed.profile = resolve_profile(ctx, &refreshed).await;
                        }
                        match call_usage_limits(ctx, &refreshed).await {
                            Ok(body) => return Ok(parse_usage(&body, &refreshed, Utc::now())),
                            Err(e) => last_err = e,
                        }
                    }
                    Ok(None) => last_err = FetchError::NotLoggedIn,
                    Err(e) => last_err = e,
                }
            }
            Err(e) => last_err = e,
        }
    }

    Err(last_err)
}

fn needs_refresh(creds: &KiroCreds) -> bool {
    if creds.access.is_empty() {
        return true;
    }
    match creds.expires_at {
        Some(exp) => exp.timestamp() <= Utc::now().timestamp() + EARLY_REFRESH_SECS,
        None => false,
    }
}

fn valid_aws_region(r: &str) -> bool {
    !r.is_empty()
        && r.len() <= 32
        && r.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && r.contains('-')
}

fn region_of(profile: Option<&str>, signed_in: Option<&str>) -> String {
    if let Some(arn) = profile {
        let parts: Vec<&str> = arn.split(':').collect();
        if parts.len() > 4 && CREDIT_REGIONS.contains(&parts[3]) {
            return parts[3].to_string();
        }
    }
    if let Some(r) = signed_in.filter(|r| valid_aws_region(r)) {
        if r.starts_with("eu-") {
            return "eu-central-1".to_string();
        }
        return r.to_string();
    }
    DEFAULT_REGION.to_string()
}

fn arn_of(s: Option<&str>) -> Option<String> {
    let s = s?.trim();
    if s.starts_with("arn:aws:codewhisperer:") && s.contains(":profile/") {
        Some(s.to_string())
    } else {
        None
    }
}

fn magpie_auth_paths(ctx: &Ctx) -> Vec<PathBuf> {
    let xdg = ctx.xdg_config().join("magpie/plugin-auth.json");
    let dot_config = ctx.home.join(".config/magpie/plugin-auth.json");
    if xdg == dot_config {
        vec![xdg]
    } else {
        vec![xdg, dot_config]
    }
}

fn load_all_creds(ctx: &Ctx) -> Result<Vec<KiroCreds>> {
    let mut saw_file = false;
    let ide_creds = load_ide_creds(ctx, &mut saw_file);
    let magpie_creds = load_magpie_creds(ctx, &mut saw_file);
    let hint = magpie_account_hint(ctx);

    let mut out = Vec::new();
    if let Some(mut ide) = ide_creds {
        if ide.account_hint.is_none() {
            ide.account_hint = hint.clone();
        }
        out.push(ide);
    }
    if let Some(magpie) = magpie_creds {
        let dup = out.iter().any(|c: &KiroCreds| c.access == magpie.access && !c.access.is_empty());
        if !dup {
            out.push(magpie);
        }
    }

    if out.is_empty() {
        if saw_file {
            Err(FetchError::NotLoggedIn)
        } else {
            Err(FetchError::NotDetected)
        }
    } else {
        Ok(out)
    }
}

#[cfg(test)]
fn load_creds(ctx: &Ctx) -> Result<KiroCreds> {
    load_all_creds(ctx).map(|mut v| v.remove(0))
}

fn load_ide_creds(ctx: &Ctx, saw_file: &mut bool) -> Option<KiroCreds> {
    let cache_dir = ctx.home.join(".aws/sso/cache");
    let token_path = cache_dir.join("kiro-auth-token.json");
    let raw = std::fs::read_to_string(&token_path).ok()?;
    *saw_file = true;
    let v: Value = serde_json::from_str(&raw).ok()?;

    let access = str_at(&v, &["accessToken", "access_token"]).unwrap_or_default().to_string();
    let refresh = str_at(&v, &["refreshToken", "refresh_token"]).map(str::to_string);
    if access.is_empty() && refresh.is_none() {
        return None;
    }

    let expires_at = time_at(&v, &["expiresAt", "expires_at"]);
    let profile = arn_of(str_at(&v, &["profileArn", "profile_arn"]));
    let region = region_of(profile.as_deref(), str_at(&v, &["region"]));
    let provider = str_at(&v, &["provider"]).unwrap_or("");
    let builder_id = provider.eq_ignore_ascii_case("builderid");
    let auth_method = str_at(&v, &["authMethod", "auth_method"]).unwrap_or("");

    let mut client_id = str_at(&v, &["clientId", "client_id"]).map(str::to_string);
    let mut client_secret = str_at(&v, &["clientSecret", "client_secret"]).map(str::to_string);
    if client_id.is_none() {
        if let Some(hash) = str_at(&v, &["clientIdHash"]) {
            let reg_path = cache_dir.join(format!("{hash}.json"));
            if let Ok(reg_raw) = std::fs::read_to_string(reg_path) {
                if let Ok(reg) = serde_json::from_str::<Value>(&reg_raw) {
                    client_id = str_at(&reg, &["clientId", "client_id"]).map(str::to_string);
                    client_secret = str_at(&reg, &["clientSecret", "client_secret"]).map(str::to_string);
                }
            }
        }
    }

    let method = if auth_method.eq_ignore_ascii_case("social") || client_id.is_none() {
        AuthMethod::Social
    } else {
        AuthMethod::Idc
    };

    Some(KiroCreds {
        access,
        refresh,
        expires_at,
        region,
        profile,
        method,
        builder_id,
        client_id,
        client_secret,
        token_url: None,
        account_hint: None,
        source: "kiro_ide",
    })
}

fn magpie_account_hint(ctx: &Ctx) -> Option<String> {
    for path in magpie_auth_paths(ctx) {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(root) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(entry) = root.get("kiro") else {
            continue;
        };
        let item = match entry {
            Value::Array(arr) => arr.first()?,
            Value::Object(_) => entry,
            _ => continue,
        };
        if let Some(hint) = str_at(item, &["accountId"]) {
            return Some(hint.to_string());
        }
    }
    None
}

fn load_magpie_creds(ctx: &Ctx, saw_file: &mut bool) -> Option<KiroCreds> {
    for path in magpie_auth_paths(ctx) {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(root) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(entry) = root.get("kiro") else {
            continue;
        };
        *saw_file = true;

        let item = match entry {
            Value::Array(arr) => arr.first()?,
            Value::Object(_) => entry,
            _ => continue,
        };

        let kind = str_at(item, &["type"]).unwrap_or("");
        if kind == "api" {
            let key = str_at(item, &["key"])?;
            return Some(KiroCreds {
                access: key.to_string(),
                refresh: None,
                expires_at: None,
                region: DEFAULT_REGION.to_string(),
                profile: None,
                method: AuthMethod::ApiKey,
                builder_id: false,
                client_id: None,
                client_secret: None,
                token_url: None,
                account_hint: str_at(item, &["accountId"]).map(str::to_string),
                source: "magpie_api",
            });
        }

        if kind != "oauth" {
            continue;
        }

        let access = str_at(item, &["access"]).unwrap_or_default().to_string();
        let refresh = str_at(item, &["refresh"]).map(str::to_string);
        if access.is_empty() && refresh.is_none() {
            continue;
        }

        let profile = arn_of(str_at(item, &["profileArn"]));
        let region = region_of(profile.as_deref(), str_at(item, &["region"]));
        let login_provider = str_at(item, &["loginProvider"]).unwrap_or("");
        let builder_id = login_provider.eq_ignore_ascii_case("builderid");
        let method = match str_at(item, &["method"]).unwrap_or("social") {
            "idc" => AuthMethod::Idc,
            "external-idp" => AuthMethod::ExternalIdp,
            _ => AuthMethod::Social,
        };

        return Some(KiroCreds {
            access,
            refresh,
            expires_at: item.get("expires").and_then(parse_time),
            region,
            profile,
            method,
            builder_id,
            client_id: str_at(item, &["clientId"]).map(str::to_string),
            client_secret: str_at(item, &["clientSecret"]).map(str::to_string),
            token_url: str_at(item, &["tokenUrl", "tokenURL"]).map(str::to_string),
            account_hint: str_at(item, &["accountId"]).map(str::to_string),
            source: "magpie_oauth",
        });
    }
    None
}

async fn try_refresh(ctx: &Ctx, creds: &KiroCreds) -> Result<Option<KiroCreds>> {
    let Some(refresh_token) = creds.refresh.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let region = if valid_aws_region(&creds.region) {
        creds.region.as_str()
    } else {
        DEFAULT_REGION
    };

    let resp = match creds.method {
        AuthMethod::Social => {
            let url = format!("https://prod.{region}.auth.desktop.kiro.dev/refreshToken");
            ctx.post(&url)
                .header("User-Agent", DESKTOP_UA)
                .json(&json!({ "refreshToken": refresh_token }))
                .send()
                .await?
        }
        AuthMethod::Idc => {
            let url = format!("https://oidc.{region}.amazonaws.com/token");
            ctx.post(&url)
                .json(&json!({
                    "clientId": creds.client_id.as_deref().unwrap_or(""),
                    "clientSecret": creds.client_secret.as_deref().unwrap_or(""),
                    "refreshToken": refresh_token,
                    "grantType": "refresh_token",
                }))
                .send()
                .await?
        }
        AuthMethod::ExternalIdp => {
            let Some(url) = creds.token_url.as_deref().filter(|s| !s.is_empty()) else {
                return Err(FetchError::NotLoggedIn);
            };
            ctx.post(url)
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", creds.client_id.as_deref().unwrap_or("")),
                    ("refresh_token", refresh_token),
                ])
                .send()
                .await?
        }
        AuthMethod::ApiKey => return Ok(None),
    };

    let status = resp.status().as_u16();
    let text = resp.text().await?;
    if matches!(status, 400 | 401 | 403) {
        return Err(FetchError::NotLoggedIn);
    }
    if !(200..300).contains(&status) {
        let body: String = text.chars().take(240).collect();
        return Err(FetchError::Http { status, body });
    }

    let v: Value = serde_json::from_str(&text)
        .map_err(|e| FetchError::parse(format!("kiro refresh json: {e}")))?;
    let access = str_at(&v, &["accessToken", "access_token"])
        .ok_or(FetchError::NotLoggedIn)?
        .to_string();
    let new_refresh = str_at(&v, &["refreshToken", "refresh_token"])
        .map(str::to_string)
        .or_else(|| creds.refresh.clone());
    let expires_in = num_at(&v, &["expiresIn", "expires_in"]).unwrap_or(3600.0).max(1.0);
    let expires_at = Some(Utc::now() + chrono::Duration::seconds(expires_in as i64));
    let profile = arn_of(str_at(&v, &["profileArn", "profile_arn"])).or_else(|| creds.profile.clone());

    Ok(Some(KiroCreds {
        access,
        refresh: new_refresh,
        expires_at,
        profile,
        ..creds.clone()
    }))
}

async fn resolve_profile(ctx: &Ctx, creds: &KiroCreds) -> Option<String> {
    if creds.method == AuthMethod::ApiKey {
        for region in CREDIT_REGIONS {
            let url = format!("https://management.{region}.kiro.dev/");
            let req = ctx
                .post(&url)
                .header("Authorization", format!("Bearer {}", creds.access))
                .header("tokentype", "API_KEY")
                .header("Content-Type", "application/x-amz-json-1.0")
                .header("X-Amz-Target", "AmazonCodeWhispererService.GetProfile")
                .header("x-amzn-codewhisperer-optout", "false")
                .json(&json!({}));
            if let Ok(v) = send_json(req).await {
                if let Some(arn) = arn_of(v.pointer("/profile/arn").and_then(Value::as_str)) {
                    return Some(arn);
                }
            }
        }
        return None;
    }

    if creds.builder_id {
        return Some(BUILDER_ID_PROFILE.to_string());
    }

    for region in CREDIT_REGIONS {
        let url = format!("https://management.{region}.kiro.dev/List-Available-Profiles");
        let mut req = ctx
            .post(&url)
            .header("Authorization", format!("Bearer {}", creds.access))
            .json(&json!({}));
        if creds.method == AuthMethod::ExternalIdp {
            req = req.header("tokentype", "EXTERNAL_IDP");
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let text = resp.text().await.unwrap_or_default();
                if status == 403 && creds.method == AuthMethod::Idc && text.contains("Builder ID") {
                    return Some(BUILDER_ID_PROFILE.to_string());
                }
                if (200..300).contains(&status) {
                    if let Ok(v) = serde_json::from_str::<Value>(&text) {
                        if let Some(arn) = v
                            .get("profiles")
                            .and_then(Value::as_array)
                            .and_then(|arr| arr.iter().find_map(|p| arn_of(p.get("arn").and_then(Value::as_str))))
                        {
                            return Some(arn);
                        }
                    }
                }
            }
            Err(_) => continue,
        }
    }
    None
}

async fn call_usage_limits(ctx: &Ctx, creds: &KiroCreds) -> Result<Value> {
    let region = if valid_aws_region(&creds.region) {
        creds.region.as_str()
    } else {
        DEFAULT_REGION
    };

    if creds.method == AuthMethod::ApiKey {
        let url = format!("https://management.{region}.kiro.dev/?origin=KIRO_CLI");
        let req = ctx
            .post(&url)
            .header("Authorization", format!("Bearer {}", creds.access))
            .header("tokentype", "API_KEY")
            .header("Content-Type", "application/x-amz-json-1.0")
            .header("X-Amz-Target", "AmazonCodeWhispererService.GetUsageLimits")
            .header("x-amzn-codewhisperer-optout", "false")
            .json(&json!({ "origin": "KIRO_CLI" }));
        return send_json(req).await;
    }

    let url = format!("https://management.{region}.kiro.dev/Get-Usage-Limits");
    let mut query = vec![
        ("origin", "KIRO_CLI"),
        ("resourceType", "CREDIT"),
        ("isEmailRequired", "true"),
    ];
    if let Some(profile) = creds.profile.as_deref().filter(|s| !s.is_empty()) {
        query.push(("profileArn", profile));
    }

    let mut req = ctx
        .get(&url)
        .query(&query)
        .header("Authorization", format!("Bearer {}", creds.access));
    if creds.method == AuthMethod::ExternalIdp {
        req = req.header("tokentype", "EXTERNAL_IDP");
    }
    send_json(req).await
}

fn format_plan_name(title: Option<&str>) -> Option<String> {
    let title = title?.trim();
    if title.is_empty() {
        return None;
    }
    let formatted = title
        .split_whitespace()
        .map(|w| {
            let lower = w.to_lowercase();
            if lower == "kiro" {
                "Kiro".to_string()
            } else {
                let mut chars = lower.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if formatted.is_empty() {
        None
    } else {
        Some(formatted)
    }
}

fn parse_usage(body: &Value, creds: &KiroCreds, now: DateTime<Utc>) -> Snapshot {
    let mut snap = Snapshot::new(ProviderId::Kiro, creds.source);
    snap.plan = format_plan_name(body.pointer("/subscriptionInfo/subscriptionTitle").and_then(Value::as_str));
    snap.account = body
        .pointer("/userInfo/email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| creds.account_hint.clone());

    let overage_on = body
        .pointer("/overageConfiguration/overageStatus")
        .and_then(Value::as_str)
        .is_some_and(|s| s.eq_ignore_ascii_case("ENABLED"))
        || body
            .pointer("/overageConfiguration/overageEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);

    let root_reset = body.get("nextDateReset").and_then(parse_time);
    let now_ms = now.timestamp_millis();

    if let Some(list) = body.get("usageBreakdownList").and_then(Value::as_array) {
        for (idx, u) in list.iter().enumerate() {
            if let Some(t) = u.get("freeTrialInfo") {
                let active = t
                    .get("freeTrialStatus")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.eq_ignore_ascii_case("ACTIVE"));
                let limit = num_at(t, &["usageLimitWithPrecision", "usageLimit"]).unwrap_or(0.0);
                if active && limit > 0.0 {
                    let used = num_at(t, &["currentUsageWithPrecision", "currentUsage"]).unwrap_or(0.0);
                    let expiry = t.get("freeTrialExpiry").and_then(parse_time);
                    snap.windows.push(
                        QuotaWindow::new("free_trial", None, pct(used, limit))
                            .kind(WindowKind::Other)
                            .scope("Free trial")
                            .amounts(used, limit, "credits")
                            .resets_at(expiry),
                    );
                }
            }

            let mut bonuses = Vec::new();
            if let Some(arr) = u.get("bonuses").and_then(Value::as_array) {
                for (b_idx, b) in arr.iter().enumerate() {
                    let active = b
                        .get("status")
                        .and_then(Value::as_str)
                        .is_some_and(|s| s.eq_ignore_ascii_case("ACTIVE"));
                    let limit = num_at(b, &["usageLimit"]).unwrap_or(0.0);
                    if !active || limit <= 0.0 {
                        continue;
                    }
                    let expires = b.get("expiresAt").and_then(parse_time);
                    if let Some(exp) = expires {
                        if exp.timestamp_millis() <= now_ms {
                            continue;
                        }
                    }
                    let used = num_at(b, &["currentUsage"]).unwrap_or(0.0);
                    let name = str_at(b, &["displayName"]).unwrap_or("Bonus credits");
                    let id = if arr.len() == 1 && idx == 0 {
                        "bonus".to_string()
                    } else {
                        format!("bonus_{b_idx}")
                    };
                    bonuses.push(
                        QuotaWindow::new(id, None, pct(used, limit))
                            .kind(WindowKind::Other)
                            .scope(name)
                            .amounts(used, limit, "credits")
                            .resets_at(expires),
                    );
                }
            }

            let used = num_at(u, &["currentUsageWithPrecision", "currentUsage"]).unwrap_or(0.0);
            let limit = num_at(u, &["usageLimitWithPrecision", "usageLimit"]).unwrap_or(0.0);
            let cap = if overage_on {
                num_at(u, &["overageCapWithPrecision", "overageCap"]).unwrap_or(0.0)
            } else {
                0.0
            };
            let reset = u.get("nextDateReset").and_then(parse_time).or(root_reset);
            let scope_name = str_at(u, &["displayNamePlural", "displayName"]).unwrap_or("Credits");

            if cap > 0.0 {
                let over = num_at(u, &["currentOveragesWithPrecision", "currentOverages"])
                    .unwrap_or_else(|| (used - limit).max(0.0));
                if limit > 0.0 {
                    let in_plan = used.min(limit);
                    snap.windows.push(
                        QuotaWindow::new("credits", Some(MONTH_MINS), pct(in_plan, limit))
                            .kind(WindowKind::Monthly)
                            .scope(scope_name)
                            .amounts(in_plan, limit, "credits")
                            .resets_at(reset),
                    );
                }
                snap.windows.push(
                    QuotaWindow::new("overage", Some(MONTH_MINS), pct(over, cap))
                        .kind(WindowKind::Monthly)
                        .scope("Overage")
                        .amounts(over, cap, "credits")
                        .resets_at(reset),
                );
            } else if limit > 0.0 {
                snap.windows.push(
                    QuotaWindow::new("credits", Some(MONTH_MINS), pct(used, limit))
                        .kind(WindowKind::Monthly)
                        .scope(scope_name)
                        .amounts(used, limit, "credits")
                        .resets_at(reset),
                );
            }

            snap.windows.extend(bonuses);
        }
    }

    snap
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use tempfile::TempDir;

    fn test_ctx(home: &std::path::Path) -> Ctx {
        Ctx::with(home.to_path_buf(), HashMap::new(), HashMap::new(), OsString::new())
    }

    fn dummy_creds() -> KiroCreds {
        KiroCreds {
            access: "tok".into(),
            refresh: Some("ref".into()),
            expires_at: None,
            region: "us-east-1".into(),
            profile: Some(BUILDER_ID_PROFILE.into()),
            method: AuthMethod::Social,
            builder_id: false,
            client_id: None,
            client_secret: None,
            token_url: None,
            account_hint: None,
            source: "kiro_ide",
        }
    }

    #[test]
    fn not_detected_without_credentials() {
        let dir = TempDir::new().unwrap();
        let ctx = test_ctx(dir.path());
        let err = load_creds(&ctx).unwrap_err();
        assert!(matches!(err, FetchError::NotDetected));
    }

    #[test]
    fn parses_monthly_credits_and_plan_and_email() {
        let now = DateTime::parse_from_rfc3339("2026-11-19T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let body = json!({
            "subscriptionInfo": { "subscriptionTitle": "KIRO PRO" },
            "userInfo": { "email": "dev@example.com" },
            "nextDateReset": 1796083200,
            "usageBreakdownList": [{
                "displayName": "Credit",
                "displayNamePlural": "Credits",
                "currentUsageWithPrecision": 125.5,
                "usageLimitWithPrecision": 1000.0
            }]
        });

        let snap = parse_usage(&body, &dummy_creds(), now);
        assert_eq!(snap.provider, ProviderId::Kiro);
        assert_eq!(snap.plan.as_deref(), Some("Kiro Pro"));
        assert_eq!(snap.account.as_deref(), Some("dev@example.com"));
        assert_eq!(snap.windows.len(), 1);
        let w = &snap.windows[0];
        assert_eq!(w.id, "credits");
        assert_eq!(w.kind, WindowKind::Monthly);
        assert_eq!(w.used, Some(125.5));
        assert_eq!(w.limit, Some(1000.0));
        assert_eq!(w.unit.as_deref(), Some("credits"));
        assert!((w.used_percent - 12.55).abs() < 1e-6);
        assert!(w.resets_at.is_some());
    }

    #[test]
    fn parses_active_unexpired_bonuses_and_overage() {
        let now = DateTime::parse_from_rfc3339("2026-11-19T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let future_sec = now.timestamp() + 86_400;
        let past_sec = now.timestamp() - 3600;

        let body = json!({
            "subscriptionInfo": { "subscriptionTitle": "KIRO FREE" },
            "overageConfiguration": { "overageStatus": "ENABLED" },
            "usageBreakdownList": [{
                "displayNamePlural": "Credits",
                "currentUsageWithPrecision": 550.0,
                "usageLimitWithPrecision": 500.0,
                "overageCapWithPrecision": 200.0,
                "currentOveragesWithPrecision": 50.0,
                "freeTrialInfo": {
                    "freeTrialStatus": "ACTIVE",
                    "currentUsageWithPrecision": 10.0,
                    "usageLimitWithPrecision": 100.0,
                    "freeTrialExpiry": future_sec
                },
                "bonuses": [
                    {
                        "displayName": "Promo Credits",
                        "status": "ACTIVE",
                        "currentUsage": 20.0,
                        "usageLimit": 100.0,
                        "expiresAt": future_sec
                    },
                    {
                        "displayName": "Expired Bonus",
                        "status": "ACTIVE",
                        "currentUsage": 0.0,
                        "usageLimit": 50.0,
                        "expiresAt": past_sec
                    },
                    {
                        "displayName": "Inactive Bonus",
                        "status": "EXPIRED",
                        "currentUsage": 0.0,
                        "usageLimit": 50.0,
                        "expiresAt": future_sec
                    }
                ]
            }]
        });

        let snap = parse_usage(&body, &dummy_creds(), now);
        assert_eq!(snap.plan.as_deref(), Some("Kiro Free"));
        assert_eq!(snap.windows.len(), 4);
        assert_eq!(snap.windows[0].id, "free_trial");
        assert_eq!(snap.windows[1].id, "credits");
        assert_eq!(snap.windows[1].used, Some(500.0));
        assert_eq!(snap.windows[2].id, "overage");
        assert_eq!(snap.windows[2].used, Some(50.0));
        assert_eq!(snap.windows[2].limit, Some(200.0));
        assert_eq!(snap.windows[3].scope.as_deref(), Some("Promo Credits"));
        assert_eq!(snap.windows[3].used, Some(20.0));
        assert_eq!(snap.windows[3].limit, Some(100.0));
    }

    #[test]
    fn detects_expired_token_needs_refresh_and_loads_ide_and_magpie() {
        let dir = TempDir::new().unwrap();
        let cache = dir.path().join(".aws/sso/cache");
        std::fs::create_dir_all(&cache).unwrap();
        let expired = (Utc::now() - chrono::Duration::minutes(5)).to_rfc3339();
        std::fs::write(
            cache.join("kiro-auth-token.json"),
            serde_json::to_string(&json!({
                "accessToken": "old_tok",
                "refreshToken": "ref_tok",
                "expiresAt": expired,
                "authMethod": "social",
                "provider": "Google"
            }))
            .unwrap(),
        )
        .unwrap();

        let magpie_dir = dir.path().join(".config/magpie");
        std::fs::create_dir_all(&magpie_dir).unwrap();
        std::fs::write(
            magpie_dir.join("plugin-auth.json"),
            serde_json::to_string(&json!({
                "kiro": {
                    "type": "oauth",
                    "source": "kiro",
                    "accountId": "user@example.com"
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let ctx = test_ctx(dir.path());
        let creds = load_creds(&ctx).unwrap();
        assert_eq!(creds.access, "old_tok");
        assert_eq!(creds.refresh.as_deref(), Some("ref_tok"));
        assert_eq!(creds.account_hint.as_deref(), Some("user@example.com"));
        assert!(needs_refresh(&creds), "expired token should trigger refresh");
    }
}
