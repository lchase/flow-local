use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// tauri-plugin-global-shortcut binding string.
    pub hotkey: String,
    /// "push_to_talk" (hold) or "toggle" (press to start/stop).
    pub activation_mode: String,
    /// Run the transcript through Ollama before pasting.
    pub cleanup_enabled: bool,
    pub ollama_url: String,
    pub ollama_model: String,
    /// Words/names the cleanup prompt should know how to spell.
    pub dictionary: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: "alt+space".to_string(),
            activation_mode: "push_to_talk".to_string(),
            cleanup_enabled: true,
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "qwen3:4b-instruct-2507".to_string(),
            dictionary: Vec::new(),
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<AppSettings>,
}

impl SettingsStore {
    pub fn load(app: &AppHandle) -> Result<Self> {
        let dir = app
            .path()
            .app_config_dir()
            .map_err(|e| anyhow!("no config dir: {e}"))?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("settings.json");
        let current = match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => AppSettings::default(),
        };
        Ok(Self {
            path,
            current: Mutex::new(current),
        })
    }

    pub fn get(&self) -> AppSettings {
        self.current.lock().unwrap().clone()
    }

    pub fn set(&self, settings: AppSettings) -> Result<()> {
        let json = serde_json::to_string_pretty(&settings)?;
        std::fs::write(&self.path, json)?;
        *self.current.lock().unwrap() = settings;
        Ok(())
    }
}
