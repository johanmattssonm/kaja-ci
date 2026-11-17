use std::path::{Path, PathBuf};
use std::process::Command;

use actix_web::{Responder, web};
use serde::Deserialize;

use crate::index::clone_page;
use crate::settingsfile::{
    AppSettings, kaja_ci_base_dir, kaja_ci_projects_dir, kaja_ci_settings_path,
};

#[path = "../../frontend/src/rest.rs"]
mod rest;

use rest::BuildCommands;

#[derive(Debug, Deserialize)]
pub struct CloneParams {
    pub repo: Option<String>,
    #[serde(rename = "clone-command")]
    pub clone_command: Option<String>,
    pub project: Option<String>,
}

#[actix_web::get("/clone")]
pub async fn clone_handler(params: web::Query<CloneParams>) -> impl Responder {
    clone_page(&params, false)
}

#[actix_web::get("/compact-clone")]
pub async fn compact_clone_handler(params: web::Query<CloneParams>) -> impl Responder {
    clone_page(&params, true)
}

pub fn load_settings() -> Result<AppSettings, String> {
    let base_dir = kaja_ci_base_dir()?;
    std::fs::create_dir_all(base_dir.as_path())
        .map_err(|err| format!("Failed to create {}: {err}", base_dir.display()))?;

    let settings_path = kaja_ci_settings_path()?;

    if !settings_path.exists() {
        std::fs::write(settings_path.as_path(), "{}")
            .map_err(|err| format!("Failed to write {}: {err}", settings_path.display()))?;
        return Ok(AppSettings::default());
    }

    let content = std::fs::read_to_string(settings_path.as_path())
        .map_err(|err| format!("Failed to read {}: {err}", settings_path.display()))?;

    serde_json::from_str::<AppSettings>(content.as_str())
        .map_err(|err| format!("Failed to parse {}: {err}", settings_path.display()))
}

pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let settings_path = kaja_ci_settings_path()?;
    let content = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("Failed to serialize settings: {err}"))?;

    std::fs::write(settings_path.as_path(), content)
        .map_err(|err| format!("Failed to write {}: {err}", settings_path.display()))
}

pub fn ensure_project_in_settings(project_path: &Path) -> Result<(), String> {
    let mut settings = load_settings()?;
    let project = project_path.to_string_lossy().to_string();

    if !settings
        .projects
        .iter()
        .any(|existing| existing == &project)
    {
        settings.projects.push(project);
        save_settings(&settings)?;
    }

    Ok(())
}

pub fn add_existing_project(project_path: &str) -> Result<PathBuf, String> {
    let input = project_path.trim();
    if input.is_empty() {
        return Err("Project path is required.".to_string());
    }

    let canonical = std::fs::canonicalize(input)
        .map_err(|err| format!("Failed to resolve project path '{input}': {err}"))?;

    if !canonical.is_dir() {
        return Err("Project folder does not exist.".to_string());
    }

    let metadata = std::fs::metadata(canonical.as_path())
        .map_err(|err| format!("Failed to read project folder metadata: {err}"))?;

    if metadata.permissions().readonly() {
        return Err("Project folder is not writable.".to_string());
    }

    ensure_project_in_settings(canonical.as_path())?;
    ensure_project_ci_defaults(canonical.as_path())?;

    Ok(canonical)
}

pub fn ensure_project_ci_defaults(project_path: &Path) -> Result<(), String> {
    let ci_path = project_path.join("ci.json");

    if ci_path.exists() {
        return Ok(());
    }

    let defaults = BuildCommands::default();
    let content = serde_json::to_string_pretty(&defaults)
        .map_err(|err| format!("Failed to serialize default ci.json: {err}"))?;

    std::fs::write(ci_path.as_path(), content)
        .map_err(|err| format!("Failed to write {}: {err}", ci_path.display()))
}

pub fn clone_target_dir_from_command(
    clone_command: &str,
    project_folder: &Path,
) -> Result<PathBuf, String> {
    let parts = shell_words::split(clone_command)
        .map_err(|err| format!("Failed to parse clone command: {err}"))?;

    if parts.len() < 3 || parts[0] != "git" || parts[1] != "clone" {
        return Err(
            "Invalid clone command. Expected: git clone <repo-url> [target-dir]".to_string(),
        );
    }

    let positional_args: Vec<&str> = parts
        .iter()
        .skip(2)
        .filter(|arg| !arg.starts_with('-'))
        .map(|arg| arg.as_str())
        .collect();

    let repo_url = positional_args
        .first()
        .ok_or_else(|| "Invalid clone command. Missing repository URL".to_string())?;

    let target_dir = if let Some(target) = positional_args.get(1) {
        let target_path = PathBuf::from(target);
        if target_path.is_absolute() {
            target_path
        } else {
            project_folder.join(target_path)
        }
    } else {
        project_folder.join(repo_name(repo_url)?.as_str())
    };

    Ok(target_dir)
}

fn repo_name(repo_url: &str) -> Result<String, String> {
    let clean = repo_url.trim().trim_end_matches('/');
    let last = clean
        .rsplit('/')
        .next()
        .ok_or_else(|| "Invalid repository URL".to_string())?;

    let name = last.strip_suffix(".git").unwrap_or(last).trim();

    if name.is_empty() {
        return Err("Could not infer repository name from URL".to_string());
    }

    Ok(name.to_string())
}

pub fn parse_repo_input(value: &str) -> Result<String, String> {
    let value = value.trim();

    if value.is_empty() {
        return Err("Repository URL is empty".to_string());
    }

    if value.starts_with("git clone") {
        let parts: Vec<&str> = value.split_whitespace().collect();

        if parts.len() < 3 {
            return Err("Invalid clone command. Expected: git clone <repo-url>".to_string());
        }

        for candidate in parts.into_iter().skip(2) {
            if candidate.starts_with('-') {
                continue;
            }

            return Ok(candidate.to_string());
        }

        return Err("Invalid clone command. Missing repository URL".to_string());
    }

    Ok(value.to_string())
}

pub fn clone_command_from_input(value: &str) -> Result<String, String> {
    let value = value.trim();

    if value.is_empty() {
        return Err("Repository URL is empty".to_string());
    }

    if value.starts_with("git clone") {
        return Ok(value.to_string());
    }

    Ok(format!("git clone {value}"))
}

pub fn clone_repo(repo_url: &str) -> Result<PathBuf, String> {
    let projects_dir = kaja_ci_projects_dir()?;
    std::fs::create_dir_all(projects_dir.as_path())
        .map_err(|err| format!("Failed to create {}: {err}", projects_dir.display()))?;

    let target_dir = projects_dir.join(repo_name(repo_url)?.as_str());

    if !target_dir.exists() {
        let output = Command::new("git")
            .arg("clone")
            .arg(repo_url)
            .arg(target_dir.as_os_str())
            .output()
            .map_err(|err| format!("Failed to run git clone: {err}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(output.stderr.as_slice()).to_string();
            return Err(format!("git clone failed: {}", stderr.trim()));
        }
    }

    ensure_project_in_settings(target_dir.as_path())?;
    ensure_project_ci_defaults(target_dir.as_path())?;

    Ok(target_dir)
}
