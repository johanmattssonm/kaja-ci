use crate::rest::{Action, ActionResponse};

use super::color;
use futures_channel::mpsc;
use futures_util::{SinkExt, StreamExt};
use gloo_net::{
    http::Request,
    websocket::{Message, futures::WebSocket},
};
use kaja_web::prelude::*;
use serde::Deserialize;
use wasm_bindgen::{JsCast, prelude::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};

#[component("console-component")]
struct Console {
    #[attribute]
    project: String,

    #[attribute]
    file: String,

    #[attribute]
    action: String,

    #[attribute]
    connect: String,

    #[attribute]
    task_key: String,

    #[attribute]
    has_connected: bool,

    #[attribute]
    command: String,

    #[attribute]
    auto_start: bool,

    // internal render state for terminal, not an attribute
    ansi_render_state: Option<color::AnsiRenderState>,
}

impl Component for ConsoleComponent {
    fn connected(&self) {
        let id = self.get_id();
        let action: String = self.get_attribute("action").unwrap_or_default();
        let project: String = self.get_attribute("project").unwrap_or_default();
        let file: String = self.get_attribute("file").unwrap_or_default();

        log!("Connected:", &id);
        self.render();
        focus_console_output(&id);

        if action == "terminal" {
            ensure_abort_on_exit_handlers();
        }

        if action.is_empty() {
            return;
        }

        if action == "terminal" {
            if project.is_empty() {
                return;
            }

            spawn_local(async move {
                connect_terminal(project).await;
            });
            return;
        }

        if action == "log" {
            if project.is_empty() && file.is_empty() {
                return;
            }

            setup_log_output_shortcuts(&id);
            load_latest_raw_log(id, project, file);
            return;
        }

        if project.is_empty() {
            return;
        }

        let command: String = self.get_attribute("command").unwrap_or_default();
        let auto_start: bool = self.get_attribute("auto_start").unwrap_or(false);

        if auto_start && !command.is_empty() {
            spawn_local(async move {
                start_and_connect_console_view_with_command(
                    action.as_str(),
                    project.as_str(),
                    command.as_str(),
                )
                .await;
            });
            return;
        }

        spawn_local(async move {
            ensure_builder_task_connected(action.as_str(), project.as_str()).await;
        });
    }

    fn disconnected(&self) {
        let id = self.get_id();
        log!("Disconnected:", &id);

        let action: String = self.get_attribute("action").unwrap_or_default();

        if action != "terminal" {
            return;
        }

        let task_key: String = self.get_attribute("task_key").unwrap_or_default();

        if task_key.is_empty() {
            return;
        }

        if abort_task_with_beacon(Some(task_key.as_str()), None) {
            return;
        }

        spawn_local(async move {
            if !abort_task(Some(task_key.as_str()), None).await {
                log!("Failed to abort task on console disconnect.", task_key);
            }
        });
    }

    fn attribute_changed(&self, name: &str, _old_value: &str, new_value: &str) {
        if name == "task_key" {
            self.render();
            focus_console_output(&self.get_id());
            return;
        }

        if name == "connect" {
            self.handle_connect(new_value);
        }
    }
}

fn focus_console_output(component_id: &str) {
    let Some(element) = get_element_by_id(&format!("{}-output", component_id)) else {
        return;
    };

    let Ok(output) = element.dyn_into::<web_sys::HtmlElement>() else {
        return;
    };

    let _ = output.focus();
}

impl ConsoleComponent {
    fn handle_connect(&self, new_value: &str) {
        let component_id = self.get_id();

        if new_value.is_empty() {
            return;
        }

        let has_connected: bool = self.get_attribute("has_connected").unwrap_or(false);

        if !has_connected {
            let output_id = format!("{}-output", component_id);
            self.with_mut(|context| {
                context.ansi_render_state = None;
            });

            if let Some(output) = get_element_by_id(output_id.as_str()) {
                output.set_class_name("terminal-output");
                output.set_inner_html("");
            }
        }

        let task_key = new_value.to_string();

        spawn_local(async move {
            connect_console_view(component_id, task_key).await;
        });
    }

