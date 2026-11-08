//! OpenCode Go subscription windows (5-hour, weekly, monthly).
//!
//! Reuses the OpenCode CLI login: `auth.json` (legacy) or the `credential` table in `opencode.db`
//! (v2), both under `$XDG_DATA_HOME/opencode`. A key in settings or `OPENCODE_API_KEY` wins.

use chrono::{Duration, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::ctx::{read_json, send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{num_at, time_at};

const USAGE_URL: &str = "https://opencode.ai/zen/go/v1/usage";
const INTEGRATIONS: [&str; 2] = ["opencode-go", "opencode"];

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let token = ctx.api_key(ProviderId::Opencode).or_else(|| cli_token(ctx)).ok_or(FetchError::NotDetected)?;
    let body = send_json(ctx.get(USAGE_URL).bearer_auth(token)).await.map_err(|e| match e {
        FetchError::Http { status: 401 | 403, .. } => FetchError::AuthExpired,
        FetchError::Http { status: 402 | 404, .. } => FetchError::Unsupported("no_subscription".into()),
        other => other,
    })?;
    parse_usage(&body)
}

fn data_dir(ctx: &Ctx) -> std::path::PathBuf {
    ctx.xdg_data().join("opencode")
}

fn cli_token(ctx: &Ctx) -> Option<String> {
    let dir = data_dir(ctx);
    let db = match ctx.env("OPENCODE_DB") {
        Some(p) if std::path::Path::new(p).is_absolute() => p.into(),
        Some(name) => dir.join(name),
        None => dir.join("opencode.db"),
    };
    if let Some(t) = db_token(&db) {
        return Some(t);
    }
    let auth = read_json(&dir.join("auth.json")).ok()?;
    INTEGRATIONS.iter().find_map(|k| credential_token(&auth[*k]))
}

fn db_token(path: &std::path::Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    let mut stmt = conn
        .prepare(
            "SELECT integration_id, value FROM credential WHERE active = 1 AND integration_id IN ('opencode-go', 'opencode') \
             ORDER BY CASE integration_id WHEN 'opencode-go' THEN 0 ELSE 1 END",
        )
        .ok()?;
    let rows: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(1)).ok()?.flatten().collect();
    rows.iter()
        .filter_map(|raw| serde_json::from_str::<Value>(raw).ok())
        .find_map(|v| credential_token(&v))
}

fn credential_token(v: &Value) -> Option<String> {
    ["key", "access", "access_token", "token"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn parse_usage(body: &Value) -> Result<Snapshot> {
    let usage = body.get("usage").unwrap_or(body);
    let mut snap = Snapshot::new(ProviderId::Opencode, "api");
    snap.plan = Some("OpenCode Go".into());
    for (key, mins, kind) in [
        ("rolling", Some(300), WindowKind::Session),
        ("weekly", Some(10080), WindowKind::Weekly),
        ("monthly", None, WindowKind::Monthly),
    ] {
        let w = &usage[key];
        let pct = num_at(w, &["usagePercent", "usedPercent", "percentUsed", "percent", "usage_percent", "used_percent"]).or_else(|| {
            let used = num_at(w, &["used", "usage"])?;
            let limit = num_at(w, &["limit", "total", "quota"]).filter(|l| *l > 0.0)?;
            Some(used / limit * 100.0)
        });
        let Some(pct) = pct else { continue };
        let reset = time_at(w, &["resetAt", "resetsAt", "reset_at", "resets_at"]).or_else(|| {
            num_at(w, &["resetInSec", "resetInSeconds", "reset_in_sec"]).map(|s| Utc::now() + Duration::seconds(s as i64))
        });
        snap.windows.push(QuotaWindow::new(key, mins, pct).kind(kind).resets_at(reset));
    }
    if snap.windows.is_empty() {
        return Err(FetchError::Unsupported("no_subscription".into()));
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_usage() {
        let body = json!({"usage": {
            "rolling": {"usagePercent": 12.5, "resetInSec": 3600},
            "weekly": {"percent": 40, "resetAt": "2025-11-10T00:00:00Z"},
            "monthly": {"used": 30, "limit": 60}
        }});
        let s = parse_usage(&body).unwrap();
        assert_eq!(s.windows.len(), 3);
        assert_eq!(s.windows[2].used_percent, 50.0);
        assert_eq!(s.windows[2].kind, WindowKind::Monthly);
        assert!(s.windows[0].resets_at.is_some());
    }

    #[test]
    fn reads_tokens_from_auth_json_and_db() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("opencode.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE credential (integration_id TEXT, value TEXT, active INTEGER);
             INSERT INTO credential VALUES ('opencode', '{\"type\":\"key\",\"key\":\"general\"}', 1);
             INSERT INTO credential VALUES ('opencode-go', '{\"type\":\"key\",\"key\":\"go\"}', 1);",
        )
        .unwrap();
        assert_eq!(db_token(&db).as_deref(), Some("go"));
        assert_eq!(credential_token(&json!({"type": "api", "key": "k"})).as_deref(), Some("k"));
    }
}
