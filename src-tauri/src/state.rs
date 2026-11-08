use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use quota_core::alerts::{AlertKind, AlertTracker};
use quota_core::cache::SnapshotCache;
use quota_core::format::{countdown, window_name, Lang};
use quota_core::{refresh_all, Ctx, Overview, ProviderReport, Status};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::settings::{Settings, SettingsView};
use crate::tray;

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub reports: Mutex<Vec<ProviderReport>>,
    pub last_refresh: Mutex<Option<DateTime<Utc>>>,
    pub refreshing: AtomicBool,
    tracker: Mutex<AlertTracker>,
    cache: SnapshotCache,
    ctx: tokio::sync::Mutex<Option<Ctx>>,
}

/// Everything the web UI renders, pushed on every change as the `snapshot` event.
#[derive(Serialize, Clone)]
pub struct AppSnapshot {
    pub reports: Vec<ProviderReport>,
    pub overview: Overview,
    pub refreshing: bool,
    pub last_refresh: Option<DateTime<Utc>>,
    pub settings: SettingsView,
    pub platform: &'static str,
}

fn cache_dir() -> std::path::PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("codexusagebar")
}

impl AppState {
    pub fn new() -> Self {
        let settings = Settings::load();
        let cache = SnapshotCache::new(cache_dir());
        // Show the last known numbers immediately; they are marked loading until the first refresh.
        let reports = cache
            .load()
            .into_iter()
            .filter(|(id, _)| !settings.disabled.contains(id))
            .map(|(id, snap)| {
                let mut r = ProviderReport::new(id, Status::Loading);
                r.snapshot = Some(snap);
                r
            })
            .collect();
        Self {
            settings: Mutex::new(settings),
            reports: Mutex::new(reports),
            last_refresh: Mutex::new(None),
            refreshing: AtomicBool::new(false),
            tracker: Mutex::new(AlertTracker::load(cache_dir())),
            cache,
            ctx: tokio::sync::Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let settings = self.settings.lock().unwrap();
        let reports = self.reports.lock().unwrap().clone();
        AppSnapshot {
            overview: Overview::from_reports(&reports, &settings.thresholds),
            reports,
            refreshing: self.refreshing.load(Ordering::SeqCst),
            last_refresh: *self.last_refresh.lock().unwrap(),
            settings: settings.view(),
            platform: std::env::consts::OS,
        }
    }

    /// Drop the cached context so the next refresh picks up new API keys.
    pub async fn reset_ctx(&self) {
        *self.ctx.lock().await = None;
    }
}

pub fn emit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snap = state.snapshot();
    tray::update(app, &snap);
    let _ = app.emit("snapshot", snap);
}

pub async fn refresh(app: AppHandle) {
    let state = app.state::<AppState>();
    if state.refreshing.swap(true, Ordering::SeqCst) {
        return;
    }
    emit(&app);

    let (disabled, thresholds, notify, notify_restored, lang) = {
        let s = state.settings.lock().unwrap();
        (s.disabled.clone(), s.thresholds, s.notify, s.notify_restored, s.lang())
    };
    let ctx = {
        let mut guard = state.ctx.lock().await;
        if guard.is_none() {
            let keys: HashMap<_, _> = state.settings.lock().unwrap().api_keys.clone();
            *guard = Some(Ctx::new(keys).await);
        }
        guard.clone().unwrap()
    };

    let previous = state.cache.load();
    let reports = refresh_all(&ctx, &disabled, &previous).await;
    state.cache.update(&reports);
    let alerts = state.tracker.lock().unwrap().evaluate(&reports, &thresholds);

    *state.reports.lock().unwrap() = reports;
    *state.last_refresh.lock().unwrap() = Some(Utc::now());
    state.refreshing.store(false, Ordering::SeqCst);
    emit(&app);

    if notify {
        for a in alerts {
            if a.kind == AlertKind::Restored && !notify_restored {
                continue;
            }
            let (title, body) = alert_text(lang, a.kind, a.provider.name(), &a.window, thresholds.warning, thresholds.critical);
            let _ = app.notification().builder().title(title).body(body).show();
        }
    }
}

fn alert_text(
    lang: Lang,
    kind: AlertKind,
    provider: &str,
    w: &quota_core::QuotaWindow,
    warn: f64,
    crit: f64,
) -> (String, String) {
    let name = window_name(w, lang);
    let left = w.remaining_percent().round();
    let when = countdown(w.resets_at, Utc::now(), lang);
    match (lang, kind) {
        (Lang::Zh, AlertKind::Warning) => (format!("{provider} 额度低于 {warn:.0}%"), format!("{name}剩余 {left}%，{when}重置")),
        (Lang::Zh, AlertKind::Critical) => (format!("{provider} 额度告急"), format!("{name}仅剩 {left}%（低于 {crit:.0}%），{when}重置")),
        (Lang::Zh, AlertKind::Restored) => (format!("{provider} 额度已恢复"), format!("{name}已重置，当前剩余 {left}%")),
        (Lang::En, AlertKind::Warning) => (format!("{provider} is below {warn:.0}%"), format!("{name}: {left}% left, {when}")),
        (Lang::En, AlertKind::Critical) => (format!("{provider} is almost out"), format!("{name}: {left}% left, {when}")),
        (Lang::En, AlertKind::Restored) => (format!("{provider} quota restored"), format!("{name} has reset: {left}% left")),
    }
}

/// Periodic refresh plus an immediate one after the machine wakes (detected as a wall-clock jump).
pub fn spawn_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        refresh(app.clone()).await;
        let tick = std::time::Duration::from_secs(15);
        let mut last_tick = Utc::now();
        loop {
            tokio::time::sleep(tick).await;
            let now = Utc::now();
            let woke = (now - last_tick).num_seconds() > 90;
            last_tick = now;
            let state = app.state::<AppState>();
            let interval = state.settings.lock().unwrap().refresh_secs as i64;
            let due = state.last_refresh.lock().unwrap().is_none_or(|t| (now - t).num_seconds() >= interval);
            if woke || due {
                refresh(app.clone()).await;
            }
        }
    });
}