    fn render(&self) {
        let id = self.get_id();
        let element = match get_element_by_id(&id) {
            Some(element) => element,
            None => {
                log!("No root element for build component");
                return;
            }
        };

        let output_id = format!("{}-output", id);
        let existing_output = get_element_by_id(output_id.as_str())
            .map(|output| (output.inner_html(), output.class_name()));

        let project: String = self.get_attribute("project").unwrap_or_default();
        let action: String = self.get_attribute("action").unwrap_or_default();
        let task_key: String = self.get_attribute("task_key").unwrap_or_default();
        let has_connected: bool = self.get_attribute("has_connected").unwrap_or(false);

        let show_connect_button = action == "terminal" && task_key.is_empty() && !has_connected;
        let preserve_previous_output = action == "terminal" && has_connected;

        if !preserve_previous_output {
            self.with_mut(|context| {
                context.ansi_render_state = None;
            });
        }
        element.set_inner_html("");

        let output_class = if show_connect_button {
            "terminal-output terminal-output--waiting"
        } else {
            "terminal-output"
        };

        let initial_status = if action == "log" {
            "Log"
        } else {
            "Disconnected"
        };

        let content = html! {{
            <h1 id="status">$(initial_status)</h1>
            <p>Project Folder: $(project)</p>
            <pre id="$(id)-output" class="$(output_class)" tabindex="0">
                <rust>
                    if show_connect_button {
                        <markup>
                            <button class="terminal-connect-button" onclick="connectTerminal('$project')">Connect</button>
                        </markup>
                    }
                </rust>
            </pre>
        }};

        element.set_inner_html(content.as_str());

        if preserve_previous_output {
            if let Some((html, class_name)) = existing_output {
                if let Some(output) = get_element_by_id(output_id.as_str()) {
                    output.set_inner_html(html.as_str());

                    if class_name.is_empty() {
                        output.set_class_name("terminal-output");
                    } else {
                        output.set_class_name(class_name.as_str());
                    }
                }
            }
        }
    }
}

fn url_encode(value: &str) -> String {
    js_sys::encode_uri_component(value).into()
}

fn load_latest_raw_log(component_id: String, project: String, file: String) {
    spawn_local(async move {
        let endpoint = if !file.is_empty() {
            format!("/log/raw?file={}", url_encode(file.as_str()))
        } else if !project.is_empty() {
            format!("/log/raw?project={}", url_encode(project.as_str()))
        } else {
            try_inner_text("#status", "No project or log file selected.");
            return;
        };

        let response = match Request::get(endpoint.as_str()).send().await {
            Ok(response) => response,
            Err(_) => {
                try_inner_text("#status", "Failed to load log output.");
                return;
            }
        };

        if !response.ok() {
            try_inner_text("#status", "No log history found.");
            return;
        }

        let output = match response.text().await {
            Ok(output) => output,
            Err(_) => {
                try_inner_text("#status", "Failed to parse log output.");
                return;
            }
        };

        let output_id = format!("{}-output", component_id);

        if let Some(element) = get_element_by_id(output_id.as_str()) {
            element.set_class_name("terminal-output");

            if let Some(component) = ConsoleComponent::get_component(component_id.as_str()) {
                let rendered = {
                    let mut context = component.borrow_mut();
                    context.ansi_render_state = None;
                    color::append_ansi_chunk(&mut context.ansi_render_state, output.as_str(), None)
                };

                element.set_inner_html(rendered.as_str());
            } else {
                element.set_text_content(Some(output.as_str()));
            }
        }

        try_inner_text("#status", "Log Output");
    });
}

fn abort_endpoint(task_key: Option<&str>, project: Option<&str>) -> Option<String> {
    let mut query = Vec::new();

    if let Some(task_key) = task_key.filter(|value| !value.is_empty()) {
        query.push(format!("task_key={}", url_encode(task_key)));
    }

    if let Some(project) = project.filter(|value| !value.is_empty()) {
        query.push(format!("project={}", url_encode(project)));
    }

    if query.is_empty() {
        return None;
    }

    Some(format!("/abort?{}", query.join("&")))
}

