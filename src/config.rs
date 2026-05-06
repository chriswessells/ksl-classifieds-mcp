use std::path::PathBuf;

use serde::Deserialize;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct Config {
    pub user_agent: String,
    pub connect_timeout_secs: u64,
    pub request_timeout_secs: u64,
    pub min_delay_ms: u64,
    pub max_delay_ms: u64,
    pub daily_request_cap: u32,
    pub data_dir: PathBuf,
}

#[derive(Deserialize, Default)]
struct RawConfig {
    user_agent: Option<String>,
    connect_timeout_secs: Option<u64>,
    request_timeout_secs: Option<u64>,
    min_delay_ms: Option<u64>,
    max_delay_ms: Option<u64>,
    daily_request_cap: Option<u32>,
    data_dir: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        let config_path = dirs_home().map(|h| h.join(".config/ksl-mcp/config.toml"));

        let raw: RawConfig = match &config_path {
            Some(path) if path.exists() => match std::fs::read_to_string(path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(raw) => raw,
                    Err(e) => {
                        warn!("Failed to parse config at {}: {}", path.display(), e);
                        RawConfig::default()
                    }
                },
                Err(e) => {
                    warn!("Failed to read config at {}: {}", path.display(), e);
                    RawConfig::default()
                }
            },
            Some(_) => {
                info!("No config file found, using defaults");
                RawConfig::default()
            }
            None => RawConfig::default(),
        };

        let defaults = Self::defaults();
        let home = dirs_home().unwrap_or_else(|| PathBuf::from("/tmp"));

        let data_dir = raw
            .data_dir
            .map(|s| expand_tilde(&s))
            .unwrap_or_else(|| defaults.data_dir.clone());

        // Security: data_dir must be within $HOME
        let data_dir = if data_dir.starts_with(&home) {
            data_dir
        } else {
            warn!(
                "data_dir '{}' is outside $HOME, using default",
                data_dir.display()
            );
            defaults.data_dir.clone()
        };

        let cfg = Config {
            user_agent: raw.user_agent.unwrap_or(defaults.user_agent),
            connect_timeout_secs: raw
                .connect_timeout_secs
                .unwrap_or(defaults.connect_timeout_secs),
            request_timeout_secs: raw
                .request_timeout_secs
                .unwrap_or(defaults.request_timeout_secs),
            min_delay_ms: raw.min_delay_ms.unwrap_or(defaults.min_delay_ms),
            max_delay_ms: raw.max_delay_ms.unwrap_or(defaults.max_delay_ms),
            daily_request_cap: raw
                .daily_request_cap
                .unwrap_or(defaults.daily_request_cap),
            data_dir,
        };

        info!(
            connect_timeout = cfg.connect_timeout_secs,
            request_timeout = cfg.request_timeout_secs,
            daily_cap = cfg.daily_request_cap,
            data_dir = %cfg.data_dir.display(),
            "Config loaded"
        );

        cfg
    }

    pub fn defaults() -> Self {
        let data_dir = dirs_home()
            .map(|h| h.join(".local/share/ksl-mcp"))
            .unwrap_or_else(|| PathBuf::from("/tmp/ksl-mcp"));

        Config {
            user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2.1 Safari/605.1.15".to_string(),
            connect_timeout_secs: 10,
            request_timeout_secs: 30,
            min_delay_ms: 3000,
            max_delay_ms: 8000,
            daily_request_cap: 500,
            data_dir,
        }
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        dirs_home()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(s))
    } else {
        PathBuf::from(s)
    }
}
