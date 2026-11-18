use crate::auth;
use futures_util::{
    StreamExt,
    future::{AbortHandle, Abortable},
};
use gloo_net::websocket::{Message, futures::WebSocket};
use js_sys::Date;
use kaja_web::prelude::*;
use serde::Deserialize;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};
use wasm_bindgen_futures::spawn_local;

thread_local! {
    static BUILD_STATUS_COMPONENT_IDS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static BUILD_STATUS_COMPONENT_BY_PROJECT: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

#[derive(Debug)]
struct BuildStatusConnection {
    project: String,
    abort_handle: Option<AbortHandle>,
}

#[derive(Deserialize, Debug)]
struct TaskRunningStatus {
    ok: bool,
    running: bool,
    #[serde(default)]
    project: String,

    #[serde(default)]
    task_key: String,
    #[serde(default)]
    action: String,
    time: String,
    href: String,
    message: Option<String>,
    #[serde(default)]
    branch: String,
}

#[derive(Deserialize, Debug)]
struct LatestLogResponse {
    found: bool,
    metadata: Option<LatestLogMetadata>,
}

#[derive(Deserialize, Debug)]
struct LatestLogMetadata {
    status: String,
    finished_at: Option<String>,
    #[serde(default)]
    started_at: String,
    raw_file: String,
    #[serde(default)]
    branch: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    task_key: String,
}

#[component("build-status")]
struct BuildStatus {
    #[attribute]
    pub project: String,

    connection: Option<BuildStatusConnection>,
}

impl Component for BuildStatusComponent {
    fn connected(&self) {
        let id = self.get_id();
        log!("Connected:", &id);
        self.render();

        let project = self.get_attribute::<String>("project").unwrap_or_default();

        BUILD_STATUS_COMPONENT_IDS.with(|ids| {
            ids.borrow_mut().insert(id.clone());
        });

        let previous_id = BUILD_STATUS_COMPONENT_BY_PROJECT
            .with(|by_project| by_project.borrow_mut().insert(project.clone(), id.clone()));

        if let Some(previous_id) = previous_id {
            if previous_id != id {
                remove_build_status_connection(previous_id.as_str());
            }
        }

        ensure_build_status_page_handlers();
        load_latest_build_status(id.clone(), project.clone());
        start_build_status_connection(id, project);
    }

    fn disconnected(&self) {
        let id = self.get_id();
        log!("Disconnected:", &id);
        remove_build_status_connection(id.as_str());
    }

    fn attribute_changed(&self, name: &str, old_value: &str, _new_value: &str) {
        self.render();

        if name != "project" {
            return;
        }

        let id = self.get_id();
        let project = self.get_attribute::<String>("project").unwrap_or_default();

        let previous_id = BUILD_STATUS_COMPONENT_BY_PROJECT.with(|by_project| {
            let mut by_project = by_project.borrow_mut();

            if !old_value.is_empty() {
                by_project.remove(old_value);
            }

            by_project.insert(project.clone(), id.clone())
        });

        if let Some(previous_id) = previous_id {
            if previous_id != id {
                remove_build_status_connection(previous_id.as_str());
            }
        }

        load_latest_build_status(id.clone(), project.clone());
        start_build_status_connection(id, project);
    }
}

impl BuildStatusComponent {
    fn render(&self) {
        let id = self.get_id();
        render_status(id.as_str(), "", "");
    }
}

fn ensure_build_status_page_handlers() {
    let Some(window) = web_sys::window() else {
        return;
    };

    let Some(pause_connections) = get_callback("pauseBuildStatusConnections") else {
        log!("Missing pauseBuildStatusConnections callback.");
        return;
    };

    let Some(resume_connections) = get_callback("resumeBuildStatusConnections") else {
        log!("Missing resumeBuildStatusConnections callback.");
        return;
    };

    let _ = window.remove_event_listener_with_callback("pagehide", &pause_connections);
    let _ = window.remove_event_listener_with_callback("pageshow", &resume_connections);

    let pagehide = window
        .add_event_listener_with_callback("pagehide", &pause_connections)
        .is_ok();

    let pageshow = window
        .add_event_listener_with_callback("pageshow", &resume_connections)
        .is_ok();

    if !pagehide || !pageshow {
        log!("Failed to register build status page lifecycle handlers.");
    }
}

#[callback("pauseBuildStatusConnections")]
fn pause_build_status_connections(_event: web_sys::Event) {
    for id in build_status_component_ids() {
        pause_build_status_connection(id.as_str());
    }
}

#[callback("resumeBuildStatusConnections")]
fn resume_build_status_connections(_event: web_sys::Event) {
    for id in build_status_component_ids() {
        let Some(component) = BuildStatusComponent::get_component(id.as_str()) else {
            continue;
        };

        let project: String = component.get_attribute("project").unwrap_or_default();
        load_latest_build_status(id.clone(), project.clone());
        start_build_status_connection(id, project);
    }
}

fn build_status_component_ids() -> Vec<String> {
    BUILD_STATUS_COMPONENT_IDS.with(|ids| ids.borrow().iter().cloned().collect())
}

fn pause_build_status_connection(id: &str) {
    let Some(component) = BuildStatusComponent::get_component(id) else {
        return;
    };

    let mut component = component.borrow_mut();

    if let Some(connection) = component.connection.as_mut() {
        if let Some(handle) = connection.abort_handle.take() {
            handle.abort();
        }
    }
}

fn remove_build_status_connection(id: &str) {
    BUILD_STATUS_COMPONENT_IDS.with(|ids| {
        ids.borrow_mut().remove(id);
    });

    BUILD_STATUS_COMPONENT_BY_PROJECT.with(|by_project| {
        by_project
            .borrow_mut()
            .retain(|_, component_id| component_id != id);
    });

    let Some(component) = BuildStatusComponent::get_component(id) else {
        return;
    };

    let mut component = component.borrow_mut();

    if let Some(connection) = component.connection.as_mut() {
        if let Some(handle) = connection.abort_handle.take() {
            handle.abort();
        }
    }
}

fn start_build_status_connection(id: String, project: String) {
    let Some(component) = BuildStatusComponent::get_component(id.as_str()) else {
        return;
    };

    let abort_registration = {
        let mut component = component.borrow_mut();

        let entry = component
            .connection
            .get_or_insert_with(|| BuildStatusConnection {
                project: project.clone(),
                abort_handle: None,
            });

        entry.project = project.clone();

        if let Some(handle) = entry.abort_handle.take() {
            handle.abort();
        }

        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        entry.abort_handle = Some(abort_handle);
        abort_registration
    };

    connect_task_ws(id, project, abort_registration);
}

fn connect_task_ws(
    id: String,
    project: String,
    abort_registration: futures_util::future::AbortRegistration,
) {
    spawn_local(async move {
        let endpoint = format!(
            "ws://127.0.0.1:2003/task?project={}",
            js_sys::encode_uri_component(project.as_str())
        );

        let endpoint = auth::with_key(endpoint.as_str()).await.unwrap_or(endpoint);

        let ws = WebSocket::open(endpoint.as_str());

        let Ok(ws) = ws else {
            log!("Failed to open task websocket");
            return;
        };

        let (_write, mut read) = ws.split();

        let read_messages = async move {
            while let Some(message) = read.next().await {
                let Ok(Message::Text(text)) = message else {
                    continue;
                };

                log!("task status", &text);

                let Ok(status) = serde_json::from_str::<TaskRunningStatus>(text.as_str()) else {
                    continue;
                };

                if !project.is_empty() && status.project != project {
                    continue;
                }

                let local_time = local_time_label(status.time.as_str());

                let is_log_href = status.href.starts_with("/log");

                let branch = normalized_branch(status.branch.as_str());
                let task = normalized_task(status.action.as_str(), status.task_key.as_str());
                let mut label = if status.running {
                    format_status_label(
                        "running",
                        branch.as_str(),
                        task.as_str(),
                        local_time.as_str(),
                    )
                } else if status.ok {
                    format_status_label("done", branch.as_str(), task.as_str(), local_time.as_str())
                } else {
                    format_status_label(
                        "failed",
                        branch.as_str(),
                        task.as_str(),
                        local_time.as_str(),
                    )
                };

                if !is_log_href {
                    if let Some(message) = status.message.filter(|value| !value.trim().is_empty()) {
                        label.push_str(" — ");
                        label.push_str(message.as_str());
                    }
                }

                render_status(id.as_str(), status.href.as_str(), label.as_str());
            }
        };

        let _ = Abortable::new(read_messages, abort_registration).await;
    });
}

fn load_latest_build_status(id: String, project: String) {
    if project.is_empty() {
        return;
    }

    spawn_local(async move {
        let encoded_project = js_sys::encode_uri_component(project.as_str()).to_string();
        let run_all_endpoint = format!("/log/latest?project={}&action=run-all", encoded_project);
        let endpoint = format!("/log/latest?project={}", encoded_project);

        let run_all_metadata = fetch_latest_metadata(run_all_endpoint.as_str()).await;
        let latest_metadata = fetch_latest_metadata(endpoint.as_str()).await;

        let Some(metadata) = select_latest_metadata(run_all_metadata, latest_metadata) else {
            render_status(id.as_str(), "", "");
            return;
        };

        render_status_from_metadata(id.as_str(), project.as_str(), metadata);
    });
}

fn select_latest_metadata(
    run_all: Option<LatestLogMetadata>,
    latest: Option<LatestLogMetadata>,
) -> Option<LatestLogMetadata> {
    match (run_all, latest) {
        (Some(run_all), Some(latest)) => {
            if run_all.status.trim().eq_ignore_ascii_case("running") {
                return Some(run_all);
            }

            let run_all_time = metadata_time_ms(&run_all);
            let latest_time = metadata_time_ms(&latest);

            if run_all_time.is_finite() && latest_time.is_finite() {
                if run_all_time >= latest_time {
                    Some(run_all)
                } else {
                    Some(latest)
                }
            } else if run_all_time.is_finite() {
                Some(run_all)
            } else {
                Some(latest)
            }
        }
        (Some(run_all), None) => Some(run_all),
        (None, Some(latest)) => Some(latest),
        (None, None) => None,
    }
}

fn metadata_time_ms(metadata: &LatestLogMetadata) -> f64 {
    let value = metadata
        .finished_at
        .as_deref()
        .unwrap_or(metadata.started_at.as_str());

    if value.trim().is_empty() {
        return f64::NAN;
    }

    Date::new(&value.into()).get_time()
}

async fn fetch_latest_metadata(endpoint: &str) -> Option<LatestLogMetadata> {
    let Ok(response) = auth::get(endpoint).await else {
        return None;
    };

    let Ok(payload) = response.text().await else {
        return None;
    };

    let Ok(latest) = serde_json::from_str::<LatestLogResponse>(payload.as_str()) else {
        return None;
    };

    if !latest.found {
        return None;
    }

    latest.metadata
}

fn render_status_from_metadata(id: &str, project: &str, metadata: LatestLogMetadata) {
    let time_value = metadata.finished_at.unwrap_or_default();
    let local_time = if time_value.is_empty() {
        "".to_string()
    } else {
        local_time_label(time_value.as_str())
    };

    let branch = normalized_branch(metadata.branch.as_str());
    let task = normalized_task(metadata.action.as_str(), metadata.task_key.as_str());

    let status_label = match metadata.status.trim().to_ascii_lowercase().as_str() {
        "success" | "ok" | "done" => "done",
        "running" => "running",
        _ => "failed",
    };

    let label = format_status_label(
        status_label,
        branch.as_str(),
        task.as_str(),
        local_time.as_str(),
    );

    let raw_file_name = metadata
        .raw_file
        .rsplit('/')
        .next()
        .and_then(|value| value.rsplit('\\').next())
        .unwrap_or(metadata.raw_file.as_str());

    let href = if raw_file_name.ends_with(".raw") {
        format!("/log?file={}", js_sys::encode_uri_component(raw_file_name))
    } else {
        format!("/log?project={}", js_sys::encode_uri_component(project))
    };

    render_status(id, href.as_str(), label.as_str());
}

fn normalized_branch(input: &str) -> String {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        "no-git".to_string()
    } else {
        trimmed.to_string()
    }
}