fn abort_task_with_beacon(task_key: Option<&str>, project: Option<&str>) -> bool {
    let Some(endpoint) = abort_endpoint(task_key, project) else {
        return false;
    };

    let Some(window) = web_sys::window() else {
        return false;
    };

    window
        .navigator()
        .send_beacon(endpoint.as_str())
        .unwrap_or(false)
}

fn abort_console_task_with_beacon() -> bool {
    let task_key = get_attribute("console-component", "task_key");

    match task_key {
        Some(task_key) => abort_task_with_beacon(Some(task_key.as_str()), None),
        None => false,
    }
}

#[callback("abortConsoleTaskOnExit")]
fn abort_console_task_on_exit(_event: web_sys::Event) {
    let _ = abort_console_task_with_beacon();
}

fn ensure_abort_on_exit_handlers() {
    let Some(window) = web_sys::window() else {
        return;
    };

    let Some(abort_on_exit) = get_callback("abortConsoleTaskOnExit") else {
        log!("Missing abortConsoleTaskOnExit callback.");
        return;
    };

    let _ = window.remove_event_listener_with_callback("pagehide", &abort_on_exit);
    let _ = window.remove_event_listener_with_callback("beforeunload", &abort_on_exit);

    let pagehide = window
        .add_event_listener_with_callback("pagehide", &abort_on_exit)
        .is_ok();

    let beforeunload = window
        .add_event_listener_with_callback("beforeunload", &abort_on_exit)
        .is_ok();

    if !pagehide && !beforeunload {
        log!("Failed to register abort handlers for page exit.");
    }
}

async fn abort_task(task_key: Option<&str>, project: Option<&str>) -> bool {
    let Some(endpoint) = abort_endpoint(task_key, project) else {
        return false;
    };

    match Request::get(endpoint.as_str()).send().await {
        Ok(response) => response.ok(),
        Err(_) => false,
    }
}

fn console_task_has_exited() -> bool {
    get_element("console-component")
        .and_then(|component| component.get_attribute("task_exited"))
        .is_some_and(|value| value == "true")
}

#[callback("abort")]
async fn abort_current_task(project: String) {
    if console_task_has_exited() {
        return;
    }

    let task_key: Option<String> =
        get_element("console-component").and_then(|component| component.get_attribute("task_key"));
    let task_key = task_key.filter(|value| !value.is_empty());

    let aborted = if let Some(task_key) = task_key {
        abort_task(Some(task_key.as_str()), None).await
    } else {
        abort_task(None, Some(project.as_str())).await
    };

    if !aborted {
        try_inner_text("#save-status", "Failed to abort task.");
    } else if let Some(component) = get_element("console-component") {
        if component.set_attribute("task_key", "").is_err() {
            log!("Failed to clear task_key attribute on console component.");
        }
    }
}

#[derive(Deserialize)]
struct RunTaskResponse {
    ok: bool,
    task: Option<String>,
    message: Option<String>,
}

async fn start_task(project: &str, action: &str, command: Option<&str>) -> Option<String> {
    let mut endpoint = format!(
        "/run?project={}&task_key={}",
        url_encode(project),
        url_encode(action)
    );

    if let Some(command) = command.filter(|value| !value.is_empty()) {
        endpoint.push_str("&command=");
        endpoint.push_str(url_encode(command).as_str());
    }

    let response = match Request::get(endpoint.as_str()).send().await {
        Ok(response) => response,
        Err(err) => {
            log!("Failed to start task", format!("{err:?}"));
            return None;
        }
    };

    let body = match response.text().await {
        Ok(body) => body,
        Err(err) => {
            log!("Failed to read /run response", format!("{err:?}"));
            return None;
        }
    };

    let payload: RunTaskResponse = match serde_json::from_str(body.as_str()) {
        Ok(payload) => payload,
        Err(err) => {
            log!("Invalid /run response", format!("{err:?}"));
            return None;
        }
    };

    if payload.ok {
        if let Some(task) = payload.task {
            return Some(task);
        }

        log!("Missing task key in /run response");
        return None;
    }

    if let Some(message) = payload.message {
        log!("/run failed", &message);

        if let Some(running_task_key) = payload.task {
            render_running_task_status(message.as_str(), running_task_key.as_str(), project);
        } else {
            try_inner_text("#status", message.as_str());
        }
    } else {
        log!("/run failed");
        try_inner_text("#status", "Failed to start task");
    }

    None
}

