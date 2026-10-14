use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AppProfile {
    pub bundle_id: String,
    /// "code" | "messaging" | "email" | "prose"
    pub mode: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Hotkey binding string parsed by `hotkey::parse_hotkey` — either a bare modifier
    /// ("alt") or a "mod[+mod...]+key" combo ("alt+space"), key tokens matching browser
    /// `KeyboardEvent.code`.
    pub hotkey: String,
    /// "push_to_talk" (hold) or "toggle" (press to start/stop).
    pub activation_mode: String,
    /// Which STT model to use (see stt::MODELS).
    pub stt_model_id: String,
    /// How aggressively the LLM cleans up the transcript.
    /// "none" | "light" | "medium" | "high"
    pub cleanup_level: String,
    /// Replace spoken "from the clipboard" / "in the clipboard" phrases with the clipboard contents.
    pub clipboard_substitution_enabled: bool,
    pub ollama_url: String,
    pub ollama_model: String,
    /// Words/names the cleanup prompt should know how to spell.
    pub dictionary: Vec<String>,
    /// Per-app output style overrides; unlisted apps fall back to built-in
    /// defaults (see cleanup::builtin_mode), then to "prose".
    pub app_profiles: Vec<AppProfile>,
    /// "forever" | "24h" | "never" — how long dictation history is kept.
    pub history_retention: String,
    /// Per-device streaming-VAD RMS onset threshold, keyed by cpal device
    /// name, measured from real speech RMS the first time an empty streaming
    /// result on that device is recovered by whole-utterance fallback (see
    /// `pipeline::process`). Devices with no entry use
    /// `stt::STREAMING_VAD_RMS_THRESHOLD`. Not user-facing — no Settings UI.
    pub mic_calibration: HashMap<String, f32>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: "alt+space".to_string(),
            activation_mode: "push_to_talk".to_string(),
            stt_model_id: crate::stt::DEFAULT_MODEL_ID.to_string(),
            cleanup_level: "light".to_string(),
            clipboard_substitution_enabled: true,
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "qwen3:4b-instruct".to_string(),
            dictionary: Vec::new(),
            app_profiles: Vec::new(),
            history_retention: "forever".to_string(),
            mic_calibration: HashMap::new(),
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
