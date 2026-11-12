use crate::rest::{Action, ActionResponse};

use super::color;
use futures_channel::mpsc;
use futures_util::{SinkExt, StreamExt};
use gloo_net::{
    http::Request,
    websocket::{Message, futures::WebSocket},
};
use kaja_web::prelude::*;
use wasm_bindgen::{JsCast, prelude::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;

#[component("console-component")]
struct Console {
    #[attribute]
    project: String,

    #[attribute]
    action: String,
}

impl Component for ConsoleComponent {
    fn connected(&self) {
        let id = self.get_id();
        let action: Option<String> = self.get_attribute("action");
        let project: Option<String> = self.get_attribute("project");

        log!("Connected:", &id);
        self.render();

        if action.is_none() {
            log!("No action for console.");
            return;
        }

        if project.is_none() {
            log!("No project for console.");
            return;
        }

        let component_id = id.clone();

        spawn_local(async {
            let action = action.unwrap();
            let project = project.unwrap();
            connect_console_view(component_id, action.as_str(), project.as_str()).await;
        });

        if let Some(element) = get_element_by_id(&format!("{}-stdin", &id)) {
            if let Ok(input) = element.dyn_into::<HtmlInputElement>() {
                let _ = input.focus();
            }
        }
    }

    fn disconnected(&self) {
        log!("Disconnected:", self.get_id());
    }

    fn attribute_changed(&self, _name: &str, _old_value: &str, _new_value: &str) {}
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
            <style>
                .terminal-output {
                    white-space: pre;
                    tab-size: 8;
                    line-height: 1.35;
                    background: #07090d;
                    color: #e6edf3;
                    border: 1px solid #2f3845;
                    border-radius: 8px;
                    padding: 14px;
                    min-height: 360px;
                    max-height: 70vh;
                    overflow: auto;
                }

                .terminal-span { white-space: pre; }
                .terminal-bold { font-weight: 700; }

                .terminal-fg-black { color: #0b0b0b; }
                .terminal-fg-red { color: #ff7b72; }
                .terminal-fg-green { color: #3fb950; }
                .terminal-fg-yellow { color: #d29922; }
                .terminal-fg-blue { color: #79c0ff; }
                .terminal-fg-magenta { color: #bc8cff; }
                .terminal-fg-cyan { color: #76e3ea; }
                .terminal-fg-white { color: #f0f6fc; }

                .terminal-fg-bright-black { color: #8b949e; }
                .terminal-fg-bright-red { color: #ffa198; }
                .terminal-fg-bright-green { color: #56d364; }
                .terminal-fg-bright-yellow { color: #e3b341; }
                .terminal-fg-bright-blue { color: #a5d6ff; }
                .terminal-fg-bright-magenta { color: #d2a8ff; }
                .terminal-fg-bright-cyan { color: #8be9fd; }
                .terminal-fg-bright-white { color: #ffffff; }

                .terminal-cursor {
                    background: #e6edf3;
                    color: #0d1117;
                }

                .terminal-inverse {
                    background: #e6edf3;
                    color: #0d1117;
                    font-weight: 700;
                }
            </style>
            <h1 id="status">Loading</h1>
            <p>Project Folder: $(project)</p>
            <pre id="$(id)-output" class="terminal-output" tabindex="0"></pre>
            <input id="$(id)-stdin" placeholder="Type input and press Enter">
        }};

        element.set_inner_html(content.as_str());
    }
}

#[callback("abort")]
async fn abort_current_task(project: String) {
    let endpoint = format!("/abort?project={}", project);

    if Request::get(endpoint.as_str()).send().await.is_err() {
        try_inner_text("#save-status", "Failed to abort task.");
    }
}

async fn connect_console_view(id: String, action: &str, project: &str) {
    let ws = WebSocket::open("ws://127.0.0.1:2003/ws");

    if ws.is_err() {
        log!("Failed to open websocket");
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

    let project_action = Action {
        action: action.to_string(),
        folder: Some(project.to_string()),
        command: Some("/home/johan/Downloads/resting-chair/test.sh".to_string()),
        stdin: None,
    };

    let json_string = serde_json::to_string(&project_action).unwrap();

    if let Err(err) = input_tx.unbounded_send(json_string) {
        log!("Failed to enqueue action message", format!("{err:?}"));
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
                return;
            }
        }
    }
}

fn key_to_terminal_input(event: &web_sys::KeyboardEvent) -> Option<String> {
    if event.ctrl_key() {
        let key = event.key();
        if key.len() == 1 {
            let ch = key.chars().next()?.to_ascii_lowercase();
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

    match event.key().as_str() {
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
        key if key.len() == 1 => Some(key.to_string()),
        _ => None,
    }
}

fn send_stdin(stdin: String, input_tx: &mpsc::UnboundedSender<String>) {
    let action = Action {
        action: "STDIN".to_string(),
        command: None,
        folder: None,
        stdin: Some(stdin),
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
    let input_id = format!("{}-stdin", id);
    let output_id = format!("{}-output", id);

    let input = match get_element_by_id(&input_id)
        .and_then(|element| element.dyn_into::<HtmlInputElement>().ok())
    {
        Some(input) => input,
        None => {
            log!("Expected HtmlInputElement for stdin box");
            return;
        }
    };

    let output = match get_element_by_id(&output_id) {
        Some(output) => output,
        None => {
            log!("No output box for build component.");
            return;
        }
    };

    let keydown_input = input.clone();
    let keydown_tx = input_tx.clone();
    let keydown = Closure::<dyn FnMut(_)>::new(move |event: web_sys::KeyboardEvent| {
        let Some(stdin) = key_to_terminal_input(&event) else {
            return;
        };

        event.prevent_default();
        send_stdin(stdin, &keydown_tx);
        keydown_input.set_value("");
    });

    if input
        .add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref())
        .is_err()
    {
        log!("Failed to register stdin keydown handler");
        return;
    }

    // Fullscreen terminal UIs often involve users clicking the output area.
    // Keep keyboard flow working by forwarding keys from the output element too.
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

    let focus_input = input.clone();
    let click_focus = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::Event| {
        let _ = focus_input.focus();
    });

    if output
        .add_event_listener_with_callback("click", click_focus.as_ref().unchecked_ref())
        .is_err()
    {
        log!("Failed to register output click handler");
        return;
    }

    keydown.forget();
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

            element.set_inner_html(&rendered);
            return;
        }

        let rendered = color::append_ansi_chunk(id, &stdout);
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
