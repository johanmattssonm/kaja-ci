use chrono::{Local, SecondsFormat};
use portable_pty::Child;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{RwLock, broadcast, mpsc};

use crate::branch::current_branch;
use crate::taskrunner::send_task_status;

// The app can have one builder running and many terminals attached.
pub struct AppData {
    pub running_project: String,
    pub running_task_name: String,
    pub running_task: Option<Task>,
    pub tasks: HashMap<String, Task>,
    pub task_history: HashMap<String, Vec<u8>>,
    pub task_history_files: HashMap<String, TaskHistoryFiles>,
    pub task_history_metadata: HashMap<String, TaskHistoryMetadata>,
}

#[derive(Clone)]
pub struct Task {
    pub history: Vec<u8>,
    pub running_command: Option<Arc<StdRwLock<Box<dyn Child + Send + Sync>>>>,
    pub stdin_tx: Option<mpsc::Sender<String>>,
    pub stdout_tx: Option<broadcast::Sender<String>>,
    pub last_activity: Arc<std::sync::atomic::AtomicU64>,
}

pub static APP_DATA: LazyLock<RwLock<AppData>> = LazyLock::new(|| {
    RwLock::new(AppData {
        tasks: HashMap::new(), //
        task_history: HashMap::new(),
        task_history_files: HashMap::new(),
        task_history_metadata: HashMap::new(),
        running_project: String::new(),
        running_task: None,
        running_task_name: String::new(),
    })
});

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskHistoryMetadata {
    pub datestamp: String,
    pub task_key: String,
    pub project: String,
    pub action: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub raw_file: String,
    #[serde(default)]
    pub branch: String,
}

#[derive(Clone, Debug)]
pub struct TaskHistoryFiles {
    pub datestamp: String,
    pub raw_path: PathBuf,
    pub meta_path: PathBuf,
    pub project: String,
    pub action: String,
    pub started_at: String,
    pub branch: String,
}

impl AppData {
    fn is_task_command_running(task: &Task) -> bool {
        let Some(command_lock) = task.running_command.as_ref() else {
            return false;
        };

        let Ok(mut command) = command_lock.write() else {
            return false;
        };

        matches!(command.try_wait(), Ok(None))
    }

    pub fn task_is_running(&self, task_key: &str) -> bool {
        self.tasks
            .get(task_key)
            .is_some_and(Self::is_task_command_running)
    }

    fn has_terminal_task_key(task_key: &str) -> bool {
        task_key.starts_with("terminal-")
    }

    fn find_running_builder_task_key(&self) -> Option<String> {
        self.tasks.iter().find_map(|(task_key, task)| {
            if Self::has_terminal_task_key(task_key.as_str()) {
                return None;
            }

            if Self::is_task_command_running(task) {
                return Some(task_key.clone());
            }

            None
        })
    }

    pub fn running_builder_task_key(&self) -> Option<String> {
        self.find_running_builder_task_key()
    }

    pub fn reconcile_running_state(&mut self) {
        if self.running_task_name.is_empty() || self.running_project.is_empty() {
            return;
        }

        let running_key = format!("{}-{}", self.running_task_name, self.running_project);

        if self.task_is_running(running_key.as_str()) {
            return;
        }

        if let Some(task) = self.tasks.get_mut(running_key.as_str()) {
            task.running_command = None;
            task.stdin_tx = None;
            task.stdout_tx = None;
        }

        self.running_task = None;
        self.running_task_name.clear();
        self.running_project.clear();
    }

    pub fn running_project(&self) -> String {
        self.running_project.clone()
    }

    pub fn running_task_name(&self) -> String {
        self.running_task_name.clone()
    }

    pub fn terminal_count(&self) -> usize {
        self.tasks.len()
    }

