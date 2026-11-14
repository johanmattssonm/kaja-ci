use portable_pty::Child;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{RwLock, broadcast, mpsc};

use crate::taskrunner::send_task_status;

// The app can have one builder running and many terminals attached.
pub struct AppData {
    pub running_project: String,
    pub running_task_name: String,
    pub running_task: Option<Task>,
    pub tasks: HashMap<String, Task>,
    pub task_history: HashMap<String, Vec<u8>>,
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
        running_project: String::new(),
        running_task: None,
        running_task_name: String::new(),
    })
});

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

    pub fn has_running_task(&self) -> bool {
        if self.running_task_name.is_empty() || self.running_project.is_empty() {
            return false;
        }

        let running_key = format!("{}-{}", self.running_task_name, self.running_project);
        self.task_is_running(running_key.as_str())
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
        println!("Starting task, key: {}", key);

        self.task_history.insert(key.clone(), Vec::new());

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
        println!("Dropping task, key: {}", key);

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
                send_task_status(
                    key,
                    ok.unwrap_or(false),
                    status_action.as_deref(),
                    status_project.as_deref(),
                );
                return Ok(());
            }

            return Err("No task running".to_string());
        };

        task.running_command = None;
        task.stdin_tx = None;
        task.stdout_tx = None;
        self.tasks.remove(key);

        if running_key == key {
            self.running_task = None;
            self.running_task_name.clear();
            self.running_project.clear();
        }

        send_task_status(
            key,
            ok.unwrap_or(false),
            status_action.as_deref(),
            status_project.as_deref(),
        );

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
