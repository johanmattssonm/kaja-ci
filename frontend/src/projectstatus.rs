use crate::auth;
use crate::buildstatus::render_build_status_error;
use crate::rest::{BuildCommands, CustomCommand};
use kaja_web::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use wasm_bindgen_futures::spawn_local;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum TaskAttribute {
    Name(String),
    Detailed(TaskDetails),
}

#[derive(Debug, Clone, Deserialize)]
struct TaskDetails {
    task: Option<String>,
    label: Option<String>,
    order: Option<i32>,
    custom: Option<bool>,
}

#[component("project-status")]
struct ProjectStatus {
    #[attribute]
    pub project: String,
    #[attribute]
    pub git: String,
    #[attribute]
    pub clone: String,
    #[attribute]
    pub build: String,
    #[attribute]
    pub test: String,
    #[attribute]
    pub release: String,
    #[attribute]
    pub clean: String,
    #[attribute]
    pub terminal: String,
    #[attribute]
    pub custom: Vec<CustomCommand>,
    #[attribute]
    pub branch: String,
    #[attribute]
    pub tasks: Vec<Value>,
    #[attribute]
    pub order: HashMap<String, i32>,
}

impl Component for ProjectStatusComponent {
    fn connected(&self) {
        let id = self.get_id();
        log!("Connected:", &id);
        self.render();

        let project: String = self.get_attribute("project").unwrap_or_default();

        let project_for_settings = project.clone();
        let id_for_settings = id.clone();

        spawn_local(async move {
            let settings = load_settings(&project_for_settings).await;
            let element = get_element_by_id(&id_for_settings);

            if element.is_none() {
                log!("Missing component.");
                return;
            }

            if settings.is_none() {
                log!("Missing settings.");
                return;
            }

            let element = element.unwrap();
            let settings = settings.unwrap();

            let git = settings.git.unwrap_or_default();
            let clone = settings.clone.unwrap_or_default();
            let build = settings.build.unwrap_or_default();
            let test = settings.test.unwrap_or_default();
            let release = settings.release.unwrap_or_default();
            let clean = settings.clean.unwrap_or_default();
            let terminal = settings.terminal.unwrap_or_default();
            let custom = settings.custom_comands.unwrap_or_default();
            let custom_json = serde_json::to_string(&custom).unwrap_or_else(|_| "[]".to_string());
            let tasks = settings.tasks.clone().unwrap_or_else(|| {
                vec!["git".to_string(), "build".to_string(), "test".to_string()]
            });
            let order = settings.order.unwrap_or_default();
            let tasks_json = serde_json::to_string(&tasks).unwrap_or_else(|_| "[]".to_string());
            let order_json = serde_json::to_string(&order).unwrap_or_else(|_| "{}".to_string());

            let _ = element.set_attribute("git", git.as_str());
            let _ = element.set_attribute("clone", clone.as_str());
            let _ = element.set_attribute("build", build.as_str());
            let _ = element.set_attribute("test", test.as_str());
            let _ = element.set_attribute("release", release.as_str());
            let _ = element.set_attribute("clean", clean.as_str());
            let _ = element.set_attribute("terminal", terminal.as_str());
            let _ = element.set_attribute("custom", custom_json.as_str());
            let _ = element.set_attribute("tasks", tasks_json.as_str());
            let _ = element.set_attribute("order", order_json.as_str());
        });

        spawn_local(async move {
            let branch = load_branch(project.as_str()).await;
            let element = get_element_by_id(&id);

            if element.is_none() {
                return;
            }

            let element = element.unwrap();
            let _ = element.set_attribute("branch", branch.as_str());
        });
    }

    fn disconnected(&self) {
        log!("Disconnected:", self.get_id());
    }

    fn attribute_changed(&self, name: &str, _old_value: &str, _new_value: &str) {
        if name == "project"
            || name == "custom"
            || name == "branch"
            || name == "git"
            || name == "clone"
            || name == "build"
            || name == "test"
            || name == "release"
            || name == "clean"
            || name == "terminal"
            || name == "tasks"
            || name == "order"
        {
            self.render();
        }
    }
}