    pub fn start_task(
        &mut self,
        task_type: Option<String>,
        project_folder: Option<String>,
        task_key: Option<String>,
        raw_history_file_name: Option<String>,
        command: Arc<StdRwLock<Box<dyn Child + Send + Sync>>>,
        stdin_tx: mpsc::Sender<String>,
        stdout_tx: broadcast::Sender<String>,
        last_activity: Arc<std::sync::atomic::AtomicU64>,
    ) -> Result<String, String> {
        let Some(task_type) = task_type else {
            return Err("No task_type for process".to_string());
        };

        let Some(project_folder) = project_folder else {
            println!("No project folder.");
            return Err("No project folder.".to_string());
        };

        let key = task_key.unwrap_or_else(|| format!("{}-{}", task_type, project_folder));

        self.task_history.insert(key.clone(), Vec::new());

        if task_type != "terminal"
            && let Some(files) = create_history_files(
                key.as_str(),
                task_type.as_str(),
                project_folder.as_str(),
                raw_history_file_name,
            )
        {
            self.task_history_files.insert(key.clone(), files.clone());
            self.task_history_metadata.insert(
                key.clone(),
                TaskHistoryMetadata {
                    datestamp: files.datestamp.clone(),
                    task_key: key.clone(),
                    project: files.project.clone(),
                    action: files.action.clone(),
                    status: "running".to_string(),
                    started_at: files.started_at.clone(),
                    finished_at: None,
                    raw_file: files.raw_path.to_string_lossy().to_string(),
                    branch: files.branch.clone(),
                },
            );
        }

        let task = Task {
            history: Vec::new(),
            running_command: Some(command),
            stdin_tx: Some(stdin_tx),
            stdout_tx: Some(stdout_tx),
            last_activity,
        };

        if task_type != "terminal" {
            self.register_builder_task(task.clone(), task_type.clone(), project_folder.clone())?;
        }

        self.tasks.insert(key.clone(), task);

        Ok(key)
    }

    fn register_builder_task(
        &mut self,
        task: Task,
        task_type: String,
        project_folder: String,
    ) -> Result<(), String> {
        self.running_project = project_folder;
        self.running_task_name = task_type;
        self.running_task = Some(task);

        Ok(())
    }

    pub fn drop_task(
        &mut self,
        task_type: Option<String>,
        project_folder: Option<String>,
    ) -> Result<(), String> {
        let (Some(task_type), Some(project_folder)) = (task_type, project_folder) else {
            println!("Cannot drop task: missing task_type or project_folder");
            return Err("No project folder".to_string());
        };

        let key = format!("{}-{}", task_type, project_folder);
        self.drop_task_by_key(key.as_str())
    }

    pub fn drop_task_by_key(&mut self, key: &str) -> Result<(), String> {
        self.drop_task_by_key_with_status(key, Some(false))
    }

    pub fn drop_task_by_key_with_status(
        &mut self,
        key: &str,
        ok: Option<bool>,
    ) -> Result<(), String> {
        let running_key = format!("{}-{}", self.running_task_name, self.running_project);

        let (status_action, status_project): (Option<String>, Option<String>) =
            if running_key == key {
                (
                    (!self.running_task_name.is_empty()).then_some(self.running_task_name.clone()),
                    (!self.running_project.is_empty()).then_some(self.running_project.clone()),
                )
            } else {
                (None, None)
            };

        let Some(task) = self.tasks.get_mut(key) else {
            println!("Task was not running.");

            if running_key == key {
                self.running_task = None;
                self.running_task_name.clear();
                self.running_project.clear();
                let completed_action = status_action
                    .clone()
                    .or_else(|| key.split_once('-').map(|(action, _)| action.to_string()))
                    .unwrap_or_else(|| key.to_string());

                let completed_project = status_project
                    .clone()
                    .or_else(|| key.split_once('-').map(|(_, project)| project.to_string()))
                    .unwrap_or_default();

                if let Some(metadata) = write_task_history_completion(
                    self.task_history_files.get(key),
                    key,
                    completed_action.as_str(),
                    completed_project.as_str(),
                    ok.unwrap_or(false),
                ) {
                    self.task_history_metadata.insert(key.to_string(), metadata);
                }
                if !Self::has_terminal_task_key(key) {
                    send_task_status(
                        key,
                        ok.unwrap_or(false),
                        status_action.as_deref(),
                        status_project.as_deref(),
                    );
                }
                return Ok(());
            }

            return Err("No task running".to_string());
        };

        task.running_command = None;
        task.stdin_tx = None;
        task.stdout_tx = None;
        self.tasks.remove(key);

        let completed_action = status_action
            .clone()
            .or_else(|| key.split_once('-').map(|(action, _)| action.to_string()))
            .unwrap_or_else(|| key.to_string());

        let completed_project = status_project
            .clone()
            .or_else(|| key.split_once('-').map(|(_, project)| project.to_string()))
            .unwrap_or_default();

        if let Some(metadata) = write_task_history_completion(
            self.task_history_files.get(key),
            key,
            completed_action.as_str(),
            completed_project.as_str(),
            ok.unwrap_or(false),
        ) {
            self.task_history_metadata.insert(key.to_string(), metadata);
        }

        if running_key == key {
            self.running_task = None;
            self.running_task_name.clear();
            self.running_project.clear();
        }

        self.task_history_files.remove(key);

        if !Self::has_terminal_task_key(key) {
            send_task_status(
                key,
                ok.unwrap_or(false),
                status_action.as_deref(),
                status_project.as_deref(),
            );
        }

        Ok(())
    }