fn render_running_task_status(message: &str, running_task_key: &str, project: &str) {
    let Some(status) = get_element("#status") else {
        return;
    };

    let href = task_href(running_task_key, project);
    let content = html! {{
        There is already a <a href="$href">task</a> running.
    }};

    status.set_inner_html(content.as_str());
}

fn task_href(task_key: &str, project: &str) -> String {
    let action = task_key
        .split_once('-')
        .map(|(action, _)| action)
        .unwrap_or(task_key);

    match action {
        "git" | "fetch" => format!("/fetch?project={project}"),
        "build" => format!("/build?project={project}"),
        "clone" => format!("/clone?project={project}"),
        "test" => format!("/test?project={project}"),
        "release" => format!("/release?project={project}"),
        "clean" => format!("/clean?project={project}"),
        "terminal" => format!("/terminal?project={project}"),
        custom => format!("/custom?project={project}&custom_label={custom}"),
    }
}

#[callback("connectTerminal")]
async fn connect_terminal(project: String) {
    let Some(console) = get_element("console-component") else {
        log!("No console component found.");
        return;
    };

    let existing_task_key = console
        .get_attribute("task_key")
        .filter(|value| !value.is_empty());

    if let Some(task_key) = existing_task_key {
        if !abort_task(Some(task_key.as_str()), None).await {
            try_inner_text("#save-status", "Failed to abort existing terminal session.");
            return;
        }
    }

    let Some(task_key) = start_task(project.as_str(), "terminal", None).await else {
        log!("Failed to start terminal.");
        return;
    };

    let console_id = console
        .get_attribute("id")
        .unwrap_or_else(|| "console-component".to_string());

    if console
        .set_attribute("task_key", task_key.as_str())
        .is_err()
    {
        log!("Failed to set task_key attribute on terminal console.");
    }

    if console.set_attribute("task_exited", "false").is_err() {
        log!("Failed to set task_exited attribute on terminal console.");
    }

    if console.set_attribute("connect", task_key.as_str()).is_err() {
        log!("Failed to set connect attribute on terminal console.");
    }

    if console.set_attribute("has_connected", "true").is_err() {
        log!("Failed to set has_connected attribute on terminal console.");
    }

    focus_console_output(&console_id);
}

#[callback("startTask")]
async fn start_task_from_nav(project: String, action: String) {
    start_and_connect_console_view(action.as_str(), project.as_str()).await;
}

#[callback("startTaskWithCommand")]
async fn start_task_with_command_from_nav(project: String, action: String, command: String) {
    start_and_connect_console_view_with_command(
        action.as_str(),
        project.as_str(),
        command.as_str(),
    )
    .await;
}

fn build_task_key(project: &str, action: &str) -> String {
    format!("{}-{}", action, project)
}

#[derive(Deserialize)]
struct StatusResponse {
    project: String,
    task: String,
}

async fn fetch_running_builder_task_key(action: &str, project: &str) -> Option<String> {
    let response = Request::get("/status").send().await.ok()?;
    let body = response.text().await.ok()?;
    let status: StatusResponse = serde_json::from_str(body.as_str()).ok()?;

    if status.task.is_empty() || status.project.is_empty() || status.task == "terminal" {
        return None;
    }

    if status.task != action || status.project != project {
        return None;
    }

    Some(build_task_key(
        status.project.as_str(),
        status.task.as_str(),
    ))
}

