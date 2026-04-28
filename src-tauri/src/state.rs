use crate::hid::LedColor;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppState {
    #[serde(default)]
    pub section: u8,
    #[serde(default)]
    pub r: u8,
    #[serde(default)]
    pub g: u8,
    #[serde(default)]
    pub b: u8,
    #[serde(default)]
    pub brightness: Option<u8>,
    #[serde(default)]
    pub sections: Option<[u16; 3]>,
    #[serde(default, rename = "perLedState")]
    pub per_led_state: Option<Vec<LedColor>>,
}

fn state_path() -> PathBuf {
    let config_dir = dirs_config();
    config_dir.join("state.json")
}

fn dirs_config() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    let dir = base.join("synclights");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn load_state() -> AppState {
    let path = state_path();
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => AppState::default(),
    }
}

pub fn save_state(state: &AppState) {
    let path = state_path();
    if let Ok(json) = serde_json::to_string_pretty(state) {
        if let Err(e) = fs::write(&path, json) {
            log::error!("Failed to save state: {}", e);
        }
    }
}