    pub fn append_task_history(&mut self, key: &str, output: &[u8]) {
        if output.is_empty() {
            return;
        }

        self.task_history
            .entry(key.to_string())
            .or_default()
            .extend_from_slice(output);

        if let Some(files) = self.task_history_files.get(key) {
            append_output_to_raw_file(files.raw_path.as_path(), output);
        }

        if let Some(task) = self.tasks.get_mut(key) {
            task.history.extend_from_slice(output);
        }
    }

    pub fn task_history(&self, key: &str) -> Option<Vec<u8>> {
        if let Some(history) = self.task_history.get(key) {
            return Some(history.clone());
        }

        self.tasks.get(key).map(|task| task.history.clone())
    }
}

pub fn log_base_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".kaja-ci").join("logs");
    }

    PathBuf::from(".kaja-ci").join("logs")
}

pub fn ensure_log_base_dir_exists() -> std::io::Result<()> {
    fs::create_dir_all(log_base_dir().as_path())
}

fn create_history_files(
    task_key: &str,
    action: &str,
    project: &str,
    raw_history_file_name: Option<String>,
) -> Option<TaskHistoryFiles> {
    let base_dir = log_base_dir();

    if let Err(err) = fs::create_dir_all(base_dir.as_path()) {
        println!(
            "Failed to create history directory {}: {}",
            base_dir.display(),
            err
        );
        return None;
    }

    let datestamp = Local::now().format("%Y%m%dT%H%M%S%.3f%z").to_string();
    let raw_path = raw_history_file_name
        .filter(|name| !name.trim().is_empty())
        .map(|name| base_dir.join(name))
        .unwrap_or_else(|| base_dir.join(format!("{}.raw", datestamp)));
    let meta_path = base_dir.join(format!("{}.json", datestamp));

    let started_at = Local::now().to_rfc3339_opts(SecondsFormat::Secs, false);

    let files = TaskHistoryFiles {
        datestamp,
        raw_path,
        meta_path,
        project: project.to_string(),
        action: action.to_string(),
        started_at,
        branch: current_branch(project).unwrap_or_default(),
    };

    let metadata = TaskHistoryMetadata {
        datestamp: files.datestamp.clone(),
        task_key: task_key.to_string(),
        project: files.project.clone(),
        action: files.action.clone(),
        status: "running".to_string(),
        started_at: files.started_at.clone(),
        finished_at: None,
        raw_file: files.raw_path.to_string_lossy().to_string(),
        branch: files.branch.clone(),
    };

    write_metadata(files.meta_path.as_path(), &metadata);

    Some(files)
}

fn append_output_to_raw_file(path: &Path, output: &[u8]) {
    let mut file = match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => file,
        Err(err) => {
            println!(
                "Failed to open raw history file {}: {}",
                path.display(),
                err
            );
            return;
        }
    };

    if let Err(err) = file.write_all(output) {
        println!(
            "Failed writing raw history file {}: {}",
            path.display(),
            err
        );
    }
}

fn write_task_history_completion(
    files: Option<&TaskHistoryFiles>,
    task_key: &str,
    action: &str,
    project: &str,
    ok: bool,
) -> Option<TaskHistoryMetadata> {
    let Some(files) = files else {
        return None;
    };

    let metadata = TaskHistoryMetadata {
        datestamp: files.datestamp.clone(),
        task_key: task_key.to_string(),
        project: if project.is_empty() {
            files.project.clone()
        } else {
            project.to_string()
        },
        action: if action.is_empty() {
            files.action.clone()
        } else {
            action.to_string()
        },
        status: if ok {
            "success".to_string()
        } else {
            "fail".to_string()
        },
        started_at: files.started_at.clone(),
        finished_at: Some(Local::now().to_rfc3339_opts(SecondsFormat::Secs, false)),
        raw_file: files.raw_path.to_string_lossy().to_string(),
        branch: files.branch.clone(),
    };

    write_metadata(files.meta_path.as_path(), &metadata);
    Some(metadata)
}

fn write_metadata(path: &Path, metadata: &TaskHistoryMetadata) {
    let payload = match serde_json::to_vec_pretty(metadata) {
        Ok(payload) => payload,
        Err(err) => {
            println!(
                "Failed to serialize metadata for {}: {}",
                path.display(),
                err
            );
            return;
        }
    };

    if let Err(err) = fs::write(path, payload) {
        println!("Failed to write metadata file {}: {}", path.display(), err);
    }
}
