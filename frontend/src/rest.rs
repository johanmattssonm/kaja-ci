use serde::*;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Action {
    pub action: String,
    pub command: Option<String>,
    pub folder: Option<String>,
    pub stdin: Option<String>,
    pub task_key: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ActionResponse {
    pub action: String,
    pub status: Option<String>,
    pub stdout: Option<String>,
    pub cursor_row: Option<usize>,
    pub cursor_col: Option<usize>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CustomCommand {
    pub label: Option<String>,
    pub command: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BuildCommands {
    pub git: Option<String>,
    pub build: Option<String>,
    pub test: Option<String>,
    pub release: Option<String>,
    pub clean: Option<String>,
    pub terminal: Option<String>,
    pub custom_comands: Option<Vec<CustomCommand>>,
    pub tasks: Option<Vec<String>>,
}

#[derive(Serialize, Clone, Debug)]
pub struct TaskRunningStatus {
    pub ok: bool,
    pub running: bool,
    pub task_key: String,
    pub time: String,
}
