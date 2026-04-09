use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub room_id: u64,
    pub cookie: String,
    pub refresh_token: String,
    #[serde(default)]
    pub debug: bool,
    #[serde(default = "default_servers")]
    pub servers: Vec<ServerSettings>,
    #[serde(default = "default_transparent")]
    pub transparent: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            room_id: 23_990_839,
            cookie: String::new(),
            refresh_token: String::new(),
            debug: false,
            servers: default_servers(),
            transparent: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerSettings {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: ServerType,
    pub port: u16,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServerType {
    Grpc,
}

fn default_transparent() -> bool {
    true
}

fn default_servers() -> Vec<ServerSettings> {
    vec![ServerSettings {
        name: "gRPC".to_owned(),
        kind: ServerType::Grpc,
        port: 50_051,
        enabled: false,
    }]
}

pub fn config_path() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("com", "uooobarry", "yuuna-danmu") {
        let dir = project_dirs.config_dir();
        let _ = fs::create_dir_all(dir);
        return dir.join("config.json");
    }

    let fallback = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("config.json");
    if let Some(parent) = fallback.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fallback
}

pub fn load() -> AppConfig {
    let path = config_path();
    let Ok(content) = fs::read_to_string(path) else {
        return AppConfig::default();
    };

    serde_json::from_str::<AppConfig>(&content).unwrap_or_default()
}

impl AppConfig {
    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建配置目录失败：{}", parent.display()))?;
        }

        let content = serde_json::to_string_pretty(self).context("序列化配置失败")?;
        fs::write(&path, content)
            .with_context(|| format!("写入配置文件失败：{}", path.display()))?;
        Ok(())
    }
}