impl ProjectStatusComponent {
    fn render(&self) {
        let id = self.get_id();

        let element = match get_element_by_id(&id) {
            Some(element) => element,
            None => {
                log!("No root element for build component");
                return;
            }
        };

        let project: String = self.get_attribute("project").unwrap_or_default();
        let custom_actions: Vec<CustomCommand> = self.get_attribute("custom").unwrap_or_default();
        let branch: String = self
            .get_attribute("branch")
            .unwrap_or_else(|| "unknown".to_string());
        let tasks: Vec<TaskAttribute> = self.get_attribute("tasks").unwrap_or_default();
        let order: HashMap<String, i32> = self.get_attribute("order").unwrap_or_default();
        let git: String = self.get_attribute("git").unwrap_or_default();
        let clone: String = self.get_attribute("clone").unwrap_or_default();
        let build: String = self.get_attribute("build").unwrap_or_default();
        let test: String = self.get_attribute("test").unwrap_or_default();
        let release: String = self.get_attribute("release").unwrap_or_default();
        let clean: String = self.get_attribute("clean").unwrap_or_default();
        let terminal: String = self.get_attribute("terminal").unwrap_or_default();

        let configured_task_names = tasks
            .iter()
            .filter_map(|task| match task {
                TaskAttribute::Name(name) => {
                    let value = name.trim();
                    if value.is_empty() {
                        None
                    } else {
                        Some(value.to_string())
                    }
                }
                TaskAttribute::Detailed(details) => details
                    .task
                    .as_deref()
                    .or(details.label.as_deref())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string),
            })
            .collect::<Vec<String>>();

        let task_list_position = |key: &str, label: &str| -> Option<i32> {
            let key_lc = key.trim().to_ascii_lowercase();
            let label_lc = label.trim().to_ascii_lowercase();

            configured_task_names
                .iter()
                .position(|task| {
                    let task_lc = task.trim().to_ascii_lowercase();
                    task_lc == key_lc || task_lc == label_lc
                })
                .map(|index| (index as i32) + 1)
        };

        let resolved_order = |key: &str, label: &str, fallback: i32| -> i32 {
            order
                .get(key)
                .copied()
                .or_else(|| order.get(label).copied())
                .or_else(|| task_list_position(key, label))
                .unwrap_or(fallback)
        };

        let mut ordered_tasks = Vec::<(String, String, i32, usize)>::new();

        let mut add_builtin =
            |label: &str, key: &str, command: &str, fallback: i32, index: usize| {
                if command.trim().is_empty() {
                    return;
                }

                ordered_tasks.push((
                    label.to_string(),
                    key.to_string(),
                    resolved_order(key, label, fallback),
                    index,
                ));
            };

        add_builtin("Git Fetch", "git", git.as_str(), 1, 0);
        add_builtin("Clone", "clone", clone.as_str(), 2, 1);
        add_builtin("Build", "build", build.as_str(), 3, 2);
        add_builtin("Test", "test", test.as_str(), 4, 3);
        add_builtin("Release", "release", release.as_str(), 5, 4);
        add_builtin("Clean", "clean", clean.as_str(), 6, 5);
        add_builtin("Terminal", "terminal", terminal.as_str(), 7, 6);

        let mut custom_index = 0usize;
        for custom in custom_actions.iter() {
            let label = custom.label.as_deref().map(str::trim).unwrap_or_default();
            let command = custom.command.as_deref().map(str::trim).unwrap_or_default();

            if label.is_empty() || command.is_empty() {
                continue;
            }

            let fallback = 100 + custom_index as i32;
            ordered_tasks.push((
                label.to_string(),
                label.to_string(),
                resolved_order(label, label, fallback),
                7 + custom_index,
            ));
            custom_index += 1;
        }

        ordered_tasks.sort_by(
            |(_, key_a, order_a, index_a), (_, key_b, order_b, index_b)| {
                order_a
                    .cmp(order_b)
                    .then(index_a.cmp(index_b))
                    .then(key_a.cmp(key_b))
            },
        );

