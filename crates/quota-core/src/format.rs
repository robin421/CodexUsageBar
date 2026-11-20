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
    Ja,
    Ko,
}

impl Lang {
    /// `zh-Hans`, `ja_JP`, `ko-KR`, and the same tags with a trailing encoding.
    pub fn from_locale(locale: &str) -> Self {
        let primary = locale
            .to_lowercase()
            .split(['-', '_', '.'])
            .next()
            .unwrap_or("")
            .to_string();
        match primary.as_str() {
            "zh" => Lang::Zh,
            "ja" => Lang::Ja,
            "ko" => Lang::Ko,
            _ => Lang::En,
        }
    }
}

fn phrase(lang: Lang, zh: &str, en: &str, ja: &str, ko: &str) -> String {
    match lang {
        Lang::Zh => zh.to_string(),
        Lang::En => en.to_string(),
        Lang::Ja => ja.to_string(),
        Lang::Ko => ko.to_string(),
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
    let base = if w.window_mins == Some(300) {
        phrase(lang, "5 小时额度", "5-hour limit", "5時間枠", "5시간 한도")
    } else if w.kind == WindowKind::Weekly {
        phrase(lang, "每周额度", "Weekly limit", "週間枠", "주간 한도")
    } else if w.kind == WindowKind::Daily {
        phrase(lang, "每日额度", "Daily limit", "1日枠", "일일 한도")
    } else if w.kind == WindowKind::Monthly {
        phrase(lang, "每月额度", "Monthly limit", "月間枠", "월간 한도")
    } else if let Some(m) = w.window_mins.filter(|m| m % 60 == 0) {
        let h = m / 60;
        match lang {
            Lang::Zh => format!("{h} 小时额度"),
            Lang::En => format!("{h}-hour limit"),
            Lang::Ja => format!("{h}時間枠"),
            Lang::Ko => format!("{h}시간 한도"),
        }
    } else {
        phrase(lang, "额度窗口", "Usage limit", "利用枠", "이용 한도")
    };
    match &w.scope {
        Some(s) => format!("{base} · {s}"),
        None => base,
    }
}

pub fn countdown_window(w: &QuotaWindow, now: DateTime<Utc>, lang: Lang) -> String {
    if let Some(at) = w.resets_at {
        return countdown(Some(at), now, lang);
    }
    let rem = w.remaining_percent();
    if rem <= 0.0 {
        return phrase(lang, "当前账号无配额", "no quota assigned", "このアカウントに枠がありません", "이 계정에 할당된 한도가 없습니다");
    }
    if rem >= 100.0 {
        if w.window_mins == Some(300) || w.kind == WindowKind::Session {
            return phrase(
                lang,
                "额度充足（使用后开始计时）",
                "full quota (starts on use)",
                "枠は満杯です（使い始めると計測）",
                "한도 여유 있음 (사용 시 집계 시작)",
            );
        }
        return phrase(lang, "额度充足", "full quota", "枠は満杯です", "한도 여유 있음");
    }
    if w.kind == WindowKind::Monthly {
        return phrase(lang, "随订阅周期生效", "active subscription", "サブスクリプション周期で更新", "구독 주기에 따라 갱신");
    }
    phrase(lang, "官方未提供倒计时", "no countdown provided", "公式のカウントダウンなし", "공식 카운트다운 없음")
}

pub fn countdown(resets_at: Option<DateTime<Utc>>, now: DateTime<Utc>, lang: Lang) -> String {
    let Some(at) = resets_at else {
        return phrase(lang, "随周期生效", "active", "周期に従う", "주기에 따름");
    };
    let secs = (at - now).num_seconds();
    if secs <= 0 {
        return phrase(lang, "已重置", "reset", "リセット済み", "초기화됨");
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
        Lang::Ja => match (d, h, m) {
            (0, 0, 0) => "まもなくリセット".into(),
            (0, 0, m) => format!("あと{m}分"),
            (0, h, 0) => format!("あと{h}時間"),
            (0, h, m) => format!("あと{h}時間{m}分"),
            (d, 0, _) => format!("あと{d}日"),
            (d, h, _) => format!("あと{d}日{h}時間"),
        },
        Lang::Ko => match (d, h, m) {
            (0, 0, 0) => "곧 초기화".into(),
            (0, 0, m) => format!("{m}분 남음"),
            (0, h, 0) => format!("{h}시간 남음"),
            (0, h, m) => format!("{h}시간 {m}분 남음"),
            (d, 0, _) => format!("{d}일 남음"),
            (d, h, _) => format!("{d}일 {h}시간 남음"),
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
        assert_eq!(countdown(None, now, Lang::Zh), "随周期生效");
        assert_eq!(countdown(at(90 * 60), now, Lang::En), "resets in 1h 30m");
        assert_eq!(countdown(at(90 * 60), now, Lang::Ja), "あと1時間30分");
        assert_eq!(countdown(at(2 * 86400 + 5 * 3600), now, Lang::Ko), "2일 5시간 남음");
        assert_eq!(Lang::from_locale("ja-JP"), Lang::Ja);
        assert_eq!(Lang::from_locale("ko_KR.UTF-8"), Lang::Ko);
        assert_eq!(Lang::from_locale("zh-Hans-CN"), Lang::Zh);
        assert_eq!(Lang::from_locale("en-US"), Lang::En);

        let w_session_full = QuotaWindow::new("test", Some(300), 0.0);
        assert_eq!(countdown_window(&w_session_full, now, Lang::Zh), "额度充足（使用后开始计时）");

        let w_no_quota = QuotaWindow::new("test", Some(10080), 100.0);
        assert_eq!(countdown_window(&w_no_quota, now, Lang::Zh), "当前账号无配额");

        let w_monthly = QuotaWindow::new("test", None, 10.0).kind(WindowKind::Monthly);
        assert_eq!(countdown_window(&w_monthly, now, Lang::Zh), "随订阅周期生效");
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