async fn ensure_builder_task_connected(action: &str, project: &str) {
    let Some(component) = get_element("console-component") else {
        return;
    };

    let id = component
        .get_attribute("id")
        .unwrap_or_else(|| "console-component".to_string());

    if let Some(task_key) = fetch_running_builder_task_key(action, project).await {
        let _ = component.set_attribute("connect", "");

        if component
            .set_attribute("task_key", task_key.as_str())
            .is_err()
        {
            log!("Failed to set task_key attribute on console component.");
        }

        if component.set_attribute("task_exited", "false").is_err() {
            log!("Failed to set task_exited attribute on console component.");
        }

        if component
            .set_attribute("connect", task_key.as_str())
            .is_err()
        {
            log!("Failed to set connect attribute on console component.");
        }

        focus_console_output(&id);
        return;
    }

    start_and_connect_console_view(action, project).await;
}

async fn start_and_connect_console_view(action: &str, project: &str) {
    start_and_connect_console_view_with_command(action, project, "").await;
}

async fn start_and_connect_console_view_with_command(action: &str, project: &str, command: &str) {
    let command = (!command.is_empty()).then_some(command);
    let Some(task_key) = start_task(project, action, command).await else {
        return;
    };

    let Some(component) = get_element("console-component") else {
        log!("No console component found.");
        return;
    };

    let id = component
        .get_attribute("id")
        .unwrap_or_else(|| "console-component".to_string());

    let _ = component.set_attribute("connect", "");

    if component
        .set_attribute("task_key", task_key.as_str())
        .is_err()
    {
        log!("Failed to set task_key attribute on console component.");
    }

    if component.set_attribute("task_exited", "false").is_err() {
        log!("Failed to set task_exited attribute on console component.");
    }

    if component
        .set_attribute("connect", task_key.as_str())
        .is_err()
    {
        log!("Failed to set connect attribute on console component.");
    }

    focus_console_output(&id);
}

async fn connect_console_view(id: String, task_key: String) {
    let ws = WebSocket::open("ws://127.0.0.1:2003/runner");

    if ws.is_err() {
        log!("Failed to open websocket");
        try_inner_text("#status", "Disconnected");
        return;
    }

    log!("Connected websocket");

    let (mut ws_write, mut ws_read) = ws.unwrap().split();
    let (input_tx, mut input_rx) = mpsc::unbounded::<String>();

    spawn_local(async move {
        while let Some(message) = input_rx.next().await {
            if let Err(err) = ws_write.send(Message::Text(message)).await {
                log!("Failed to send websocket message", format!("{err:?}"));
                break;
            }
        }
    });

    setup_stdin_handler(&id, input_tx.clone());

    let attach_action = Action {
        action: "attach".to_string(),
        folder: None,
        command: None,
        stdin: None,
        task_key: Some(task_key),
    };

    let json_string = match serde_json::to_string(&attach_action) {
        Ok(json) => json,
        Err(err) => {
            log!("Failed to serialize attach action", format!("{err:?}"));
            return;
        }
    };

    if let Err(err) = input_tx.unbounded_send(json_string) {
        log!("Failed to enqueue attach action", format!("{err:?}"));
        return;
    }

    while let Some(message) = ws_read.next().await {
        match message {
            Ok(Message::Text(text)) => {
                process_build_output(id.as_str(), text);
            }
            Ok(Message::Bytes(_)) => {
                log!("Expecting string got binary data.");
            }
            Err(err) => {
                log!("WebSocket receive error", format!("{err:?}"));
                break;
            }
        }
    }

    try_inner_text("#status", "Disconnected");
}

fn single_char_key(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let ch = chars.next()?;

    if chars.next().is_some() {
        None
    } else {
        Some(ch)
    }
}

