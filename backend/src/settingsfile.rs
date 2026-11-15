use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub projects: Vec<String>,
}

pub fn kaja_ci_base_dir() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home).join(".kaja-ci"))
}

pub fn kaja_ci_projects_dir() -> Result<PathBuf, String> {
    Ok(kaja_ci_base_dir()?.join("projects"))
}

pub fn kaja_ci_settings_path() -> Result<PathBuf, String> {
    Ok(kaja_ci_base_dir()?.join("settings.json"))
}

pub fn ensure_kaja_ci_dirs_exist() -> Result<(), String> {
    let base_dir = kaja_ci_base_dir()?;
    let projects_dir = kaja_ci_projects_dir()?;
    let logs_dir = base_dir.join("logs");

    fs::create_dir_all(base_dir.as_path())
        .map_err(|err| format!("Failed to create {}: {err}", base_dir.display()))?;
    fs::create_dir_all(projects_dir.as_path())
        .map_err(|err| format!("Failed to create {}: {err}", projects_dir.display()))?;
    fs::create_dir_all(logs_dir.as_path())
        .map_err(|err| format!("Failed to create {}: {err}", logs_dir.display()))?;

    Ok(())
}
