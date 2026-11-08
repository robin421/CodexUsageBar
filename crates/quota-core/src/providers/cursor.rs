//! Cursor monthly plan usage.
//!
//! Reads the signed-in desktop app's access token from its `state.vscdb` (SQLite, read-only) and
//! calls `cursor.com/api/usage-summary` with the equivalent session cookie.

use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::ctx::{send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{Balance, ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{jwt_claims, num, num_at, pct, str_at, time_at, title_case};

const SUMMARY_URL: &str = "https://cursor.com/api/usage-summary";

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let db = state_db(ctx);
    if !db.exists() {
        return Err(FetchError::NotDetected);
    }
    let token = read_token(&db).ok_or(FetchError::NotLoggedIn)?;
    let claims = jwt_claims(&token).ok_or(FetchError::NotLoggedIn)?;
    if claims["exp"].as_i64().is_some_and(|exp| exp < chrono::Utc::now().timestamp()) {
        return Err(FetchError::AuthExpired);
    }
    let user_id = claims["sub"]
        .as_str()
        .and_then(|s| s.rsplit('|').next())
        .filter(|s| !s.is_empty())
        .ok_or(FetchError::NotLoggedIn)?;

    let req = ctx
        .get(SUMMARY_URL)
        .header("Cookie", format!("WorkosCursorSessionToken={user_id}%3A%3A{token}"))
        .header("Origin", "https://cursor.com")
        .header("Referer", "https://cursor.com/dashboard");
    let body = send_json(req).await?;
    let mut snap = parse_summary(&body)?;
    snap.account = claims["email"].as_str().map(str::to_string);
    Ok(snap)
}

fn state_db(ctx: &Ctx) -> PathBuf {
    ctx.app_config_dir().join("Cursor/User/globalStorage/state.vscdb")
}

fn read_token(db: &std::path::Path) -> Option<String> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    let raw: String = conn
        .query_row("SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken' LIMIT 1", [], |r| r.get(0))
        .ok()?;
    let t = raw.trim().trim_matches('"').to_string();
    (!t.is_empty()).then_some(t)
}

pub fn parse_summary(body: &Value) -> Result<Snapshot> {
    let mut snap = Snapshot::new(ProviderId::Cursor, "api");
    snap.plan = str_at(body, &["membershipType"]).map(|m| format!("Cursor {}", title_case(m)));
    let reset = time_at(body, &["billingCycleEnd"]);
    let indiv = &body["individualUsage"];
    let plan = &indiv["plan"];

    let used_cents = num_at(plan, &["used"]);
    let limit_cents = num_at(plan, &["limit"]);
    let percent = num_at(plan, &["totalPercentUsed"])
        .or_else(|| match (used_cents, limit_cents) {
            (Some(u), Some(l)) if l > 0.0 => Some(pct(u, l)),
            _ => None,
        })
        .or_else(|| {
            let o = &indiv["overall"];
            let (u, l) = (num_at(o, &["used"])?, num_at(o, &["limit"]).filter(|l| *l > 0.0)?);
            Some(pct(u, l))
        });

    if let Some(p) = percent {
        let mut w = QuotaWindow::new("plan", None, p).kind(WindowKind::Monthly).resets_at(reset);
        if let (Some(u), Some(l)) = (used_cents, limit_cents) {
            if l > 0.0 {
                w = w.amounts(u / 100.0, l / 100.0, "USD");
            }
        }
        snap.windows.push(w);
    }
    for (key, scope) in [("autoPercentUsed", "Auto"), ("apiPercentUsed", "API models")] {
        if let Some(p) = plan.get(key).and_then(num) {
            snap.windows.push(QuotaWindow::new(key, None, p).kind(WindowKind::Monthly).scope(scope).resets_at(reset));
        }
    }
    let od = &indiv["onDemand"];
    if let (Some(u), Some(l)) = (num_at(od, &["used"]), num_at(od, &["limit"])) {
        if l > 0.0 {
            snap.balances.push(Balance { id: "on_demand".into(), amount: (l - u) / 100.0, currency: "USD".into(), total: Some(l / 100.0), unlimited: false });
        }
    }
    if snap.windows.is_empty() && snap.plan.is_none() {
        return Err(FetchError::parse("no Cursor usage data"));
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_summary() {
        let body = json!({
            "billingCycleEnd": "2025-12-01T00:00:00.000Z",
            "membershipType": "pro",
            "individualUsage": {
                "plan": {"used": 1200, "limit": 2000, "totalPercentUsed": 60.0, "autoPercentUsed": 20.0, "apiPercentUsed": 75.0},
                "onDemand": {"used": 500, "limit": 2000}
            }
        });
        let s = parse_summary(&body).unwrap();
        assert_eq!(s.plan.as_deref(), Some("Cursor Pro"));
        assert_eq!(s.windows[0].used, Some(12.0));
        assert_eq!(s.windows[0].limit, Some(20.0));
        assert_eq!(s.windows.len(), 3);
        assert_eq!(s.balances[0].amount, 15.0);
    }

    #[test]
    fn reads_token_from_vscdb() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.vscdb");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE ItemTable (key TEXT, value TEXT); INSERT INTO ItemTable VALUES ('cursorAuth/accessToken', 'abc.def.ghi');").unwrap();
        assert_eq!(read_token(&db).as_deref(), Some("abc.def.ghi"));
    }
}