fn format_status_label(status: &str, branch: &str, task: &str, local_time: &str) -> String {
    let icon = match status {
        "done" => "✓",
        "failed" => "✗",
        "running" => "⟳",
        _ => "",
    };

    if local_time.is_empty() {
        format!("{} {} {} {}", icon, status, branch, task)
    } else {
        format!("{} {} {} {} {}", icon, status, branch, task, local_time)
    }
}

fn normalized_task(action: &str, task_key: &str) -> String {
    let action = action.trim();

    let raw = if !action.is_empty() && action != "log" {
        action
    } else if task_key.trim() == "all-done" {
        "run-all"
    } else if !task_key.trim().is_empty() {
        task_key
            .split_once('-')
            .map(|(name, _)| name)
            .unwrap_or(task_key)
            .trim()
    } else {
        "task"
    };

    raw.replace(['-', '_'], " ")
}

fn local_time_label(input: &str) -> String {
    let date = Date::new(&input.into());

    if date.get_time().is_nan() {
        return input.to_string();
    }

    let year = date.get_full_year();
    let month = date.get_month() + 1;
    let day = date.get_date();
    let hour = date.get_hours();
    let minute = date.get_minutes();
    let second = date.get_seconds();

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hour, minute, second
    )
}

pub(crate) fn render_build_status_error(project: &str, message: &str) {
    if project.trim().is_empty() || message.trim().is_empty() {
        return;
    }

    let component_id = BUILD_STATUS_COMPONENT_BY_PROJECT
        .with(|by_project| by_project.borrow().get(project).cloned());

    let Some(component_id) = component_id else {
        return;
    };

    render_status(component_id.as_str(), "", message);
}

fn render_status(id: &str, href: &str, label: &str) {
    let element = match get_element_by_id(id) {
        Some(element) => element,
        None => {
            log!("No root element for build status");
            return;
        }
    };

    if label.trim().is_empty() {
        element.set_inner_html("&nbsp;");
        return;
    }

    if href.trim().is_empty() {
        let content = html! {{
            <span>$label</span>
        }};

        element.set_inner_html(content.as_str());
        return;
    }

    let content = html! {{
        <a href="$href">$label</a>
    }};

    element.set_inner_html(content.as_str());
}
