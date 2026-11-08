use crate::model::{Issue, ProviderId};

#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum FetchError {
    /// No local login or key was found; the provider is simply not used on this machine.
    #[error("not detected")]
    NotDetected,
    #[error("CLI not installed")]
    NotInstalled,
    #[error("not logged in")]
    NotLoggedIn,
    #[error("credentials expired")]
    AuthExpired,
    #[error("API key required")]
    MissingKey,
    #[error("{0}")]
    Unsupported(String),
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("{0}")]
    Network(String),
    #[error("unexpected response: {0}")]
    Parse(String),
}

impl FetchError {
    pub fn parse(msg: impl Into<String>) -> Self {
        FetchError::Parse(msg.into())
    }

    /// Auth problems may be solved by another data source; transport problems will not.
    pub fn is_auth(&self) -> bool {
        matches!(self, FetchError::NotLoggedIn | FetchError::AuthExpired | FetchError::NotDetected)
            || matches!(self, FetchError::Http { status, .. } if *status == 401 || *status == 403)
    }

    pub fn into_issue(self, id: ProviderId) -> Issue {
        let command = id.login_command().map(str::to_string);
        match self {
            FetchError::NotDetected => Issue::NotDetected,
            FetchError::NotInstalled => Issue::NotInstalled,
            FetchError::NotLoggedIn => Issue::NotLoggedIn { command },
            FetchError::AuthExpired => Issue::AuthExpired { command },
            FetchError::MissingKey => Issue::MissingKey,
            FetchError::Unsupported(reason) => Issue::Unsupported { reason },
            FetchError::Http { status: 401 | 403, .. } => match id.auth() {
                crate::model::AuthKind::ApiKey => Issue::Error { message: "API key rejected (HTTP 401/403)".into() },
                crate::model::AuthKind::Local => Issue::AuthExpired { command },
            },
            other => Issue::Error { message: other.to_string() },
        }
    }
}

impl From<reqwest::Error> for FetchError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            FetchError::Network("request timed out".into())
        } else if e.is_connect() {
            FetchError::Network("could not connect".into())
        } else if e.is_decode() {
            FetchError::Parse(e.to_string())
        } else {
            FetchError::Network(e.without_url().to_string())
        }
    }
}

impl From<serde_json::Error> for FetchError {
    fn from(e: serde_json::Error) -> Self {
        FetchError::Parse(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, FetchError>;
