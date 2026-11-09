use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Utc;
use quota_core::format::{countdown_window, short_label, Lang};
use quota_core::icon::{self, IconState, IconStyle};
use quota_core::{Level, Status};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::settings::DisplayMode;
use crate::state::{self, AppSnapshot, AppState};

const TRAY_ID: &str = "main";
const ICON_PX: u32 = 64;

/// When the panel last hid because it lost focus; a tray click right after must not reopen it.
static LAST_BLUR_HIDE: Mutex<Option<Instant>> = Mutex::new(None);
/// When the panel was last shown; focus-outs right after (tray menu closing, WM settling) are ignored.
static LAST_SHOW: Mutex<Option<Instant>> = Mutex::new(None);
/// Set once the tray has reported its on-screen position (never happens on Linux AppIndicator).
static TRAY_POSITION_KNOWN: AtomicBool = AtomicBool::new(false);

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let lang = app.state::<AppState>().settings.lock().unwrap().lang();
    let rgba = icon::render(IconState::Idle, IconStyle::Color, ICON_PX);
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::new_owned(rgba, ICON_PX, ICON_PX))
        .tooltip("CodexUsageBar")
        .menu(&build_menu(app, None, lang)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_panel(app),
            "refresh" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { state::refresh(app).await });
            }
            "settings" => open_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            TRAY_POSITION_KNOWN.store(true, Ordering::Relaxed);
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn tr(lang: Lang, zh: &'static str, en: &'static str) -> &'static str {
    match lang {
        Lang::Zh => zh,
        Lang::En => en,
    }
}

fn build_menu(app: &AppHandle, snap: Option<&AppSnapshot>, lang: Lang) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    if let Some(snap) = snap {
        let now = Utc::now();
        let mut any = false;
        for r in snap.reports.iter().filter(|r| r.visible()) {
            let line = match r.snapshot.as_ref().and_then(|s| s.tightest()) {
                Some(w) => format!(
                    "{}  {} {}{:.0}%  {}",
                    r.name,
                    short_label(w),
                    tr(lang, "剩余 ", ""),
                    w.remaining_percent(),
                    countdown_window(w, now, lang)
                ),
                None => format!("{}  {}", r.name, tr(lang, "需要处理", "needs attention")),
            };
            menu.append(&MenuItem::with_id(app, format!("p:{}", r.id.slug()), line, false, None::<&str>)?)?;
            any = true;
        }
        if any {
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        }
    }
    menu.append(&MenuItem::with_id(app, "open", tr(lang, "打开面板", "Open panel"), true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "refresh", tr(lang, "立即刷新", "Refresh now"), true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "settings", tr(lang, "设置…", "Settings…"), true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", tr(lang, "退出", "Quit"), true, None::<&str>)?)?;
    Ok(menu)
}

fn target_for_filter<'a>(
    snap: &'a AppSnapshot,
    filter: &'a str,
) -> (Option<&'a str>, Option<&'a quota_core::QuotaWindow>, Option<Level>, Option<f64>) {
    let t = &snap.settings.thresholds;
    if filter == "all" {
        return (
            None,
            snap.overview.tightest_window.as_ref(),
            snap.overview.level,
            snap.overview.min_remaining,
        );
    }

    // 1. Direct provider match by slug
    if let Some(r) = snap.reports.iter().find(|r| r.id.slug() == filter) {
        if let Some(s) = r.snapshot.as_ref() {
            if let Some(w) = s.tightest() {
                return (Some(r.name.as_str()), Some(w), Some(w.level(t)), Some(w.remaining_percent()));
            }
        }
        return (Some(r.name.as_str()), None, None, None);
    }

    // 2. Antigravity model / scope match (e.g. "gemini-flash", "argon-4", "model:Gemini 3.8 Flash", etc.)
    if let Some(agy) = snap.reports.iter().find(|r| r.id == quota_core::ProviderId::Agy) {
        if let Some(s) = agy.snapshot.as_ref() {
            if let Some(stripped) = filter.strip_prefix("account:") {
                let (acc, model_part) = match stripped.split_once('#') {
                    Some((a, m)) => (a, Some(m)),
                    None => (stripped, None),
                };
                let short = acc.split('@').next().unwrap_or(acc);
                let kw = match model_part {
                    Some(m) if m.contains("flash") || m.contains("3.8") => Some("3.8 Flash"),
                    Some(m) if m.contains("argon") => Some("Argon"),
                    Some(m) if m.contains("claude") || m.contains("gpt") => Some("Claude"),
                    _ => None,
                };
                let w = if let Some(k) = kw {
                    s.tightest_for_account_and_scope(acc, k)
                } else {
                    s.tightest_for_account(acc)
                };
                return (
                    Some(short),
                    w,
                    w.map(|win| win.level(t)),
                    w.map(|win| win.remaining_percent()),
                );
            }

            let (keyword, display_name) = if filter.contains("flash") || filter.contains("3.8") {
                ("3.8 Flash", "Flash")
            } else if filter.contains("argon") {
                ("Argon", "Argon 4")
            } else if filter.contains("claude") || filter.contains("gpt") {
                ("Claude", "Claude")
            } else if let Some(stripped) = filter.strip_prefix("model:") {
                (stripped, stripped)
            } else {
                ("", "Antigravity")
            };

            let w = if let Some(act) = &snap.active_agy_account {
                s.tightest_for_account_and_scope(act, keyword).or_else(|| s.tightest_for_scope(keyword))
            } else {
                s.tightest_for_scope(keyword)
            };

            if let Some(win) = w {
                return (Some(display_name), Some(win), Some(win.level(t)), Some(win.remaining_percent()));
            }
        }
    }

    // Fallback on overview
    (
        None,
        snap.overview.tightest_window.as_ref(),
        snap.overview.level,
        snap.overview.min_remaining,
    )
}