        let content = html! {{
            <div class="project-status-card">
                <p class="project-status-title">$project</p>
                <p class="project-status-branch">Branch: <code>$branch</code></p>
                <build-status project="$project"></build-status>
                <div class="project-actions">
                <button class="project-action-button" onclick="runAll('$project')">Run</button>

                <rust>
                    for task in ordered_tasks.iter() {
                        let task_label = task.0.as_str();
                        let task_action = task.1.trim();
                        let task_key = task_action.to_ascii_lowercase();

                        match task_key.as_str() {
                            "clean" => {
                                <markup>
                                    <button class="project-action-button" onclick="goTo('/clean?project=$project')">Clean</button>
                                </markup>
                            }
                            "terminal" => {
                                <markup>
                                    <button class="project-action-button" onclick="goTo('/terminal?project=$project')">Terminal</button>
                                </markup>
                            }
                            _ => {
                                let custom_label = task_label.trim();

                                if !custom_label.is_empty() {
                                    <markup>
                                        <button class="project-action-button" onclick="goTo('/custom?project=$(project)&custom_label=$(task_action)')">$custom_label</button>
                                    </markup>
                                }
                            }
                        }
                    }
                </rust>

                    <button class="project-action-button" onclick="goTo('/settings?project=$project')">Settings</button>
                    <button class="project-action-button" onclick="goTo('/history?project=$project')">History</button>
                </div>
            </div>
        }};

        element.set_inner_html(content.as_str());
    }
}

#[callback("goTo")]
fn go_to(url: String) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let url = auth::with_cached_key(url.as_str());
    let _ = window.location().set_href(url.as_str());
}

#[derive(Deserialize)]
struct RunAllResponse {
    ok: bool,
    message: Option<String>,
}

#[callback("runAll")]
async fn run_all(project: String) {
    let endpoint = format!("/run-all?project={}", project);

    let response = match auth::get(endpoint.as_str()).await {
        Ok(response) => response,
        Err(_) => {
            render_build_status_error(project.as_str(), "Failed to run all tasks.");
            return;
        }
    };

    let fallback_message = "Run all has nothing to run.".to_string();
    let request_ok = response.ok();

    let message = match response.text().await {
        Ok(body) => {
            if let Ok(parsed) = serde_json::from_str::<RunAllResponse>(body.as_str()) {
                if parsed.ok {
                    log!("Running all tasks.");
                    return;
                }

                parsed.message.unwrap_or(fallback_message)
            } else if request_ok {
                log!("Running all tasks.");
                return;
            } else if body.trim().is_empty() {
                fallback_message
            } else {
                body
            }
        }
        Err(_) => {
            if request_ok {
                log!("Running all tasks.");
                return;
            }

            fallback_message
        }
    };

    if !message.trim().is_empty() {
        render_build_status_error(project.as_str(), message.as_str());
    }
}

#[derive(Deserialize)]
struct BranchResponse {
    ok: bool,
    branch: String,
    #[serde(default)]
    message: Option<String>,
}

async fn load_settings(project: &str) -> Option<BuildCommands> {
    let endpoint = format!("/load-settings?project={}", project);

    let response = match auth::get(endpoint.as_str()).await {
        Ok(response) => response,
        Err(_) => {
            log!("Failed to load settings.");
            return None;
        }
    };

    let body = match response.text().await {
        Ok(body) => body,
        Err(_) => {
            log!("#save-status", "Failed to read settings response.");
            return None;
        }
    };

    match serde_json::from_str::<BuildCommands>(body.as_str()) {
        Ok(commands) => return Some(commands),
        Err(_) => {
            log!("#save-status", "Failed to parse settings response.");
            return None;
        }
    }
}

async fn load_branch(project: &str) -> String {
    let endpoint = format!("/branch?project={}", js_sys::encode_uri_component(project));

    let response = match auth::get(endpoint.as_str()).await {
        Ok(response) => response,
        Err(_) => {
            log!("Failed to load branch.");
            return "no-git".to_string();
        }
    };

    let body = match response.text().await {
        Ok(body) => body,
        Err(_) => {
            log!("Failed to read branch response.");
            return "no-git".to_string();
        }
    };

    match serde_json::from_str::<BranchResponse>(body.as_str()) {
        Ok(branch_response) if branch_response.ok => {
            let branch = branch_response.branch.trim();
            if branch.is_empty() {
                "no-git".to_string()
            } else {
                branch.to_string()
            }
        }
        Ok(branch_response) => {
            if let Some(message) = branch_response.message {
                log!("Branch unavailable", message);
            }

            let branch = branch_response.branch.trim();
            if branch.is_empty() {
                "no-git".to_string()
            } else {
                branch.to_string()
            }
        }
        Err(_) => {
            log!("Failed to parse branch response.");
            "no-git".to_string()
        }
    }
}
