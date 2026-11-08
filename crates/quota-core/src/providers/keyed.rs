//! Providers that need an API key from settings or the environment.
//! Region-split services try the international host first, then the China host.

use serde_json::Value;

use crate::ctx::{send_json, Ctx};
use crate::error::{FetchError, Result};
use crate::model::{Balance, ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{num, num_at, pct, str_at, time_at};

fn key(ctx: &Ctx, id: ProviderId) -> Result<String> {
    ctx.api_key(id).ok_or(FetchError::MissingKey)
}

/// Tries each host in order; moves on only when the key is rejected (it belongs to the other region).
async fn get_regional(ctx: &Ctx, urls: &[&str], key: &str, raw_auth: bool) -> Result<(usize, Value)> {
    let mut last = FetchError::MissingKey;
    for (i, url) in urls.iter().enumerate() {
        let req = if raw_auth { ctx.get(url).header("Authorization", key) } else { ctx.get(url).bearer_auth(key) };
        match send_json(req).await {
            Ok(v) => return Ok((i, v)),
            Err(e @ FetchError::Http { status: 401 | 403, .. }) => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

// --- OpenRouter -----------------------------------------------------------------------------

pub async fn openrouter(ctx: &Ctx) -> Result<Snapshot> {
    let k = key(ctx, ProviderId::Openrouter)?;
    let credits = send_json(ctx.get("https://openrouter.ai/api/v1/credits").bearer_auth(&k)).await;
    let key_info = send_json(ctx.get("https://openrouter.ai/api/v1/key").bearer_auth(&k)).await;
    if let (Err(e), Err(_)) = (&credits, &key_info) {
        return Err(e.clone());
    }
    Ok(parse_openrouter(credits.ok().as_ref(), key_info.ok().as_ref()))
}

pub fn parse_openrouter(credits: Option<&Value>, key_info: Option<&Value>) -> Snapshot {
    let mut snap = Snapshot::new(ProviderId::Openrouter, "api");
    if let Some(d) = credits.map(|c| &c["data"]) {
        if let (Some(total), Some(used)) = (num_at(d, &["total_credits"]), num_at(d, &["total_usage"])) {
            snap.balances.push(Balance { id: "credits".into(), amount: total - used, currency: "USD".into(), total: Some(total), unlimited: false });
        }
    }
    if let Some(d) = key_info.map(|k| &k["data"]) {
        snap.account = str_at(d, &["label"]).map(str::to_string);
        if let (Some(limit), Some(usage)) = (num_at(d, &["limit"]), num_at(d, &["usage"])) {
            if limit > 0.0 {
                let kind = match str_at(d, &["limit_reset"]) {
                    Some("daily") => WindowKind::Daily,
                    Some("weekly") => WindowKind::Weekly,
                    Some("monthly") => WindowKind::Monthly,
                    _ => WindowKind::Other,
                };
                snap.windows.push(QuotaWindow::new("key_limit", None, pct(usage, limit)).kind(kind).scope("Key limit").amounts(usage, limit, "USD"));
            }
        }
        if d["is_free_tier"].as_bool() == Some(true) {
            snap.plan = Some("Free tier".into());
        }
    }
    snap
}

// --- DeepSeek -------------------------------------------------------------------------------

pub async fn deepseek(ctx: &Ctx) -> Result<Snapshot> {
    let k = key(ctx, ProviderId::Deepseek)?;
    let body = send_json(ctx.get("https://api.deepseek.com/user/balance").bearer_auth(&k)).await?;
    parse_deepseek(&body)
}

pub fn parse_deepseek(body: &Value) -> Result<Snapshot> {
    let infos = body["balance_infos"].as_array().ok_or_else(|| FetchError::parse("balance_infos missing"))?;
    let mut snap = Snapshot::new(ProviderId::Deepseek, "api");
    for info in infos {
        let (Some(cur), Some(total)) = (str_at(info, &["currency"]), num_at(info, &["total_balance"])) else { continue };
        snap.balances.push(Balance { id: format!("balance_{}", cur.to_lowercase()), amount: total, currency: cur.into(), total: None, unlimited: false });
    }
    Ok(snap)
}

// --- Moonshot / Kimi ------------------------------------------------------------------------

pub async fn moonshot(ctx: &Ctx) -> Result<Snapshot> {
    let k = key(ctx, ProviderId::Moonshot)?;
    let urls = ["https://api.moonshot.ai/v1/users/me/balance", "https://api.moonshot.cn/v1/users/me/balance"];
    let (region, body) = get_regional(ctx, &urls, &k, false).await?;
    parse_moonshot(&body, if region == 0 { "USD" } else { "CNY" })
}

pub fn parse_moonshot(body: &Value, currency: &str) -> Result<Snapshot> {
    let d = &body["data"];
    let avail = num_at(d, &["available_balance"]).ok_or_else(|| FetchError::parse("available_balance missing"))?;
    let mut snap = Snapshot::new(ProviderId::Moonshot, "api");
    snap.balances.push(Balance { id: "available".into(), amount: avail, currency: currency.into(), total: None, unlimited: false });
    if let Some(v) = num_at(d, &["voucher_balance"]).filter(|v| *v > 0.0) {
        snap.balances.push(Balance { id: "voucher".into(), amount: v, currency: currency.into(), total: None, unlimited: false });
    }
    Ok(snap)
}

// --- Z.ai / Zhipu GLM Coding Plan -----------------------------------------------------------

pub async fn zai(ctx: &Ctx) -> Result<Snapshot> {
    let k = key(ctx, ProviderId::Zai)?;
    let urls = ["https://api.z.ai/api/monitor/usage/quota/limit", "https://open.bigmodel.cn/api/monitor/usage/quota/limit"];
    let (_, body) = get_regional(ctx, &urls, &k, true).await?;
    parse_zai(&body)
}

pub fn parse_zai(body: &Value) -> Result<Snapshot> {
    if body["code"].as_i64().is_some_and(|c| c != 200) || body["success"].as_bool() == Some(false) {
        let msg = body["msg"].as_str().unwrap_or("error");
        if msg.contains("coding plan") {
            return Err(FetchError::Unsupported("no_subscription".into()));
        }
        return Err(FetchError::Network(format!("z.ai: {msg}")));
    }
    let d = &body["data"];
    let limits = d["limits"].as_array().ok_or_else(|| FetchError::parse("limits missing"))?;
    let mut snap = Snapshot::new(ProviderId::Zai, "api");
    snap.plan = str_at(d, &["planName", "plan", "packageName", "level"]).map(str::to_string);
    for (i, l) in limits.iter().enumerate() {
        let Some(percent) = num_at(l, &["percentage"]) else { continue };
        let ty = str_at(l, &["type"]).unwrap_or("");
        let unit_mins = match l["unit"].as_i64() {
            Some(1) => Some(1440),
            Some(3) => Some(60),
            Some(5) => Some(1),
            Some(6) => Some(10080),
            _ => None,
        };
        let mins = unit_mins.zip(l["number"].as_u64()).map(|(u, n)| u * n as u32);
        let mut w = if ty == "TIME_LIMIT" {
            QuotaWindow::new(format!("mcp{i}"), None, percent).kind(WindowKind::Monthly).scope("MCP")
        } else {
            QuotaWindow::new(format!("tokens{i}"), mins, percent)
        };
        w = w.resets_at(time_at(l, &["nextResetTime"]));
        if let (Some(total), Some(cur)) = (num_at(l, &["usage"]), num_at(l, &["currentValue"])) {
            if total > 0.0 {
                w.used = Some(cur);
                w.limit = Some(total);
                w.unit = Some(if ty == "TIME_LIMIT" { "calls" } else { "prompts" }.into());
            }
        }
        snap.windows.push(w);
    }
    if snap.windows.is_empty() {
        return Err(FetchError::Unsupported("no_subscription".into()));
    }
    snap.windows.sort_by_key(|w| (w.scope.is_some(), w.window_mins.unwrap_or(u32::MAX)));
    Ok(snap)
}

// --- MiniMax Coding Plan --------------------------------------------------------------------

pub async fn minimax(ctx: &Ctx) -> Result<Snapshot> {
    let k = key(ctx, ProviderId::Minimax)?;
    let urls = [
        "https://api.minimax.io/v1/api/openplatform/coding_plan/remains",
        "https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains",
    ];
    let (_, body) = get_regional(ctx, &urls, &k, false).await?;
    parse_minimax(&body)
}

pub fn parse_minimax(body: &Value) -> Result<Snapshot> {
    if body["base_resp"]["status_code"].as_i64().is_some_and(|c| c != 0) {
        let msg = body["base_resp"]["status_msg"].as_str().unwrap_or("error");
        return Err(FetchError::Network(format!("MiniMax: {msg}")));
    }
    let models = body["model_remains"].as_array().ok_or_else(|| FetchError::Unsupported("no_subscription".into()))?;
    let mut snap = Snapshot::new(ProviderId::Minimax, "api");
    snap.plan = Some("Coding Plan".into());
    for (i, m) in models.iter().enumerate() {
        let name = str_at(m, &["model_name"]).unwrap_or("MiniMax");
        let lanes = [
            ("interval", Some(300), "current_interval_remaining_percent", "current_interval_used_percent", "current_interval_total_count", "current_interval_usage_count", "end_time"),
            ("weekly", Some(10080), "current_weekly_remaining_percent", "current_weekly_used_percent", "current_weekly_total_count", "current_weekly_usage_count", "weekly_end_time"),
        ];
        for (lane, mins, rem_k, used_k, total_k, left_k, end_k) in lanes {
            let total = m.get(total_k).and_then(num).filter(|t| *t > 0.0);
            // `*_usage_count` is the count still available in the window, not the count consumed.
            let left = m.get(left_k).and_then(num);
            let used_pct = m
                .get(used_k)
                .and_then(num)
                .or_else(|| m.get(rem_k).and_then(num).map(|r| 100.0 - r))
                .or_else(|| Some(pct(total? - left?, total?)));
            let Some(used_pct) = used_pct else { continue };
            let mut w = QuotaWindow::new(format!("{lane}{i}"), mins, used_pct).resets_at(time_at(m, &[end_k]));
            if models.len() > 1 {
                w = w.scope(name);
            }
            if let (Some(t), Some(l)) = (total, left) {
                w = w.amounts((t - l).max(0.0), t, "prompts");
            }
            snap.windows.push(w);
        }
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
    fn openrouter_credits_and_limit() {
        let c = json!({"data": {"total_credits": 50.0, "total_usage": 12.5}});
        let k = json!({"data": {"label": "work", "limit": 20.0, "usage": 15.0, "limit_reset": "monthly"}});
        let s = parse_openrouter(Some(&c), Some(&k));
        assert_eq!(s.balances[0].amount, 37.5);
        assert_eq!(s.windows[0].remaining_percent(), 25.0);
        assert_eq!(s.windows[0].kind, WindowKind::Monthly);
    }

    #[test]
    fn deepseek_balance() {
        let s = parse_deepseek(&json!({"is_available": true, "balance_infos": [{"currency": "CNY", "total_balance": "110.00"}]})).unwrap();
        assert_eq!(s.balances[0].amount, 110.0);
        assert_eq!(s.balances[0].currency, "CNY");
    }

    #[test]
    fn moonshot_balance() {
        let s = parse_moonshot(&json!({"code": 0, "data": {"available_balance": 49.5, "voucher_balance": 0, "cash_balance": 49.5}}), "USD").unwrap();
        assert_eq!(s.balances.len(), 1);
    }

    #[test]
    fn zai_limits() {
        let body = json!({"code": 200, "success": true, "data": {"limits": [
            {"type": "TIME_LIMIT", "unit": 5, "number": 1, "usage": 1000, "currentValue": 120, "remaining": 880, "percentage": 12, "nextResetTime": 1764547200000_i64},
            {"type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 34, "nextResetTime": 1762596000000_i64}
        ]}});
        let s = parse_zai(&body).unwrap();
        assert_eq!(s.windows[0].window_mins, Some(300));
        assert_eq!(s.windows[0].kind, WindowKind::Session);
        assert_eq!(s.windows[1].scope.as_deref(), Some("MCP"));
        assert_eq!(s.windows[1].used, Some(120.0));
        assert_eq!(parse_zai(&json!({"code": 500, "msg": "当前用户不存在coding plan"})).unwrap_err(), FetchError::Unsupported("no_subscription".into()));
    }

    #[test]
    fn minimax_counts_are_remaining() {
        let body = json!({"base_resp": {"status_code": 0}, "model_remains": [
            {"model_name": "MiniMax-M2", "current_interval_total_count": 300, "current_interval_usage_count": 240, "end_time": 1762596000000_i64}
        ]});
        let s = parse_minimax(&body).unwrap();
        assert_eq!(s.windows.len(), 1);
        assert_eq!(s.windows[0].remaining_percent(), 80.0);
        assert_eq!(s.windows[0].used, Some(60.0));
    }
}
