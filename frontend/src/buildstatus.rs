use futures_util::{
    StreamExt,
    future::{AbortHandle, Abortable},
};
use gloo_net::{
    http::Request,
    websocket::{Message, futures::WebSocket},
};
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
    task_key: String,
    time: String,
    href: String,
    message: Option<String>,
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
    raw_file: String,
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

        entry.project = project;

        if let Some(handle) = entry.abort_handle.take() {
            handle.abort();
        }

        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        entry.abort_handle = Some(abort_handle);
        abort_registration
    };

    connect_task_ws(id, abort_registration);
}

fn connect_task_ws(id: String, abort_registration: futures_util::future::AbortRegistration) {
    spawn_local(async move {
        let ws = WebSocket::open("ws://127.0.0.1:2003/task");

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

                let local_time = local_time_label(status.time.as_str());

                let is_log_href = status.href.starts_with("/log");

                let mut label = if status.ok && !status.running && is_log_href {
                    if local_time.is_empty() {
                        "run-all".to_string()
                    } else {
                        format!("run-all - {}", local_time)
                    }
                } else {
                    let action = task_action_label(status.task_key.as_str());
                    let outcome = if status.running {
                        "running"
                    } else if status.ok {
                        "done"
                    } else {
                        "failed"
                    };

                    format!("{} {} {}", action, outcome, local_time)
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
        let endpoint = format!(
            "/log/latest?project={}",
            js_sys::encode_uri_component(project.as_str())
        );

        let Ok(response) = Request::get(endpoint.as_str()).send().await else {
            return;
        };

        let Ok(payload) = response.text().await else {
            return;
        };

        let Ok(latest) = serde_json::from_str::<LatestLogResponse>(payload.as_str()) else {
            return;
        };

        if !latest.found {
            render_status(id.as_str(), "", "");
            return;
        }

        let Some(metadata) = latest.metadata else {
            return;
        };

        let time_value = metadata.finished_at.unwrap_or_default();
        let local_time = if time_value.is_empty() {
            "".to_string()
        } else {
            local_time_label(time_value.as_str())
        };

        let label = if metadata.status == "success" {
            if local_time.is_empty() {
                "✓ done".to_string()
            } else {
                format!("✓ done {}", local_time)
            }
        } else {
            if local_time.is_empty() {
                "✗ failed".to_string()
            } else {
                format!("✗ failed {}", local_time)
            }
        };

        let raw_file_name = metadata
            .raw_file
            .rsplit('/')
            .next()
            .and_then(|value| value.rsplit('\\').next())
            .unwrap_or(metadata.raw_file.as_str());

        let href = if raw_file_name.ends_with(".raw") {
            format!("/log?file={}", js_sys::encode_uri_component(raw_file_name))
        } else {
            format!(
                "/log?project={}",
                js_sys::encode_uri_component(project.as_str())
            )
        };

        render_status(id.as_str(), href.as_str(), label.as_str());
    });
}

fn task_action_label(task_key: &str) -> &str {
    task_key
        .split_once('-')
        .map(|(action, _)| action)
        .unwrap_or(task_key)
}

fn local_time_label(input: &str) -> String {
    let date = Date::new(&input.into());

    if date.get_time().is_nan() {
        return input.to_string();
    }

    let value = date.to_locale_string("en-GB", &wasm_bindgen::JsValue::UNDEFINED);
    value.as_string().unwrap_or_else(|| input.to_string())
}

fn render_status(id: &str, href: &str, label: &str) {
    let element = match get_element_by_id(id) {
        Some(element) => element,
        None => {
            log!("No root element for build status");
            return;
        }
    };

    if href.trim().is_empty() || label.trim().is_empty() {
        element.set_inner_html("");
        return;
    }

    let content = html! {{
        <a href="$href">$label</a>
    }};

    element.set_inner_html(content.as_str());
}
