use portable_pty::Child;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{RwLock, broadcast, mpsc};

// The app can have one builder running and many terminals attched.

pub struct AppData {
    pub running_project: String,
    pub running_task_name: String,
    pub running_task: Option<Task>,
    pub tasks: HashMap<String, Task>,
}

#[derive(Clone)]
pub struct Task {
    pub history: Vec<u8>,
    pub running_command: Option<Arc<StdRwLock<Box<dyn Child + Send + Sync>>>>,
    pub stdin_tx: Option<mpsc::Sender<String>>,
    pub stdout_tx: Option<broadcast::Sender<String>>,
}

pub static APP_DATA: LazyLock<RwLock<AppData>> = LazyLock::new(|| {
    RwLock::new(AppData {
        tasks: HashMap::new(), //
        running_project: String::new(),
        running_task: None,
        running_task_name: String::new(),
    })
});

impl AppData {
    pub fn has_running_task(&self) -> bool {
        self.running_task.is_some()
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

        let task = Task {
            history: Vec::new(),
            running_command: Some(command),
            stdin_tx: Some(stdin_tx),
            stdout_tx: Some(stdout_tx),
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
        println!("Dropping task, key: {}", key);

        let Some(mut task) = self.tasks.remove(key) else {
            println!("Task was not running.");
            return Err("No task running".to_string());
        };

        task.running_command = None;
        task.stdin_tx = None;
        task.stdout_tx = None;

        let running_key = format!("{}-{}", self.running_task_name, self.running_project);

        if running_key == key {
            self.running_task = None;
            self.running_task_name.clear();
            self.running_project.clear();
        }

        Ok(())
    }
}
