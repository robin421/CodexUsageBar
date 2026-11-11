//! GitHub Copilot monthly quotas (premium requests, chat, completions).
//!
//! Token sources, in order: Copilot editor plugins (`github-copilot/apps.json`, `hosts.json`),
//! the GitHub CLI config (`gh/hosts.yml`), then `gh auth token`.

use std::path::PathBuf;

use serde_json::Value;

use crate::ctx::{read_json, send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{num, num_at, str_at, time_at, title_case};

const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let (token, user) = find_token(ctx).await.ok_or(FetchError::NotDetected)?;
    let req = ctx
        .get(USAGE_URL)
        .header("Authorization", format!("token {token}"))
        .header("Editor-Version", "vscode/1.96.2")
        .header("Editor-Plugin-Version", "copilot-chat/0.26.7")
        .header("X-Github-Api-Version", "2025-04-01");
    let body = send_json(req).await.map_err(|e| match e {
        FetchError::Http { status: 401, .. } => FetchError::AuthExpired,
        FetchError::Http { status: 404, .. } => FetchError::Unsupported("no_subscription".into()),
        other => other,
    })?;
    let mut snap = parse_usage(&body)?;
    snap.account = user;
    Ok(snap)
}

fn copilot_search_dirs(ctx: &Ctx) -> Vec<PathBuf> {
    let mut copilot_dirs = if cfg!(windows) {
        vec![
            ctx.local_app_data().join("github-copilot"),
            ctx.xdg_config().join("github-copilot"),
        ]
    } else {
        vec![
            ctx.app_config_dir().join("github-copilot"),
            ctx.home.join(".config/github-copilot"),
            ctx.xdg_config().join("github-copilot"),
        ]
    };
    let mut seen = std::collections::HashSet::new();
    copilot_dirs.retain(|d| seen.insert(d.clone()));
    copilot_dirs
}

async fn find_token(ctx: &Ctx) -> Option<(String, Option<String>)> {
    let copilot_dirs = copilot_search_dirs(ctx);
    for dir in &copilot_dirs {
        for file in ["apps.json", "hosts.json"] {
            let Ok(v) = read_json(&dir.join(file)) else { continue };
            let Some(obj) = v.as_object() else { continue };
            for (host, entry) in obj {
                if !host.starts_with("github.com") {
                    continue;
                }
                if let Some(t) = str_at(entry, &["oauth_token"]) {
                    return Some((t.to_string(), str_at(entry, &["user"]).map(str::to_string)));
                }
            }
        }
    }

    let gh_dir = if cfg!(windows) { ctx.app_config_dir().join("GitHub CLI") } else { ctx.xdg_config().join("gh") };
    let gh_dir = ctx.env("GH_CONFIG_DIR").map(Into::into).unwrap_or(gh_dir);
    if let Ok(text) = std::fs::read_to_string(gh_dir.join("hosts.yml")) {
        if let Some((token, user)) = parse_hosts_yml(&text) {
            return Some((token, user));
        }
    }

    if let Some(t) = ctx.env("GH_TOKEN").or_else(|| ctx.env("GITHUB_TOKEN")) {
        return Some((t.to_string(), None));
    }
    let gh = ctx.find_bin("gh")?;
    let out = ctx.command(gh).args(["auth", "token", "--hostname", "github.com"]).output().await.ok()?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !token.is_empty()).then_some((token, None))
}

/// Minimal reader for the `github.com:` block of gh's hosts.yml (tokens stored outside the keyring).
fn parse_hosts_yml(text: &str) -> Option<(String, Option<String>)> {
    let mut in_github = false;
    let (mut token, mut user) = (None, None);
    for line in text.lines() {
        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_github = line.trim_end_matches(':').trim() == "github.com";
            continue;
        }
        if !in_github {
            continue;
        }
        let l = line.trim();
        if let Some(v) = l.strip_prefix("oauth_token:") {
            token = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = l.strip_prefix("user:") {
            user = Some(v.trim().trim_matches('"').to_string());
        }
    }
    token.filter(|t| !t.is_empty()).map(|t| (t, user))
}

