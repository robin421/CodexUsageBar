use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderId {
    Codex,
    Claude,
    Gemini,
    Copilot,
    Opencode,
    Cursor,
    Jetbrains,
    Codebuff,
    Openrouter,
    Deepseek,
    Moonshot,
    Zai,
    Minimax,
}

/// How a provider authenticates: reusing a local CLI/app login, or an API key the user supplies.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    Local,
    ApiKey,
}

impl ProviderId {
    pub const ALL: [ProviderId; 13] = [
        ProviderId::Codex,
        ProviderId::Claude,
        ProviderId::Gemini,
        ProviderId::Copilot,
        ProviderId::Opencode,
        ProviderId::Cursor,
        ProviderId::Jetbrains,
        ProviderId::Codebuff,
        ProviderId::Openrouter,
        ProviderId::Deepseek,
        ProviderId::Moonshot,
        ProviderId::Zai,
        ProviderId::Minimax,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            ProviderId::Codex => "codex",
            ProviderId::Claude => "claude",
            ProviderId::Gemini => "gemini",
            ProviderId::Copilot => "copilot",
            ProviderId::Opencode => "opencode",
            ProviderId::Cursor => "cursor",
            ProviderId::Jetbrains => "jetbrains",
            ProviderId::Codebuff => "codebuff",
            ProviderId::Openrouter => "openrouter",
            ProviderId::Deepseek => "deepseek",
            ProviderId::Moonshot => "moonshot",
            ProviderId::Zai => "zai",
            ProviderId::Minimax => "minimax",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.slug() == s)
    }

    pub fn name(self) -> &'static str {
        match self {
            ProviderId::Codex => "Codex",
            ProviderId::Claude => "Claude Code",
            ProviderId::Gemini => "Gemini CLI",
            ProviderId::Copilot => "GitHub Copilot",
            ProviderId::Opencode => "OpenCode Go",
            ProviderId::Cursor => "Cursor",
            ProviderId::Jetbrains => "JetBrains AI",
            ProviderId::Codebuff => "Codebuff",
            ProviderId::Openrouter => "OpenRouter",
            ProviderId::Deepseek => "DeepSeek",
            ProviderId::Moonshot => "Moonshot / Kimi",
            ProviderId::Zai => "Z.ai / GLM",
            ProviderId::Minimax => "MiniMax",
        }
    }

    pub fn auth(self) -> AuthKind {
        match self {
            ProviderId::Openrouter
            | ProviderId::Deepseek
            | ProviderId::Moonshot
            | ProviderId::Zai
            | ProviderId::Minimax => AuthKind::ApiKey,
            _ => AuthKind::Local,
        }
    }

    /// Providers whose upstream endpoints are undocumented and shift more often.
    pub fn beta(self) -> bool {
        matches!(
            self,
            ProviderId::Cursor | ProviderId::Jetbrains | ProviderId::Codebuff | ProviderId::Opencode
        )
    }

    /// Command the user runs to (re)authenticate the underlying tool.
    pub fn login_command(self) -> Option<&'static str> {
        match self {
            ProviderId::Codex => Some("codex login"),
            ProviderId::Claude => Some("claude"),
            ProviderId::Gemini => Some("gemini"),
            ProviderId::Copilot => Some("gh auth login"),
            ProviderId::Opencode => Some("opencode auth login"),
            ProviderId::Codebuff => Some("codebuff login"),
            _ => None,
        }
    }

    /// Environment variables checked for an API key when none is configured.
    pub fn env_keys(self) -> &'static [&'static str] {
        match self {
            ProviderId::Openrouter => &["OPENROUTER_API_KEY"],
            ProviderId::Deepseek => &["DEEPSEEK_API_KEY"],
            ProviderId::Moonshot => &["MOONSHOT_API_KEY", "KIMI_API_KEY"],
            ProviderId::Zai => &["Z_AI_API_KEY", "ZAI_API_KEY", "ZHIPUAI_API_KEY", "GLM_API_KEY"],
            ProviderId::Minimax => &["MINIMAX_API_KEY"],
            ProviderId::Codebuff => &["CODEBUFF_API_KEY"],
            ProviderId::Opencode => &["OPENCODE_API_KEY"],
            _ => &[],
        }
    }

    pub fn dashboard_url(self) -> &'static str {
        match self {
            ProviderId::Codex => "https://chatgpt.com/codex/settings/usage",
            ProviderId::Claude => "https://claude.ai/settings/usage",
            ProviderId::Gemini => "https://aistudio.google.com/",
            ProviderId::Copilot => "https://github.com/settings/copilot",
            ProviderId::Opencode => "https://opencode.ai/auth",
            ProviderId::Cursor => "https://cursor.com/dashboard?tab=usage",
            ProviderId::Jetbrains => "https://account.jetbrains.com/licenses",
            ProviderId::Codebuff => "https://www.codebuff.com/usage",
            ProviderId::Openrouter => "https://openrouter.ai/settings/credits",
            ProviderId::Deepseek => "https://platform.deepseek.com/usage",
            ProviderId::Moonshot => "https://platform.moonshot.ai/console/account",
            ProviderId::Zai => "https://z.ai/manage-apikey/coding-plan/personal/my-plan",
            ProviderId::Minimax => "https://platform.minimax.io/user-center/payment/coding-plan",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    Session,
    Daily,
    Weekly,
    Monthly,
    Other,
}