pub fn update(app: &AppHandle, snap: &AppSnapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let o = &snap.overview;
    let lang = snap.settings.resolved_language;
    let (target_label, target_win, target_level, target_rem) = target_for_filter(snap, &snap.active_filter);

    let all_failed = o.active_count > 0 && o.level.is_none() && !o.any_loading && o.has_issue;
    let state = match (target_level, target_rem) {
        (Some(level), Some(remaining)) => IconState::Gauge { remaining, level, issue: o.has_issue },
        _ if all_failed => IconState::Error,
        _ => IconState::Idle,
    };
    // macOS: a template icon blends into the menu bar while all is well; colour appears only on alerts.
    let template = cfg!(target_os = "macos") && matches!(state, IconState::Gauge { level: Level::Normal, .. } | IconState::Idle);
    let style = if template { IconStyle::Template } else { IconStyle::Color };
    let _ = tray.set_icon(Some(Image::new_owned(icon::render(state, style, ICON_PX), ICON_PX, ICON_PX)));
    let _ = tray.set_icon_as_template(template);

    let now = Utc::now();
    let visible: Vec<_> = snap.reports.iter().filter(|r| r.visible()).collect();
    let title = match snap.settings.display {
        DisplayMode::IconOnly => String::new(),
        DisplayMode::Compact => match (target_label, target_rem) {
            (Some(name), Some(r)) => format!("{name} {r:.0}%"),
            (None, Some(r)) => format!("{r:.0}%"),
            _ => String::new(),
        },
        DisplayMode::Full => match (target_label, target_win, target_rem) {
            (Some(name), Some(w), Some(r)) => format!("{name} {r:.0}% ({})", countdown_window(w, now, lang)),
            _ => visible
                .iter()
                .filter_map(|r| {
                    let w = r.snapshot.as_ref()?.tightest()?;
                    Some(format!("{} {:.0}%", r.name.split(' ').next().unwrap_or(&r.name), w.remaining_percent()))
                })
                .collect::<Vec<_>>()
                .join("  "),
        },
    };
    let _ = tray.set_title(if title.is_empty() { None } else { Some(title) });

    let mut tip = vec!["CodexUsageBar".to_string()];
    for r in &visible {
        match r.snapshot.as_ref().and_then(|s| s.tightest()) {
            Some(w) if r.status != Status::Issue => tip.push(format!(
                "{}: {} {:.0}% ({})",
                r.name,
                short_label(w),
                w.remaining_percent(),
                countdown_window(w, now, lang)
            )),
            _ => tip.push(format!("{}: {}", r.name, tr(lang, "需要处理", "needs attention"))),
        }
    }
    let _ = tray.set_tooltip(Some(tip.join("\n")));
    if let Ok(menu) = build_menu(app, Some(snap), lang) {
        let _ = tray.set_menu(Some(menu));
    }
}

