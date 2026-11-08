//! Short human strings used outside the web UI: tray title/tooltip/menu, notifications, CLI.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{QuotaWindow, WindowKind};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Zh,
    En,
}

impl Lang {
    pub fn from_locale(locale: &str) -> Self {
        if locale.to_lowercase().starts_with("zh") {
            Lang::Zh
        } else {
            Lang::En
        }
    }
}

/// Compact label for the menu bar: "5h", "7d", "Opus 7d", "Mo".
pub fn short_label(w: &QuotaWindow) -> String {
    let base = match (w.kind, w.window_mins) {
        (_, Some(10080)) => "7d".into(),
        (_, Some(1440)) => "24h".into(),
        (_, Some(m)) if m > 0 && m % 10080 == 0 => format!("{}w", m / 10080),
        (_, Some(m)) if m > 0 && m % 1440 == 0 => format!("{}d", m / 1440),
        (_, Some(m)) if m > 0 && m % 60 == 0 => format!("{}h", m / 60),
        (_, Some(m)) if m > 0 => format!("{m}m"),
        (WindowKind::Monthly, _) => "Mo".into(),
        (WindowKind::Weekly, _) => "7d".into(),
        (WindowKind::Daily, _) => "24h".into(),
        _ => "".into(),
    };
    match &w.scope {
        Some(s) if base.is_empty() => s.clone(),
        Some(s) => format!("{s} {base}"),
        None => base,
    }
}

/// Descriptive name: "5 小时额度", "每周额度 · Opus".
pub fn window_name(w: &QuotaWindow, lang: Lang) -> String {
    let base = match (lang, w.window_mins, w.kind) {
        (Lang::Zh, Some(300), _) => "5 小时额度".to_string(),
        (Lang::En, Some(300), _) => "5-hour limit".to_string(),
        (Lang::Zh, _, WindowKind::Weekly) => "每周额度".into(),
        (Lang::En, _, WindowKind::Weekly) => "Weekly limit".into(),
        (Lang::Zh, _, WindowKind::Daily) => "每日额度".into(),
        (Lang::En, _, WindowKind::Daily) => "Daily limit".into(),
        (Lang::Zh, _, WindowKind::Monthly) => "每月额度".into(),
        (Lang::En, _, WindowKind::Monthly) => "Monthly limit".into(),
        (Lang::Zh, Some(m), _) if m % 60 == 0 => format!("{} 小时额度", m / 60),
        (Lang::En, Some(m), _) if m % 60 == 0 => format!("{}-hour limit", m / 60),
        (Lang::Zh, _, _) => "额度窗口".into(),
        (Lang::En, _, _) => "Usage limit".into(),
    };
    match &w.scope {
        Some(s) => format!("{base} · {s}"),
        None => base,
    }
}

pub fn countdown(resets_at: Option<DateTime<Utc>>, now: DateTime<Utc>, lang: Lang) -> String {
    let Some(at) = resets_at else {
        return match lang {
            Lang::Zh => "重置时间未知".into(),
            Lang::En => "reset time unknown".into(),
        };
    };
    let secs = (at - now).num_seconds();
    if secs <= 0 {
        return match lang {
            Lang::Zh => "已重置".into(),
            Lang::En => "reset".into(),
        };
    }
    let (d, h, m) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60);
    match lang {
        Lang::Zh => match (d, h, m) {
            (0, 0, 0) => "即将重置".into(),
            (0, 0, m) => format!("还有 {m} 分钟"),
            (0, h, 0) => format!("还有 {h} 小时"),
            (0, h, m) => format!("还有 {h} 小时 {m} 分钟"),
            (d, 0, _) => format!("还有 {d} 天"),
            (d, h, _) => format!("还有 {d} 天 {h} 小时"),
        },
        Lang::En => match (d, h, m) {
            (0, 0, 0) => "resets in <1m".into(),
            (0, 0, m) => format!("resets in {m}m"),
            (0, h, m) => format!("resets in {h}h {m}m"),
            (d, h, _) => format!("resets in {d}d {h}h"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn countdowns() {
        let now = Utc::now();
        let at = |s: i64| Some(now + Duration::seconds(s));
        assert_eq!(countdown(at(3 * 3600 + 25 * 60), now, Lang::Zh), "还有 3 小时 25 分钟");
        assert_eq!(countdown(at(2 * 86400 + 5 * 3600), now, Lang::Zh), "还有 2 天 5 小时");
        assert_eq!(countdown(at(2 * 86400 + 30), now, Lang::Zh), "还有 2 天");
        assert_eq!(countdown(at(30), now, Lang::Zh), "即将重置");
        assert_eq!(countdown(at(-5), now, Lang::Zh), "已重置");
        assert_eq!(countdown(None, now, Lang::Zh), "重置时间未知");
        assert_eq!(countdown(at(90 * 60), now, Lang::En), "resets in 1h 30m");
    }

    #[test]
    fn labels() {
        assert_eq!(short_label(&QuotaWindow::new("a", Some(300), 0.0)), "5h");
        assert_eq!(short_label(&QuotaWindow::new("a", Some(10080), 0.0)), "7d");
        assert_eq!(short_label(&QuotaWindow::new("a", Some(1440), 0.0)), "24h");
        assert_eq!(short_label(&QuotaWindow::new("a", Some(10080), 0.0).scope("Opus")), "Opus 7d");
        assert_eq!(short_label(&QuotaWindow::new("a", None, 0.0).kind(WindowKind::Monthly)), "Mo");
        assert_eq!(window_name(&QuotaWindow::new("a", Some(10080), 0.0).scope("Opus"), Lang::Zh), "每周额度 · Opus");
    }
}