impl WindowKind {
    pub fn from_minutes(mins: Option<u32>) -> Self {
        match mins {
            Some(m) if m <= 12 * 60 => WindowKind::Session,
            Some(m) if m <= 36 * 60 => WindowKind::Daily,
            Some(m) if m <= 8 * 24 * 60 => WindowKind::Weekly,
            Some(m) if m <= 32 * 24 * 60 => WindowKind::Monthly,
            _ => WindowKind::Other,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct QuotaWindow {
    pub id: String,
    pub kind: WindowKind,
    pub window_mins: Option<u32>,
    /// Model or feature the window is scoped to ("Opus", "gemini-2.5-pro", "Premium requests").
    pub scope: Option<String>,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    /// Unit of `used` / `limit` ("requests", "USD", "credits").
    pub unit: Option<String>,
}

impl QuotaWindow {
    pub fn new(id: impl Into<String>, window_mins: Option<u32>, used_percent: f64) -> Self {
        Self {
            id: id.into(),
            kind: WindowKind::from_minutes(window_mins),
            window_mins,
            scope: None,
            used_percent: clamp_pct(used_percent),
            resets_at: None,
            used: None,
            limit: None,
            unit: None,
        }
    }

    pub fn kind(mut self, kind: WindowKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    pub fn resets_at(mut self, at: Option<DateTime<Utc>>) -> Self {
        self.resets_at = at;
        self
    }

    pub fn amounts(mut self, used: f64, limit: f64, unit: impl Into<String>) -> Self {
        self.used = Some(used);
        self.limit = Some(limit);
        self.unit = Some(unit.into());
        self
    }

    pub fn remaining_percent(&self) -> f64 {
        clamp_pct(100.0 - self.used_percent)
    }

    pub fn level(&self, t: &Thresholds) -> Level {
        let rem = self.remaining_percent();
        if rem < t.critical {
            Level::Critical
        } else if rem < t.warning {
            Level::Warning
        } else {
            Level::Normal
        }
    }
}

pub fn clamp_pct(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 100.0)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Balance {
    pub id: String,
    pub amount: f64,
    pub currency: String,
    /// Optional total the balance counts down from (prepaid credits, spend cap).
    pub total: Option<f64>,
    #[serde(default)]
    pub unlimited: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Snapshot {
    pub provider: ProviderId,
    pub account: Option<String>,
    pub plan: Option<String>,
    pub windows: Vec<QuotaWindow>,
    pub balances: Vec<Balance>,
    /// Which data path produced this snapshot ("oauth", "app-server", "api", "local").
    pub source: String,
    pub updated_at: DateTime<Utc>,
}

impl Snapshot {
    pub fn new(provider: ProviderId, source: &str) -> Self {
        Self {
            provider,
            account: None,
            plan: None,
            windows: Vec::new(),
            balances: Vec::new(),
            source: source.to_string(),
            updated_at: Utc::now(),
        }
    }

    /// The window with the least headroom left.
    pub fn tightest(&self) -> Option<&QuotaWindow> {
        self.windows
            .iter()
            .min_by(|a, b| a.remaining_percent().total_cmp(&b.remaining_percent()))
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Normal,
    Warning,
    Critical,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Thresholds {
    /// Remaining percent below which a window is a warning.
    pub warning: f64,
    /// Remaining percent below which a window is critical.
    pub critical: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self { warning: 20.0, critical: 10.0 }
    }
}

/// Why a provider has no fresh data.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Issue {
    NotDetected,
    NotInstalled,
    NotLoggedIn { command: Option<String> },
    AuthExpired { command: Option<String> },
    MissingKey,
    Unsupported { reason: String },
    Error { message: String },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    /// Last good snapshot is shown, but the latest refresh failed.
    Stale,
    Issue,
    Loading,
    Disabled,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ProviderReport {
    pub id: ProviderId,
    pub name: String,
    pub auth: AuthKind,
    pub beta: bool,
    pub status: Status,
    pub snapshot: Option<Snapshot>,
    pub issue: Option<Issue>,
    pub dashboard_url: String,
    pub checked_at: DateTime<Utc>,
}

impl ProviderReport {
    pub fn new(id: ProviderId, status: Status) -> Self {
        Self {
            id,
            name: id.name().to_string(),
            auth: id.auth(),
            beta: id.beta(),
            status,
            snapshot: None,
            issue: None,
            dashboard_url: id.dashboard_url().to_string(),
            checked_at: Utc::now(),
        }
    }

    pub fn level(&self, t: &Thresholds) -> Option<Level> {
        let snap = self.snapshot.as_ref()?;
        snap.windows.iter().map(|w| w.level(t)).max()
    }

    /// Whether this provider should appear in the main panel.
    pub fn visible(&self) -> bool {
        let hidden_issue = match &self.issue {
            Some(Issue::NotDetected | Issue::MissingKey) => true,
            Some(Issue::Unsupported { reason }) => reason == "no_subscription",
            _ => false,
        };
        !matches!(self.status, Status::Disabled) && !hidden_issue
    }
}

/// Aggregate used by the tray icon: the single most constrained window across providers.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Overview {
    pub level: Option<Level>,
    pub min_remaining: Option<f64>,
    pub tightest_provider: Option<ProviderId>,
    pub tightest_window: Option<QuotaWindow>,
    pub has_issue: bool,
    pub any_loading: bool,
    pub active_count: usize,
}

impl Overview {
    pub fn from_reports(reports: &[ProviderReport], t: &Thresholds) -> Self {
        let mut best: Option<(ProviderId, &QuotaWindow)> = None;
        let mut has_issue = false;
        let mut any_loading = false;
        let mut active_count = 0;
        for r in reports.iter().filter(|r| r.visible()) {
            active_count += 1;
            match r.status {
                Status::Stale | Status::Issue => has_issue = true,
                Status::Loading => any_loading = true,
                _ => {}
            }
            if let Some(w) = r.snapshot.as_ref().and_then(|s| s.tightest()) {
                if best.is_none_or(|(_, b)| w.remaining_percent() < b.remaining_percent()) {
                    best = Some((r.id, w));
                }
            }
        }
        Self {
            level: best.map(|(_, w)| w.level(t)),
            min_remaining: best.map(|(_, w)| w.remaining_percent()),
            tightest_provider: best.map(|(p, _)| p),
            tightest_window: best.map(|(_, w)| w.clone()),
            has_issue,
            any_loading,
            active_count,
        }
    }
}
