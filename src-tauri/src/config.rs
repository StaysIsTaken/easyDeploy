//! Persistent app configuration (projects, targets, profiles) as JSON in the
//! OS app-config directory. Secrets are NOT stored here – see `secrets`.

use std::path::PathBuf;

use serde_json::Value;
use tauri::{AppHandle, Manager};

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("config.json"))
}

#[tauri::command]
pub fn config_load(app: AppHandle) -> Result<Value, String> {
    let path = config_path(&app)?;
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("config.json ist beschädigt: {e}")),
        Err(_) => Ok(Value::Null),
    }
}

#[tauri::command]
pub fn config_save(app: AppHandle, data: Value) -> Result<(), String> {
    let path = config_path(&app)?;
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn path_exists(path: String) -> bool {
    PathBuf::from(path).exists()
}

/// Only used to write GitHub workflow files, so writes are limited to
/// `<project>/.github/workflows/*.yml` instead of arbitrary paths.
#[tauri::command]
pub fn write_text_file(path: String, content: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    let normalized = path.replace('\\', "/");
    let in_workflows = p.parent().is_some_and(|d| d.ends_with(".github/workflows") || d.ends_with(".github\\workflows"));
    let is_yaml = normalized.ends_with(".yml") || normalized.ends_with(".yaml");
    if !p.is_absolute() || !in_workflows || !is_yaml || normalized.contains("/../") {
        return Err("Schreiben ist nur für .github/workflows/*.yml erlaubt".into());
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(p, content).map_err(|e| e.to_string())
}
