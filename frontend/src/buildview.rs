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
use wasm_bindgen::{JsCast, JsValue, prelude::Closure};
use wasm_bindgen_futures::spawn_local;

const ACTIVE_TASK_KEY_KEY: &str = "__kaja_active_task_key";

#[component("console-component")]
struct Console {
    #[attribute]
    project: String,

    #[attribute]
    action: String,

    #[attribute]
    connect: String,

    #[attribute]
    task_key: String,
}

impl Component for ConsoleComponent {
    fn connected(&self) {
        let id = self.get_id();
        let action: String = self.get_attribute("action").unwrap_or_default();
        let project: String = self.get_attribute("project").unwrap_or_default();

        log!("Connected:", &id);
        self.render();
        focus_console_output(&id);
        ensure_abort_on_exit_handlers();
        set_active_task_key(None);

        if action.is_empty() || project.is_empty() {
            return;
        }

        if action == "terminal" {
            return;
        }

        let component_id = id.clone();

        spawn_local(async move {
            start_and_connect_console_view(component_id, action.as_str(), project.as_str()).await;
        });
    }

    fn disconnected(&self) {
        let id = self.get_id();
        log!("Disconnected:", &id);

        let task_key: String = self.get_attribute("task_key").unwrap_or_default();

        if task_key.is_empty() {
            return;
        }

        if abort_task_with_beacon(Some(task_key.as_str()), None) {
            set_active_task_key(None);
            return;
        }

        spawn_local(async move {
            if !abort_task(Some(task_key.as_str()), None).await {
                log!("Failed to abort task on console disconnect.", task_key);
            } else {
                set_active_task_key(None);
            }
        });
    }

    fn attribute_changed(&self, name: &str, _old_value: &str, new_value: &str) {
        if name != "connect" || new_value.is_empty() {
            return;
        }

        let component_id = self.get_id();

        if let Some(component) = get_element_by_id(&component_id) {
            if component.set_attribute("task_key", new_value).is_err() {
                log!("Failed to set task_key attribute on console component.");
            }
        }

        let task_key = new_value.to_string();
        set_active_task_key(Some(task_key.as_str()));

        spawn_local(async move {
            connect_console_view(component_id, task_key).await;
        });
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
    fn render(&self) {
        let id = self.get_id();
        let element = match get_element_by_id(&id) {
            Some(element) => element,
            None => {
                log!("No root element for build component");
                return;
            }
        };

        color::clear_render_state(&format!("{}-output", id));
        element.set_inner_html("");

        let project: String = self.get_attribute("project").unwrap_or_default();

        let content = html! {{
            <h1 id="status">Loading</h1>
            <p>Project Folder: $(project)</p>
            <pre id="$(id)-output" class="terminal-output" tabindex="0"></pre>
        }};

        element.set_inner_html(content.as_str());
    }
}

fn url_encode(value: &str) -> String {
    js_sys::encode_uri_component(value).into()
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

fn set_window_string(key: &str, value: Option<&str>) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let js_value = match value {
        Some(value) => JsValue::from_str(value),
        None => JsValue::NULL,
    };

    let _ = js_sys::Reflect::set(window.as_ref(), &JsValue::from_str(key), &js_value);
}

fn get_window_string(key: &str) -> Option<String> {
    let window = web_sys::window()?;
    let value = js_sys::Reflect::get(window.as_ref(), &JsValue::from_str(key)).ok()?;

    if value.is_null() || value.is_undefined() {
        return None;
    }

    value.as_string().filter(|value| !value.is_empty())
}

fn set_active_task_key(task_key: Option<&str>) {
    set_window_string(ACTIVE_TASK_KEY_KEY, task_key);
}

fn get_active_task_key() -> Option<String> {
    get_window_string(ACTIVE_TASK_KEY_KEY)
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
    let task_key = get_active_task_key().or_else(|| {
        get_element("console-component")
            .and_then(|component| component.get_attribute("task_key"))
            .filter(|value| !value.is_empty())
    });

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

    Request::get(endpoint.as_str()).send().await.is_ok()
}

#[callback("abort")]
async fn abort_current_task(project: String) {
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
    } else {
        set_active_task_key(None);
    }
}

#[derive(Deserialize)]
struct RunTaskResponse {
    ok: bool,
    task: Option<String>,
    message: Option<String>,
}

async fn start_task(project: &str, action: &str) -> Option<String> {
    let endpoint = format!("/run?project={}&task_key={}", project, action);

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
        log!("/run failed", message);
    } else {
        log!("/run failed");
    }

    None
}

#[callback("connectTerminal")]
async fn connect_terminal(project: String) {
    let Some(task_key) = start_task(project.as_str(), "terminal").await else {
        log!("Failed to start terminal.");
        return;
    };

    let Some(console) = get_element("console-component") else {
        log!("No console component found.");
        return;
    };

    let console_id = console
        .get_attribute("id")
        .unwrap_or_else(|| "console-component".to_string());

    set_active_task_key(Some(task_key.as_str()));

    if console
        .set_attribute("task_key", task_key.as_str())
        .is_err()
    {
        log!("Failed to set task_key attribute on terminal console.");
    }

    if console.set_attribute("connect", task_key.as_str()).is_err() {
        log!("Failed to set connect attribute on terminal console.");
    }

    focus_console_output(&console_id);
}

async fn start_and_connect_console_view(id: String, action: &str, project: &str) {
    let Some(task_key) = start_task(project, action).await else {
        return;
    };

    let Some(component) = get_element_by_id(&id) else {
        log!("No console component found.", id);
        return;
    };

    set_active_task_key(Some(task_key.as_str()));

    if component
        .set_attribute("task_key", task_key.as_str())
        .is_err()
    {
        log!("Failed to set task_key attribute on console component.");
    }

    if component
        .set_attribute("connect", task_key.as_str())
        .is_err()
    {
        log!("Failed to set connect attribute on console component.");
    }
}

async fn connect_console_view(id: String, task_key: String) {
    let ws = WebSocket::open("ws://127.0.0.1:2003/ws");

    if ws.is_err() {
        log!("Failed to open websocket");
        try_inner_text("#status", "disconnected");
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
                process_build_output(&format!("{}-output", id), text);
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

    try_inner_text("#status", "disconnected");
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

fn process_build_output(id: &str, chunk: String) {
    let element = match get_element_by_id(id) {
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
        handle_status(resp);
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

        let rendered = color::append_ansi_chunk(id, &stdout, resp.cursor_col);
        element.set_inner_html(&rendered);
        element.set_scroll_top(element.scroll_height());
    }
}

fn handle_status(line: ActionResponse) {
    if line.status.is_none() {
        log!("No status");
        return;
    }

    let status = line.status.unwrap();

    match status.as_str() {
        "status: RUNNING" => {
            try_inner_text("#status", "Task: Running");
        }
        "status: FAILED" => {
            try_inner_text("#status", "Task: Failed");
        }
        "status: SUCCESS" => {
            try_inner_text("#status", "Task Finished Succesfully");
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