pub fn parse_usage(body: &Value) -> Result<Snapshot> {
    if str_at(body, &["access_type_sku"]) == Some("no_access") {
        return Err(FetchError::Unsupported("no_subscription".into()));
    }
    let mut snap = Snapshot::new(ProviderId::Copilot, "api");
    snap.plan = str_at(body, &["copilot_plan"]).map(|p| match p {
        "individual" => "Copilot Pro".to_string(),
        "individual_pro" => "Copilot Pro+".to_string(),
        other => format!("Copilot {}", title_case(other)),
    });
    let reset = time_at(body, &["quota_reset_date", "quota_reset_date_utc", "limited_user_reset_date"]);

    if let Some(snaps) = body["quota_snapshots"].as_object() {
        for (key, scope) in [("premium_interactions", "Premium requests"), ("chat", "Chat"), ("completions", "Completions")] {
            let Some(q) = snaps.get(key) else { continue };
            if q["unlimited"].as_bool() == Some(true) {
                continue;
            }
            let entitlement = num_at(q, &["entitlement"]);
            let remaining = num_at(q, &["remaining"]);
            let used_pct = match (num_at(q, &["percent_remaining"]), entitlement, remaining) {
                (Some(p), _, _) => 100.0 - p,
                (None, Some(e), Some(r)) if e > 0.0 => (e - r) / e * 100.0,
                _ => continue,
            };
            let mut w = QuotaWindow::new(key, None, used_pct).kind(WindowKind::Monthly).scope(scope).resets_at(reset);
            if let (Some(e), Some(r)) = (entitlement, remaining) {
                if e > 0.0 {
                    w = w.amounts((e - r).max(0.0), e, "requests");
                }
            }
            snap.windows.push(w);
        }
    }

    // Copilot Free reports remaining counts against monthly totals instead.
    if snap.windows.is_empty() {
        if let (Some(left), Some(total)) = (body["limited_user_quotas"].as_object(), body["monthly_quotas"].as_object()) {
            for (key, scope) in [("chat", "Chat"), ("completions", "Completions")] {
                let (Some(r), Some(t)) = (left.get(key).and_then(num), total.get(key).and_then(num)) else { continue };
                if t <= 0.0 {
                    continue;
                }
                snap.windows.push(
                    QuotaWindow::new(key, None, (t - r) / t * 100.0)
                        .kind(WindowKind::Monthly)
                        .scope(scope)
                        .resets_at(reset)
                        .amounts(t - r, t, "requests"),
                );
            }
        }
    }
    if snap.windows.is_empty() && body["quota_snapshots"].is_null() {
        return Err(FetchError::Unsupported("no_subscription".into()));
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_paid_plan() {
        let body = json!({
            "copilot_plan": "individual",
            "quota_reset_date": "2025-12-01",
            "quota_snapshots": {
                "premium_interactions": {"entitlement": 300, "remaining": 75, "percent_remaining": 25.0, "unlimited": false},
                "chat": {"unlimited": true},
                "completions": {"unlimited": true}
            }
        });
        let s = parse_usage(&body).unwrap();
        assert_eq!(s.plan.as_deref(), Some("Copilot Pro"));
        assert_eq!(s.windows.len(), 1);
        assert_eq!(s.windows[0].remaining_percent(), 25.0);
        assert_eq!(s.windows[0].used, Some(225.0));
        assert!(s.windows[0].resets_at.is_some());
    }

    #[test]
    fn parses_free_plan() {
        let body = json!({
            "copilot_plan": "free",
            "limited_user_reset_date": "2025-12-01",
            "limited_user_quotas": {"chat": 40, "completions": 1800},
            "monthly_quotas": {"chat": 50, "completions": 2000}
        });
        let s = parse_usage(&body).unwrap();
        assert_eq!(s.windows.len(), 2);
        assert_eq!(s.windows[0].remaining_percent(), 80.0);
    }

    #[test]
    fn no_access_is_not_a_subscription() {
        let body = json!({"copilot_plan": "individual", "access_type_sku": "no_access", "quota_snapshots": null});
        assert_eq!(parse_usage(&body).unwrap_err(), FetchError::Unsupported("no_subscription".into()));
    }

    #[test]
    fn reads_hosts_yml() {
        let yml = "github.com:\n    users:\n        me:\n            oauth_token: gho_abc\n    oauth_token: gho_abc\n    user: me\n    git_protocol: https\n";
        assert_eq!(parse_hosts_yml(yml), Some(("gho_abc".into(), Some("me".into()))));
        assert_eq!(parse_hosts_yml("github.com:\n    user: me\n"), None);
    }

    #[test]
    fn search_dirs_include_macos_app_support_and_config() {
        let fake_home = PathBuf::from("/test/fake_home");
        let ctx = Ctx::with(
            fake_home.clone(),
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            std::ffi::OsString::new(),
        );
        let dirs = copilot_search_dirs(&ctx);
        if cfg!(target_os = "macos") {
            assert!(dirs.contains(&fake_home.join("Library/Application Support/github-copilot")));
            assert!(dirs.contains(&fake_home.join(".config/github-copilot")));
        } else if !cfg!(windows) {
            assert!(dirs.contains(&fake_home.join(".config/github-copilot")));
        }
        // Ensure no duplicates
        let mut set = std::collections::HashSet::new();
        for d in &dirs {
            assert!(set.insert(d.clone()), "Duplicate dir found: {:?}", d);
        }
    }
}
