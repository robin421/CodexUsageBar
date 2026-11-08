//! Reads AI coding tool quotas from logins that already exist on this machine.

pub mod alerts;
pub mod cache;
pub mod ctx;
pub mod error;
pub mod format;
pub mod icon;
pub mod model;
pub mod providers;
pub mod util;

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::Utc;
use futures::future::join_all;

pub use ctx::Ctx;
pub use error::FetchError;
pub use model::*;

/// Refreshes every enabled provider concurrently.
///
/// `previous` supplies the last good snapshot per provider so transient failures degrade to
/// `Status::Stale` instead of blanking the card.
pub async fn refresh_all(
    ctx: &Ctx,
    disabled: &HashSet<ProviderId>,
    previous: &HashMap<ProviderId, Snapshot>,
) -> Vec<ProviderReport> {
    let jobs = ProviderId::ALL.into_iter().map(|id| async move {
        if disabled.contains(&id) {
            return ProviderReport::new(id, Status::Disabled);
        }
        let result = tokio::time::timeout(Duration::from_secs(25), providers::fetch(id, ctx))
            .await
            .unwrap_or_else(|_| Err(FetchError::Network("timed out".into())));
        report_from(id, result, previous.get(&id))
    });
    join_all(jobs).await
}

pub fn report_from(id: ProviderId, result: Result<Snapshot, FetchError>, previous: Option<&Snapshot>) -> ProviderReport {
    match result {
        Ok(snap) => {
            let mut r = ProviderReport::new(id, Status::Ok);
            r.snapshot = Some(snap);
            r
        }
        Err(e) => {
            let transient = matches!(e, FetchError::Network(_) | FetchError::Parse(_))
                || matches!(e, FetchError::Http { status, .. } if status >= 429);
            let mut r = ProviderReport::new(id, Status::Issue);
            if let (true, Some(prev)) = (transient, previous) {
                r.status = Status::Stale;
                r.snapshot = Some(prev.clone());
            }
            r.issue = Some(e.into_issue(id));
            r.checked_at = Utc::now();
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_errors_keep_previous_snapshot() {
        let prev = Snapshot::new(ProviderId::Claude, "oauth");
        let r = report_from(ProviderId::Claude, Err(FetchError::Network("offline".into())), Some(&prev));
        assert_eq!(r.status, Status::Stale);
        assert!(r.snapshot.is_some());

        let r = report_from(ProviderId::Claude, Err(FetchError::AuthExpired), Some(&prev));
        assert_eq!(r.status, Status::Issue);
        assert!(r.snapshot.is_none());
        assert_eq!(r.issue, Some(Issue::AuthExpired { command: Some("claude".into()) }));
    }

    #[test]
    fn not_detected_providers_are_hidden() {
        let r = report_from(ProviderId::Cursor, Err(FetchError::NotDetected), None);
        assert!(!r.visible());
        let o = Overview::from_reports(&[r], &Thresholds::default());
        assert_eq!(o.active_count, 0);
        assert!(o.level.is_none());
    }

    #[test]
    fn overview_picks_tightest_window_across_providers() {
        let mut a = Snapshot::new(ProviderId::Claude, "oauth");
        a.windows.push(QuotaWindow::new("five_hour", Some(300), 50.0));
        let mut b = Snapshot::new(ProviderId::Codex, "oauth");
        b.windows.push(QuotaWindow::new("primary", Some(10080), 85.0));
        let reports = vec![
            report_from(ProviderId::Claude, Ok(a), None),
            report_from(ProviderId::Codex, Ok(b), None),
            report_from(ProviderId::Gemini, Err(FetchError::Network("x".into())), None),
        ];
        let o = Overview::from_reports(&reports, &Thresholds::default());
        assert_eq!(o.tightest_provider, Some(ProviderId::Codex));
        assert_eq!(o.min_remaining, Some(15.0));
        assert_eq!(o.level, Some(Level::Warning));
        assert!(o.has_issue);
        assert_eq!(o.active_count, 3);
    }
}
