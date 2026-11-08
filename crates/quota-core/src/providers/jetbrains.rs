//! JetBrains AI Assistant monthly credits, read from the IDE's local
//! `options/AIAssistantQuotaManager2.xml` (no network access).

use std::path::PathBuf;

use serde_json::Value;

use crate::ctx::Ctx;
use crate::error::{FetchError, Result};
use crate::model::{Balance, ProviderId, QuotaWindow, Snapshot, WindowKind};
use crate::util::{num_at, pct, time_at};

const QUOTA_FILE: &str = "AIAssistantQuotaManager2.xml";

pub async fn fetch(ctx: &Ctx) -> Result<Snapshot> {
    let (ide, path) = latest_quota_file(ctx).ok_or(FetchError::NotDetected)?;
    let xml = std::fs::read_to_string(&path).map_err(|_| FetchError::NotDetected)?;
    let mut snap = parse_xml(&xml)?;
    snap.account = Some(ide);
    Ok(snap)
}

fn latest_quota_file(ctx: &Ctx) -> Option<(String, PathBuf)> {
    let base = ctx.app_config_dir();
    let roots = [base.join("JetBrains"), base.join("Google")];
    roots
        .iter()
        .filter_map(|r| std::fs::read_dir(r).ok())
        .flatten()
        .flatten()
        .filter_map(|e| {
            let file = e.path().join("options").join(QUOTA_FILE);
            let modified = std::fs::metadata(&file).and_then(|m| m.modified()).ok()?;
            Some((modified, e.file_name().to_string_lossy().into_owned(), file))
        })
        .max_by_key(|(m, _, _)| *m)
        .map(|(_, name, file)| (name, file))
}

fn option_json(xml: &str, name: &str) -> Option<Value> {
    let re = regex::Regex::new(&format!(r#"<option\s+name="{name}"\s+value="([^"]*)""#)).ok()?;
    let raw = re.captures(xml)?.get(1)?.as_str();
    serde_json::from_str(&unescape(raw)).ok()
}

fn unescape(s: &str) -> String {
    s.replace("&#10;", "\n")
        .replace("&#13;", "\r")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub fn parse_xml(xml: &str) -> Result<Snapshot> {
    let quota = option_json(xml, "quotaInfo").ok_or_else(|| FetchError::parse("quotaInfo missing"))?;
    let refill = option_json(xml, "nextRefill");
    let tariff = quota.get("tariffQuota").unwrap_or(&quota);
    let used = num_at(tariff, &["current"]).or_else(|| num_at(&quota, &["current"])).unwrap_or(0.0);
    let max = num_at(tariff, &["maximum"]).or_else(|| num_at(&quota, &["maximum"])).unwrap_or(0.0);
    if max <= 0.0 {
        return Err(FetchError::Unsupported("no_subscription".into()));
    }
    let reset = refill.as_ref().and_then(|r| time_at(r, &["next"])).or_else(|| time_at(&quota, &["until"]));

    let mut snap = Snapshot::new(ProviderId::Jetbrains, "local");
    snap.plan = quota.get("type").and_then(Value::as_str).map(|t| format!("AI {}", crate::util::title_case(&t.to_lowercase())));
    snap.windows.push(
        QuotaWindow::new("monthly", None, pct(used, max))
            .kind(WindowKind::Monthly)
            .resets_at(reset)
            .amounts(used, max, "credits"),
    );
    if let Some(top) = quota.get("topUpQuota") {
        if let Some(avail) = num_at(top, &["available"]).filter(|a| *a > 0.0) {
            snap.balances.push(Balance { id: "top_up".into(), amount: avail, currency: "credits".into(), total: num_at(top, &["maximum"]), unlimited: false });
        }
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quota_xml() {
        let xml = r#"<application>
  <component name="AIAssistantQuotaManager2">
    <option name="nextRefill" value="{&#10;  &quot;type&quot;: &quot;Known&quot;,&#10;  &quot;next&quot;: &quot;2025-12-01T00:00:00Z&quot;&#10;}" />
    <option name="quotaInfo" value="{&#10;  &quot;type&quot;: &quot;Available&quot;,&#10;  &quot;current&quot;: &quot;7500.0&quot;,&#10;  &quot;maximum&quot;: &quot;10000.0&quot;,&#10;  &quot;until&quot;: &quot;2025-12-31T00:00:00Z&quot;,&#10;  &quot;tariffQuota&quot;: {&quot;current&quot;: &quot;7500.0&quot;, &quot;maximum&quot;: &quot;10000.0&quot;, &quot;available&quot;: &quot;2500.0&quot;}&#10;}" />
  </component>
</application>"#;
        let s = parse_xml(xml).unwrap();
        assert_eq!(s.windows[0].remaining_percent(), 25.0);
        assert_eq!(s.windows[0].resets_at.unwrap().to_rfc3339(), "2025-12-01T00:00:00+00:00");
    }
}
