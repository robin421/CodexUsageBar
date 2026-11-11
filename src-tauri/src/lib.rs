mod settings;
mod state;
mod tray;

use quota_core::{Ctx, ProviderId};
use serde::Serialize;
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

use settings::SettingsPatch;
use state::{AppSnapshot, AppState};

#[tauri::command]
fn get_snapshot(state: tauri::State<AppState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
fn set_active_filter(app: AppHandle, filter: String) -> AppSnapshot {
    let state = app.state::<AppState>();
    {
        let mut f = state.active_filter.lock().unwrap();
        *f = filter;
    }
    state::emit(&app);
    state.snapshot()
}

#[tauri::command]
async fn switch_agy_account(app: AppHandle, email: String) -> AppSnapshot {
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".gemini/google_accounts.json");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let existing = std::fs::read_to_string(&path).ok();
        let payload = quota_core::providers::agy::update_accounts_payload(existing.as_deref(), &email);
        let _ = std::fs::write(path, serde_json::to_string_pretty(&payload).unwrap_or_default());
    }
    state::refresh(app.clone()).await;
    app.state::<AppState>().snapshot()
}

#[tauri::command]
async fn refresh(app: AppHandle) -> AppSnapshot {
    state::refresh(app.clone()).await;
    app.state::<AppState>().snapshot()
}

#[tauri::command]
fn update_settings(app: AppHandle, patch: SettingsPatch) -> AppSnapshot {
    let state = app.state::<AppState>();
    let refetch = patch.disabled.is_some();
    {
        let mut s = state.settings.lock().unwrap();
        s.apply(patch);
        s.save();
    }
    state::emit(&app);
    if refetch {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { state::refresh(app).await });
    }
    state.snapshot()
}

#[tauri::command]
async fn set_api_key(app: AppHandle, provider: ProviderId, key: Option<String>) -> AppSnapshot {
    {
        let state = app.state::<AppState>();
        let mut s = state.settings.lock().unwrap();
        match key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
            Some(k) => {
                s.api_keys.insert(provider, k);
            }
            None => {
                s.api_keys.remove(&provider);
            }
        }
        s.save();
    }
    app.state::<AppState>().reset_ctx().await;
    state::refresh(app.clone()).await;
    app.state::<AppState>().snapshot()
}

#[derive(Serialize)]
struct DiagnosticEntry {
    label: String,
    value: Option<String>,
}

#[tauri::command]
async fn diagnostics() -> Vec<DiagnosticEntry> {
    let ctx = Ctx::new(Default::default()).await;
    let mut out: Vec<DiagnosticEntry> = ["codex", "claude", "agy", "grok", "gh", "opencode", "codebuff"]
        .iter()
        .map(|bin| DiagnosticEntry { label: format!("{bin} CLI"), value: ctx.find_bin(bin).map(|p| p.display().to_string()) })
        .collect();
    out.push(DiagnosticEntry { label: "config".into(), value: Some(settings::Settings::dir().display().to_string()) });
    out.push(DiagnosticEntry {
        label: "cache".into(),
        value: dirs::cache_dir().map(|d| d.join("codexusagebar").display().to_string()),
    });
    out
}

#[tauri::command]
fn test_notification(app: AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    let zh = app.state::<AppState>().settings.lock().unwrap().lang() == quota_core::format::Lang::Zh;
    let (title, body) = if zh {
        ("CodexUsageBar 测试通知", "额度不足时会以这种方式提醒你。")
    } else {
        ("CodexUsageBar test notification", "This is how low-quota alerts will look.")
    };
    let _ = app.notification().builder().title(title).body(body).show();
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    tray::open_settings(&app);
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    if let Some(w) = app.get_webview_window("panel") {
        let _ = w.hide();
    }
}

/// Sizes the panel to its content. GTK only honours size changes on resizable windows, so on
/// Linux the (undecorated, hence not user-resizable) panel stays resizable.
/// On macOS, set_size works directly on non-resizable windows without toggling resizable flags,
/// which avoids window shadow and stylemask redraw flickering.
#[tauri::command]
fn fit_panel(app: AppHandle, height: f64) {
    let Some(w) = app.get_webview_window("panel") else { return };
    let target_height = height.clamp(200.0, 640.0).round();

    if let Ok(cur_size) = w.inner_size() {
        let scale = w.scale_factor().unwrap_or(1.0);
        let cur_logical_h = (cur_size.height as f64 / scale).round();
        if (cur_logical_h - target_height).abs() < 2.0 {
            return;
        }
    }

    let size = tauri::LogicalSize::new(384.0, target_height);
    #[cfg(target_os = "linux")]
    {
        let _ = w.set_size(size);
    }
    #[cfg(target_os = "macos")]
    {
        let _ = w.set_size(size);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = w.set_resizable(true);
        let _ = w.set_size(size);
        let _ = w.set_resizable(false);
    }
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| tray::show_panel(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            set_active_filter,
            switch_agy_account,
            refresh,
            update_settings,
            set_api_key,
            diagnostics,
            test_notification,
            open_settings,
            hide_panel,
            fit_panel,
            quit
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::create(app.handle())?;
            if let Some(panel) = app.get_webview_window("panel") {
                // Created resizable so GTK lets fit_panel shrink it; the undecorated panel has no
                // edges to drag on Linux, and macOS/Windows lock it here.
                #[cfg(not(target_os = "linux"))]
                let _ = panel.set_resizable(false);
                let handle = app.handle().clone();
                panel.on_window_event(move |e| {
                    if let WindowEvent::Focused(false) = e {
                        tray::on_panel_blur(&handle);
                    }
                });
            }
            state::spawn_scheduler(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building CodexUsageBar");

    app.run(|_, event| {
        // Closing the settings window must not quit the tray app.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}
