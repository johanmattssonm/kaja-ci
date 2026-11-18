use serde::*;
use std::collections::HashMap;

pub fn default_terminal_command() -> String {
    #[cfg(target_os = "windows")]
    {
        return "powershell".to_string();
    }

    #[cfg(target_os = "macos")]
    {
        return "zsh".to_string();
    }

    #[cfg(target_os = "linux")]
    {
        return "bash".to_string();
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "bash".to_string()
    }
}

#[derive(Debug)]
struct ParsedTaskEntry {
    task: String,
    order: i32,
    index: usize,
}

fn deserialize_tasks_option<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<Vec<serde_json::Value>>::deserialize(deserializer)?;

    let Some(raw) = raw else {
        return Ok(None);
    };

    let mut parsed = Vec::<ParsedTaskEntry>::new();

    for (index, value) in raw.into_iter().enumerate() {
        match value {
            serde_json::Value::String(task) => {
                let task = task.trim().to_string();
                if task.is_empty() {
                    continue;
                }

                parsed.push(ParsedTaskEntry {
                    task,
                    order: (index as i32) + 1,
                    index,
                });
            }
            serde_json::Value::Object(map) => {
                let task = map
                    .get("task")
                    .and_then(|value| value.as_str())
                    .or_else(|| map.get("label").and_then(|value| value.as_str()))
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                if task.is_empty() {
                    continue;
                }

                let include_in_run_all = map
                    .get("run-all")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);

                if !include_in_run_all {
                    continue;
                }

                let order = map
                    .get("order")
                    .and_then(|value| value.as_i64())
                    .and_then(|value| i32::try_from(value).ok())
                    .unwrap_or((index as i32) + 1);

                parsed.push(ParsedTaskEntry { task, order, index });
            }
            _ => continue,
        }
    }

    parsed.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then(a.index.cmp(&b.index))
            .then(a.task.cmp(&b.task))
    });

    let mut tasks = Vec::<String>::new();
    for entry in parsed {
        if tasks.iter().any(|existing| existing == &entry.task) {
            continue;
        }

        tasks.push(entry.task);
    }

    Ok(Some(tasks))
}

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

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ScheduledRun {
    pub frequency: Option<String>,
    pub at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BuildCommands {
    pub git: Option<String>,
    pub clone: Option<String>,
    pub build: Option<String>,
    pub test: Option<String>,
    pub release: Option<String>,
    pub clean: Option<String>,
    pub terminal: Option<String>,
    pub custom_comands: Option<Vec<CustomCommand>>,
    #[serde(default)]
    #[serde(rename = "scheduled-run")]
    pub scheduled_runs: Option<Vec<ScheduledRun>>,
    #[serde(default, deserialize_with = "deserialize_tasks_option")]
    pub tasks: Option<Vec<String>>,
    pub order: Option<HashMap<String, i32>>,
    #[serde(rename = "run-all")]
    pub run_all: Option<Vec<String>>,
}

impl Default for BuildCommands {
    fn default() -> Self {
        Self {
            git: None,
            clone: None,
            build: None,
            test: None,
            release: None,
            clean: None,
            terminal: Some(default_terminal_command()),
            custom_comands: None,
            scheduled_runs: None,
            tasks: None,
            order: None,
            run_all: None,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct TaskRunningStatus {
    pub ok: bool,
    pub running: bool,
    pub task_key: String,
    pub time: String,
}
