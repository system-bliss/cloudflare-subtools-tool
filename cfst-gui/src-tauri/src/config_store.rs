use crate::models::Settings;
use std::fs;
use std::path::{Path, PathBuf};

fn settings_path(app_data_dir: &Path) -> PathBuf {
    let dir = app_data_dir.join("cfst-gui");
    dir.join("settings.json")
}

pub fn load_settings(app_data_dir: &Path) -> Settings {
    let path = settings_path(app_data_dir);
    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<Settings>(&content) {
            Ok(mut s) => {
                // Ensure presets are always populated
                if s.presets.is_empty() {
                    s.presets = crate::models::default_presets();
                }
                s
            }
            Err(_) => Settings::default(),
        },
        Err(_) => Settings::default(),
    }
}

pub fn save_settings(app_data_dir: &Path, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app_data_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {}", e))?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| format!("Serialize error: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Write error: {}", e))?;
    Ok(())
}
