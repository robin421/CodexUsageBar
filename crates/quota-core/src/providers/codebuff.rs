//! Codebuff credits, using the CLI login in `~/.config/manicode/credentials.json`.

use serde_json::{json, Value};

use crate::ctx::{read_json, send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{Balance, ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{num_at, pct, str_at, time_at};

const USAGE_URL: &str = "https://www.codebuff.com/api/v1/usage";

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let token = ctx.api_key(ProviderId::Codebuff).or_else(|| cli_token(ctx)).ok_or(FetchError::NotDetected)?;
    let req = ctx
        .post(USAGE_URL)
        .bearer_auth(token)
        .json(&json!({"fingerprintId": "codexusagebar"}));
    let body = send_json(req).await.map_err(|e| match e {
        FetchError::Http { status: 401 | 403, .. } => FetchError::AuthExpired,
        other => other,
    })?;
    parse_usage(&body)
}

fn cli_token(ctx: &Ctx) -> Option<String> {
    let v = read_json(&ctx.home.join(".config/manicode/credentials.json")).ok()?;
    str_at(&v["default"], &["authToken"]).or_else(|| str_at(&v, &["authToken"])).map(str::to_string)
}

pub fn parse_usage(body: &Value) -> Result<Snapshot> {
    let used = num_at(body, &["usage", "used"]);
    let total = num_at(body, &["quota", "limit"]);
    let remaining = num_at(body, &["remainingBalance", "remaining"]);
    let reset = time_at(body, &["next_quota_reset", "nextQuotaReset"]);
    let mut snap = Snapshot::new(ProviderId::Codebuff, "api");
    if let (Some(u), Some(t)) = (used, total) {
        if t > 0.0 {
            snap.windows.push(
                QuotaWindow::new("credits", None, pct(u, t)).kind(WindowKind::Monthly).resets_at(reset).amounts(u, t, "credits"),
            );
        }
    }
    if let Some(r) = remaining {
        snap.balances.push(Balance { id: "credits".into(), amount: r, currency: "credits".into(), total, unlimited: false });
    }
    if snap.windows.is_empty() && snap.balances.is_empty() {
        return Err(FetchError::parse("no Codebuff usage data"));
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usage() {
        let s = parse_usage(&json!({"usage": 250, "quota": 1000, "remainingBalance": 750, "next_quota_reset": "2025-12-01T00:00:00Z"})).unwrap();
        assert_eq!(s.windows[0].remaining_percent(), 75.0);
        assert_eq!(s.balances[0].amount, 750.0);
    }
}
