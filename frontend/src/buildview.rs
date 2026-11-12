use crate::rest::{Action, ActionResponse};

use super::color;
use futures_channel::mpsc;
use futures_util::{SinkExt, StreamExt};
use gloo_net::websocket::{Message, futures::WebSocket};
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
            <p>Project Folder: $(project)</p>
            <pre id="$(id)-output"></pre>
            <input id="$(id)-stdin" placeholder="Type input and press Enter">
        }};

        element.set_inner_html(content.as_str());
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

fn setup_stdin_handler(id: &str, input_tx: mpsc::UnboundedSender<String>) {
    let input_id = format!("{}-stdin", id);
    let element = get_element_by_id(&input_id);

    if element.is_none() {
        log!("No stdin input box for build component.");
        return;
    };

    let input = element.unwrap().dyn_into::<HtmlInputElement>();

    if input.is_err() {
        log!("Expected HtmlInputElement for stdin box");
        return;
    }

    let input = input.unwrap();
    let keydown_input = input.clone();
    let keydown_tx = input_tx.clone();
    let keydown = Closure::<dyn FnMut(_)>::new(move |event: web_sys::KeyboardEvent| {
        if event.key() != "Enter" {
            return;
        }

        event.prevent_default();

        let value = keydown_input.value();
        let action = Action {
            action: "STDIN".to_string(),
            command: None,
            folder: None,
            stdin: Some(value),
        };

        let json = serde_json::to_string(&action);

        if json.is_err() {
            log!("Failed to create json action.");
            return;
        }

        let json = json.unwrap();

        if keydown_tx.unbounded_send(json).is_err() {
            log!("WebSocket for stdin channel is closed");
            return;
        }

        keydown_input.set_value("");
    });

    let callback_ref = keydown.as_ref().unchecked_ref();
    let registerd = input.add_event_listener_with_callback("keydown", callback_ref);

    if registerd.is_err() {
        log!("Failed to register stdin keydown handler");
        return;
    }

    keydown.forget(); // this is ok because the input is around until user leaves this page
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