fn key_to_terminal_input(event: &web_sys::KeyboardEvent) -> Option<String> {
    let key = event.key();

    if event.ctrl_key() && event.shift_key() {
        if let Some(ch) = single_char_key(&key) {
            let ch = ch.to_ascii_lowercase();
            if ch == 'c' || ch == 'v' {
                return None;
            }
        }
    }

    if event.ctrl_key() {
        if let Some(ch) = single_char_key(&key) {
            let ch = ch.to_ascii_lowercase();
            if ch.is_ascii_lowercase() {
                let code = (ch as u8) - b'a' + 1;
                return Some((code as char).to_string());
            }
        }

        return match key.as_str() {
            "[" => Some("\u{1b}".to_string()),
            _ => None,
        };
    }

    match key.as_str() {
        "Enter" => Some("\r".to_string()),
        "Backspace" => Some("\u{7f}".to_string()),
        "Tab" => Some("\t".to_string()),
        "Escape" => Some("\u{1b}".to_string()),
        "ArrowUp" => Some("\u{1b}[A".to_string()),
        "ArrowDown" => Some("\u{1b}[B".to_string()),
        "ArrowRight" => Some("\u{1b}[C".to_string()),
        "ArrowLeft" => Some("\u{1b}[D".to_string()),
        "Home" => Some("\u{1b}[H".to_string()),
        "End" => Some("\u{1b}[F".to_string()),
        "PageUp" => Some("\u{1b}[5~".to_string()),
        "PageDown" => Some("\u{1b}[6~".to_string()),
        "Delete" => Some("\u{1b}[3~".to_string()),
        key if single_char_key(key).is_some() => Some(key.to_string()),
        _ => None,
    }
}

fn send_stdin(stdin: String, input_tx: &mpsc::UnboundedSender<String>) {
    let action = Action {
        action: "STDIN".to_string(),
        command: None,
        folder: None,
        stdin: Some(stdin),
        task_key: None,
    };

    let json = serde_json::to_string(&action);

    if json.is_err() {
        log!("Failed to create json action.");
        return;
    }

    if input_tx.unbounded_send(json.unwrap()).is_err() {
        log!("WebSocket for stdin channel is closed");
    }
}

fn handle_copy_shortcut(event: &web_sys::KeyboardEvent) -> bool {
    if !(event.ctrl_key() && event.shift_key()) {
        return false;
    }

    if !event.key().eq_ignore_ascii_case("c") {
        return false;
    }

    event.prevent_default();
    let _ = js_sys::eval("document.execCommand('copy')");
    true
}

fn handle_terminal_copy_paste_shortcuts(
    event: &web_sys::KeyboardEvent,
    input_tx: &mpsc::UnboundedSender<String>,
) -> bool {
    if handle_copy_shortcut(event) {
        return true;
    }

    if !(event.ctrl_key() && event.shift_key()) {
        return false;
    }

    let key = event.key();
    let is_paste = key.eq_ignore_ascii_case("v");

    if is_paste {
        event.prevent_default();

        let output_tx = input_tx.clone();
        spawn_local(async move {
            let Ok(value) = js_sys::eval(
                "navigator.clipboard && navigator.clipboard.readText ? navigator.clipboard.readText() : null",
            ) else {
                return;
            };

            if value.is_null() || value.is_undefined() {
                return;
            }

            let Ok(promise) = value.dyn_into::<js_sys::Promise>() else {
                return;
            };

            let Ok(clipboard_text) = JsFuture::from(promise).await else {
                return;
            };

            let Some(stdin) = clipboard_text.as_string() else {
                return;
            };

            if stdin.is_empty() {
                return;
            }

            send_stdin(stdin, &output_tx);
        });

        return true;
    }

    false
}

fn setup_log_output_shortcuts(id: &str) {
    let output_id = format!("{}-output", id);

    let output = match get_element_by_id(&output_id) {
        Some(output) => output,
        None => {
            log!("No output box for log view component.");
            return;
        }
    };

    let output_keydown = Closure::<dyn FnMut(_)>::new(move |event: web_sys::KeyboardEvent| {
        let _ = handle_copy_shortcut(&event);
    });

    if output
        .add_event_listener_with_callback("keydown", output_keydown.as_ref().unchecked_ref())
        .is_err()
    {
        log!("Failed to register log output keydown handler");
        return;
    }

    output_keydown.forget();
}

