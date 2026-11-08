use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::error::{FetchError, Result};
use crate::model::ProviderId;

/// Everything a provider needs to run: HTTP client, home directory, environment and user-supplied keys.
/// Tests build one pointing at a temp home so no real credentials are touched.
#[derive(Clone)]
pub struct Ctx {
    pub http: reqwest::Client,
    pub home: PathBuf,
    pub env: HashMap<String, String>,
    pub api_keys: HashMap<ProviderId, String>,
    /// PATH used to locate CLIs, including the user's login-shell PATH (GUI apps start with a minimal one).
    pub search_path: OsString,
}

pub const USER_AGENT: &str = concat!("CodexUsageBar/", env!("CARGO_PKG_VERSION"));

impl Ctx {
    pub async fn new(api_keys: HashMap<ProviderId, String>) -> Self {
        let env: HashMap<String, String> = std::env::vars().collect();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let search_path = build_search_path(&env, &home).await;
        Self::with(home, env, api_keys, search_path)
    }

    pub fn with(
        home: PathBuf,
        env: HashMap<String, String>,
        api_keys: HashMap<ProviderId, String>,
        search_path: OsString,
    ) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .build()
            .expect("http client");
        Self { http, home, env, api_keys, search_path }
    }

    pub fn env(&self, key: &str) -> Option<&str> {
        self.env.get(key).map(|s| s.trim()).filter(|s| !s.is_empty())
    }

    /// Configured API key, falling back to the provider's conventional environment variables.
    pub fn api_key(&self, id: ProviderId) -> Option<String> {
        if let Some(k) = self.api_keys.get(&id).map(|s| s.trim()).filter(|s| !s.is_empty()) {
            return Some(k.to_string());
        }
        id.env_keys().iter().find_map(|k| self.env(k).map(str::to_string))
    }

    /// Per-OS application config root (`~/.config`, `~/Library/Application Support`, `%APPDATA%`).
    pub fn app_config_dir(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home.join("Library/Application Support")
        } else if cfg!(windows) {
            self.env("APPDATA").map(PathBuf::from).unwrap_or_else(|| self.home.join("AppData/Roaming"))
        } else {
            self.xdg_config()
        }
    }

    pub fn xdg_config(&self) -> PathBuf {
        self.env("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| self.home.join(".config"))
    }

    pub fn xdg_data(&self) -> PathBuf {
        self.env("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| self.home.join(".local/share"))
    }

    pub fn local_app_data(&self) -> PathBuf {
        self.env("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| self.home.join("AppData/Local"))
    }

    pub fn find_bin(&self, name: &str) -> Option<PathBuf> {
        which::which_in(name, Some(&self.search_path), &self.home).ok()
    }

    /// Spawns a process without flashing a console window on Windows.
    pub fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
        #[allow(unused_mut)]
        let mut cmd = tokio::process::Command::new(program);
        cmd.env("PATH", &self.search_path).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        cmd
    }

    pub fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.http.get(url).header("Accept", "application/json")
    }

    pub fn post(&self, url: &str) -> reqwest::RequestBuilder {
        self.http.post(url).header("Accept", "application/json")
    }
}

pub fn read_json(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).map_err(|_| FetchError::NotDetected)?;
    serde_json::from_str(&text).map_err(|e| FetchError::parse(format!("{}: {e}", path.display())))
}

/// Sends the request and decodes JSON, mapping HTTP failures to `FetchError::Http`.
pub async fn send_json(req: reqwest::RequestBuilder) -> Result<Value> {
    let resp = req.send().await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        let body: String = text.chars().take(240).collect();
        return Err(FetchError::Http { status: status.as_u16(), body });
    }
    serde_json::from_str(&text).map_err(|e| FetchError::parse(format!("{e}: {}", text.chars().take(120).collect::<String>())))
}

async fn build_search_path(env: &HashMap<String, String>, home: &Path) -> OsString {
    let mut dirs: Vec<PathBuf> = env
        .get("PATH")
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();

    #[cfg(unix)]
    if let Some(shell) = env.get("SHELL").filter(|s| !s.is_empty()) {
        let out = tokio::time::timeout(
            Duration::from_secs(3),
            tokio::process::Command::new(shell)
                .args(["-ilc", "printf '__P__%s__P__' \"$PATH\""])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await;
        if let Ok(Ok(out)) = out {
            let s = String::from_utf8_lossy(&out.stdout);
            if let Some(p) = s.split("__P__").nth(1) {
                dirs.extend(std::env::split_paths(p));
            }
        }
    }

    let extra: &[&str] = if cfg!(windows) {
        &["AppData/Roaming/npm", ".local/bin", ".cargo/bin", "scoop/shims", ".bun/bin"]
    } else {
        &[".local/bin", ".cargo/bin", ".npm-global/bin", ".bun/bin", ".volta/bin", ".local/node/bin", "bin"]
    };
    dirs.extend(extra.iter().map(|d| home.join(d)));
    if !cfg!(windows) {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    std::env::join_paths(dirs).unwrap_or_default()
}
