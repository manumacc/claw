use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub storage_root: PathBuf,
    pub socket_path: PathBuf,
    pub default_provider: String,
    pub workspace_root: PathBuf,
    pub codex: CodexConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexConfig {
    pub command: String,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub search: bool,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let mut config = Self::defaults()?;
        let config_path = config.storage_root.join("config.json");

        if config_path.exists() {
            let raw = std::fs::read_to_string(&config_path)
                .with_context(|| format!("reading {}", config_path.display()))?;
            config = serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", config_path.display()))?;
        }

        if let Ok(root) = env::var("CLAW_HOME") {
            config.storage_root = PathBuf::from(root);
            config.socket_path = config.storage_root.join("claw.sock");
        }

        if let Ok(provider) = env::var("CLAW_PROVIDER") {
            config.default_provider = provider;
        }

        if let Ok(command) = env::var("CLAW_CODEX") {
            config.codex.command = command;
        }

        Ok(config)
    }

    pub fn defaults() -> Result<Self> {
        let storage_root = default_storage_root()?;
        let workspace_root = if let Ok(root) = env::var("CLAW_WORKSPACE") {
            PathBuf::from(root)
        } else {
            env::current_dir().context("resolving workspace root")?
        };

        Ok(Self {
            socket_path: storage_root.join("claw.sock"),
            storage_root,
            default_provider: "codex".to_string(),
            workspace_root,
            codex: CodexConfig {
                command: "codex".to_string(),
                model: None,
                profile: None,
                search: false,
            },
        })
    }

    pub fn with_storage_root(mut self, root: impl AsRef<Path>) -> Self {
        self.storage_root = root.as_ref().to_path_buf();
        self.socket_path = self.storage_root.join("claw.sock");
        self
    }
}

fn default_storage_root() -> Result<PathBuf> {
    if let Ok(root) = env::var("CLAW_HOME") {
        return Ok(PathBuf::from(root));
    }

    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Claw"))
}