fn setup_stdin_handler(id: &str, input_tx: mpsc::UnboundedSender<String>) {
    let output_id = format!("{}-output", id);

    let output = match get_element_by_id(&output_id) {
        Some(output) => output,
        None => {
            log!("No output box for build component.");
            return;
        }
    };

    let output_tx = input_tx.clone();
    let output_keydown = Closure::<dyn FnMut(_)>::new(move |event: web_sys::KeyboardEvent| {
        if handle_terminal_copy_paste_shortcuts(&event, &output_tx) {
            return;
        }

        let Some(stdin) = key_to_terminal_input(&event) else {
            return;
        };

        event.prevent_default();
        send_stdin(stdin, &output_tx);
    });

    if output
        .add_event_listener_with_callback("keydown", output_keydown.as_ref().unchecked_ref())
        .is_err()
    {
        log!("Failed to register output keydown handler");
        return;
    }

    let focus_output = output.clone();
    let click_focus = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::Event| {
        let _ = focus_output.focus();
    });

    if output
        .add_event_listener_with_callback("click", click_focus.as_ref().unchecked_ref())
        .is_err()
    {
        log!("Failed to register output click handler");
        return;
    }

    output_keydown.forget();
    click_focus.forget();
}

fn process_build_output(component_id: &str, chunk: String) {
    let output_id = format!("{}-output", component_id);

    let element = match get_element_by_id(output_id.as_str()) {
        Some(element) => element,
        None => {
            log!("No output box for build.");
            return;
        }
    };

    let resp: std::result::Result<ActionResponse, _> = serde_json::from_str(&chunk);

    if resp.is_err() {
        log!("Invalid response", chunk);
        return;
    }

    let resp = resp.unwrap();

    if resp.status.is_some() {
        handle_status(component_id, resp);
        return;
    }

    if let Some(stdout) = resp.stdout {
        if resp.action == "SCREEN" {
            let rendered = match (resp.cursor_row, resp.cursor_col) {
                (Some(row), Some(col)) => {
                    color::render_terminal_screen_with_cursor(&stdout, row, col)
                }
                _ => color::render_terminal_screen(&stdout),
            };

            element.set_class_name("terminal-output terminal-mono terminal-fullscreen");
            element.set_inner_html(&rendered);
            return;
        }

        element.set_class_name("terminal-output");

        let Some(component) = ConsoleComponent::get_component(component_id) else {
            log!("No console component found for output.", component_id);
            return;
        };

        let rendered = {
            let mut context = component.borrow_mut();
            color::append_ansi_chunk(&mut context.ansi_render_state, &stdout, resp.cursor_col)
        };

        element.set_inner_html(&rendered);
        element.set_scroll_top(element.scroll_height());
    }
}

fn handle_status(component_id: &str, line: ActionResponse) {
    if line.status.is_none() {
        log!("No status");
        return;
    }

    let status = line.status.unwrap();

    let Some(component) = get_element_by_id(component_id) else {
        log!(
            "No console component found for status update.",
            component_id
        );
        return;
    };

    match status.as_str() {
        "status: RUNNING" => {
            try_inner_text("#status", "Task Status: Running");
            let _ = component.set_attribute("task_exited", "false");
        }
        "status: FAILED" => {
            try_inner_text("#status", "Task: Failed");
            let _ = component.set_attribute("task_exited", "true");
        }
        "status: SUCCESS" => {
            try_inner_text("#status", "Task Finished Succesfully");
            let _ = component.set_attribute("task_exited", "true");
        }
        _ => {
            log!("Unknown status from runner:", status.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::single_char_key;

    #[test]
    fn single_char_key_accepts_ascii_character() {
        assert_eq!(single_char_key("a"), Some('a'));
    }

    #[test]
    fn single_char_key_accepts_unicode_character() {
        assert_eq!(single_char_key("å"), Some('å'));
        assert_eq!(single_char_key("ä"), Some('ä'));
        assert_eq!(single_char_key("ö"), Some('ö'));
    }

    #[test]
    fn single_char_key_rejects_longer_keys() {
        assert_eq!(single_char_key("Enter"), None);
        assert_eq!(single_char_key("ab"), None);
    }
}