pub fn toggle_panel(app: &AppHandle) {
    let Some(win) = app.get_webview_window("panel") else { return };
    if win.is_visible().unwrap_or(false) {
        let _ = win.hide();
        return;
    }
    let recently_hidden = LAST_BLUR_HIDE.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if !recently_hidden {
        show_panel(app);
    }
}

pub fn show_panel(app: &AppHandle) {
    let Some(win) = app.get_webview_window("panel") else { return };
    // Show before positioning: a hidden GTK window reports a 0x0 size, which breaks the maths.
    *LAST_SHOW.lock().unwrap() = Some(Instant::now());
    let _ = win.show();
    // Hang the panel from the tray icon when we know where it is; otherwise use the screen corner
    // where trays live on this platform (Windows taskbar bottom-right, macOS/Linux top-right).
    let window = win.as_ref().window();
    let placed = TRAY_POSITION_KNOWN.load(Ordering::Relaxed)
        && window
            .move_window_constrained(if cfg!(windows) { Position::TrayBottomCenter } else { Position::TrayCenter })
            .is_ok();
    if !placed {
        place_in_corner(&win);
    }
    let _ = win.set_focus();

    // Data older than a minute is refreshed when the panel opens.
    let state = app.state::<AppState>();
    let stale = state.last_refresh.lock().unwrap().is_none_or(|t| (Utc::now() - t).num_seconds() > 60);
    if stale {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { state::refresh(app).await });
    }
}

/// Top-right (bottom-right on Windows) of the monitor the cursor is on, inset from the edges.
/// Computed from the monitor and the configured panel width rather than the window's own size,
/// which GTK reports as 0x0 until the window is realised.
fn place_in_corner(win: &tauri::WebviewWindow) {
    let monitor = win
        .cursor_position()
        .ok()
        .and_then(|p| win.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| win.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let scale = m.scale_factor();
    let (w, h) = win
        .outer_size()
        .ok()
        .filter(|s| s.width > 0)
        .map(|s| (s.width as f64, s.height as f64))
        .unwrap_or((384.0 * scale, 600.0 * scale));
    let margin = 8.0 * scale;
    let area = m.work_area();
    let x = area.position.x as f64 + area.size.width as f64 - w - margin;
    let y = if cfg!(windows) {
        area.position.y as f64 + area.size.height as f64 - h - margin
    } else {
        area.position.y as f64 + margin
    };
    let _ = win.set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

pub fn on_panel_blur(app: &AppHandle) {
    if LAST_SHOW.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(600)) {
        return;
    }
    if let Some(win) = app.get_webview_window("panel") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
            *LAST_BLUR_HIDE.lock().unwrap() = Some(Instant::now());
        }
    }
}

pub fn open_settings(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        return;
    }
    let lang = app.state::<AppState>().settings.lock().unwrap().lang();
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html#/settings".into()))
        .title(tr(lang, "CodexUsageBar 设置", "CodexUsageBar Settings"))
        .inner_size(780.0, 580.0)
        .min_inner_size(640.0, 480.0)
        .center()
        .build();
}
