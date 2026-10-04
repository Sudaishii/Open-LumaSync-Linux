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

pub fn dirs_config() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".config")
        });
    let dir = base.join("snzhy-opensycnlights");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn load_state() -> AppState {
    let path = state_path();
    match fs::read_to_string(&path) {
        Ok(content) => {
            let mut state: AppState = serde_json::from_str(&content).unwrap_or_default();
            if state.sections.is_some_and(|v| crate::validation::sections(v).is_err()) { state.sections=None; }
            state
        },
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

pub fn load_controller_config() -> Option<serde_json::Value> {
    let data = fs::read(dirs_config().join("controller.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&data).ok()?;
    if value.is_object() { Some(value) } else { None }
}

pub fn save_controller_config(value: &serde_json::Value) -> Result<(), String> {
    write_controller_config(&dirs_config().join("controller.json"), value)
}

fn write_controller_config(path: &std::path::Path, value: &serde_json::Value) -> Result<(), String> {
    use std::io::Write;
    if !value.is_object() { return Err("Controller settings must be an object".into()); }
    let data = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if data.len() > 65536 { return Err("Controller settings are too large".into()); }
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&data).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(temporary, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod controller_tests {
    #[test]
    fn atomic_settings_replace_and_reject_invalid_input() {
        let path = std::env::temp_dir().join(format!("snzhy-controller-test-{}.json",std::process::id()));
        let first = serde_json::json!({"resumeEnabled":false});
        let next = serde_json::json!({"resumeEnabled":true,"resumeWanted":true,"lastMode":"audio"});
        super::write_controller_config(&path,&first).unwrap();
        super::write_controller_config(&path,&next).unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap(),next);
        assert!(super::write_controller_config(&path,&serde_json::json!([])).is_err());
        assert!(super::write_controller_config(&path,&serde_json::json!({"large":"x".repeat(65536)})).is_err());
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap(),next);
        std::fs::remove_file(path).unwrap();
    }
}
