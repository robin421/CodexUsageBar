//! Last successful snapshot per provider, so the panel has data immediately at launch
//! and failed refreshes can fall back to stale-but-known numbers.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::model::{ProviderId, ProviderReport, Snapshot, Status};

pub struct SnapshotCache {
    path: PathBuf,
}

impl SnapshotCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { path: dir.join("snapshots.json") }
    }

    pub fn load(&self) -> HashMap<ProviderId, Snapshot> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Stores fresh snapshots and drops providers that are now logged out or removed.
    pub fn update(&self, reports: &[ProviderReport]) -> HashMap<ProviderId, Snapshot> {
        let mut map = self.load();
        for r in reports {
            match (r.status, &r.snapshot) {
                (Status::Ok, Some(s)) => {
                    map.insert(r.id, s.clone());
                }
                (Status::Issue | Status::Disabled, _) => {
                    map.remove(&r.id);
                }
                _ => {}
            }
        }
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string(&map) {
            let _ = std::fs::write(&self.path, text);
        }
        map
    }

    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::QuotaWindow;

    #[test]
    fn round_trips_and_evicts() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SnapshotCache::new(dir.path().to_path_buf());
        let mut snap = Snapshot::new(ProviderId::Claude, "oauth");
        snap.windows.push(QuotaWindow::new("five_hour", Some(300), 10.0));
        let mut ok = ProviderReport::new(ProviderId::Claude, Status::Ok);
        ok.snapshot = Some(snap.clone());
        cache.update(&[ok]);
        assert_eq!(cache.load().get(&ProviderId::Claude), Some(&snap));

        cache.update(&[ProviderReport::new(ProviderId::Claude, Status::Issue)]);
        assert!(cache.load().is_empty());
        cache.clear();
    }
}
