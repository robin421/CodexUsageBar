use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use quota_core::format::Lang;
use quota_core::{ProviderId, Thresholds};
use serde::{Deserialize, Serialize};

/// What the menu bar shows next to the icon (macOS and Linux; Windows trays have no text).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    /// Every provider's tightest window: "Claude 70% Codex 64%".
    Full,
    /// Only the tightest window overall: "70%".
    Compact,
    #[default]
    IconOnly,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub refresh_secs: u64,
    pub display: DisplayMode,
    pub thresholds: Thresholds,
    pub notify: bool,
    pub notify_restored: bool,
    pub disabled: HashSet<ProviderId>,
    pub api_keys: HashMap<ProviderId, String>,
    /// "auto", "zh" or "en".
    pub language: String,
    /// "system", "light" or "dark".
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_secs: 300,
            display: DisplayMode::IconOnly,
            thresholds: Thresholds::default(),
            notify: true,
            notify_restored: true,
            disabled: HashSet::new(),
            api_keys: HashMap::new(),
            language: "auto".into(),
            theme: "system".into(),
        }
    }
}

impl Settings {
    pub fn dir() -> PathBuf {
        dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("codexusagebar")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::dir().join("config.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = Self::dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, text);
            // API keys live in this file; keep it private to the user.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    pub fn lang(&self) -> Lang {
        match self.language.as_str() {
            "zh" => Lang::Zh,
            "en" => Lang::En,
            _ => Lang::from_locale(&sys_locale::get_locale().unwrap_or_default()),
        }
    }

    pub fn view(&self) -> SettingsView {
        SettingsView {
            refresh_secs: self.refresh_secs,
            display: self.display,
            thresholds: self.thresholds,
            notify: self.notify,
            notify_restored: self.notify_restored,
            disabled: self.disabled.iter().copied().collect(),
            api_keys: self.api_keys.iter().map(|(k, v)| (*k, mask(v))).collect(),
            language: self.language.clone(),
            resolved_language: self.lang(),
            theme: self.theme.clone(),
            config_path: Self::dir().join("config.json").display().to_string(),
        }
    }
}

/// Settings as sent to the web UI: API keys are masked, never returned in full.
#[derive(Serialize, Clone, Debug)]
pub struct SettingsView {
    pub refresh_secs: u64,
    pub display: DisplayMode,
    pub thresholds: Thresholds,
    pub notify: bool,
    pub notify_restored: bool,
    pub disabled: Vec<ProviderId>,
    pub api_keys: BTreeMap<ProviderId, String>,
    pub language: String,
    pub resolved_language: Lang,
    pub theme: String,
    pub config_path: String,
}

#[derive(Deserialize, Debug, Default)]
pub struct SettingsPatch {
    pub refresh_secs: Option<u64>,
    pub display: Option<DisplayMode>,
    pub thresholds: Option<Thresholds>,
    pub notify: Option<bool>,
    pub notify_restored: Option<bool>,
    pub disabled: Option<Vec<ProviderId>>,
    pub language: Option<String>,
    pub theme: Option<String>,
}

impl Settings {
    pub fn apply(&mut self, p: SettingsPatch) {
        if let Some(v) = p.refresh_secs {
            self.refresh_secs = v.clamp(60, 3600);
        }
        if let Some(v) = p.display {
            self.display = v;
        }
        if let Some(mut t) = p.thresholds {
            t.critical = t.critical.clamp(1.0, 50.0);
            t.warning = t.warning.clamp(t.critical + 1.0, 80.0);
            self.thresholds = t;
        }
        if let Some(v) = p.notify {
            self.notify = v;
        }
        if let Some(v) = p.notify_restored {
            self.notify_restored = v;
        }
        if let Some(v) = p.disabled {
            self.disabled = v.into_iter().collect();
        }
        if let Some(v) = p.language {
            self.language = v;
        }
        if let Some(v) = p.theme {
            self.theme = v;
        }
    }
}

fn mask(key: &str) -> String {
    let k = key.trim();
    let n = k.chars().count();
    if n <= 8 {
        "••••".into()
    } else {
        let tail: String = k.chars().skip(n - 4).collect();
        format!("{}••••{}", k.chars().take(3).collect::<String>(), tail)
    }
}
