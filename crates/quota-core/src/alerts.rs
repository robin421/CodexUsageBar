//! Low-quota alerts: fire once per window per reset cycle when it crosses into warning or
//! critical, and once more when a window that alerted has reset.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{Level, ProviderId, ProviderReport, QuotaWindow, Status, Thresholds};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    Warning,
    Critical,
    Restored,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub kind: AlertKind,
    pub provider: ProviderId,
    pub window: QuotaWindow,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
struct Seen {
    /// Reset timestamp (seconds) identifying the window's current cycle.
    cycle: Option<i64>,
    level: Level,
}

#[derive(Default)]
pub struct AlertTracker {
    seen: HashMap<String, Seen>,
    path: Option<PathBuf>,
}

impl AlertTracker {
    pub fn load(dir: PathBuf) -> Self {
        let path = dir.join("alerts.json");
        let seen = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Self { seen, path: Some(path) }
    }

    pub fn evaluate(&mut self, reports: &[ProviderReport], t: &Thresholds) -> Vec<Alert> {
        let mut out = Vec::new();
        for r in reports.iter().filter(|r| r.status == Status::Ok) {
            let Some(snap) = &r.snapshot else { continue };
            for w in &snap.windows {
                let key = format!("{}:{}", r.id.slug(), w.id);
                let cycle = w.resets_at.map(|d| d.timestamp() / 60);
                let level = w.level(t);
                let prev = self.seen.get(&key).copied();
                let new_cycle = prev.is_some_and(|p| p.cycle.is_some() && cycle.is_some() && p.cycle != cycle);
                let mut notified = match prev {
                    Some(p) if !new_cycle => p.level,
                    _ => Level::Normal,
                };
                if new_cycle && prev.is_some_and(|p| p.level > Level::Normal) && level == Level::Normal {
                    out.push(Alert { kind: AlertKind::Restored, provider: r.id, window: w.clone() });
                }
                if level > notified {
                    let kind = if level == Level::Critical { AlertKind::Critical } else { AlertKind::Warning };
                    out.push(Alert { kind, provider: r.id, window: w.clone() });
                    notified = level;
                }
                // Climbing back above a threshold mid-cycle (limits raised) re-arms the alert.
                if level < notified {
                    notified = level;
                }
                self.seen.insert(key, Seen { cycle, level: notified });
            }
        }
        self.persist();
        out
    }

    fn persist(&self) {
        let Some(path) = &self.path else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string(&self.seen) {
            let _ = std::fs::write(path, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Snapshot;
    use chrono::{TimeZone, Utc};

    fn report(used: f64, reset_secs: i64) -> Vec<ProviderReport> {
        let mut s = Snapshot::new(ProviderId::Codex, "oauth");
        s.windows.push(QuotaWindow::new("primary", Some(300), used).resets_at(Utc.timestamp_opt(reset_secs, 0).single()));
        let mut r = ProviderReport::new(ProviderId::Codex, Status::Ok);
        r.snapshot = Some(s);
        vec![r]
    }

    #[test]
    fn fires_once_per_level_per_cycle() {
        let t = Thresholds::default();
        let mut tr = AlertTracker::default();
        assert!(tr.evaluate(&report(50.0, 1_000_000), &t).is_empty());
        let a = tr.evaluate(&report(85.0, 1_000_000), &t);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, AlertKind::Warning);
        assert!(tr.evaluate(&report(86.0, 1_000_000), &t).is_empty());
        let a = tr.evaluate(&report(95.0, 1_000_000), &t);
        assert_eq!(a[0].kind, AlertKind::Critical);
        assert!(tr.evaluate(&report(99.0, 1_000_000), &t).is_empty());
        // New cycle: restored notice, then alerts re-arm.
        let a = tr.evaluate(&report(5.0, 2_000_000), &t);
        assert_eq!(a[0].kind, AlertKind::Restored);
        let a = tr.evaluate(&report(92.0, 2_000_000), &t);
        assert_eq!(a[0].kind, AlertKind::Critical);
    }
}
