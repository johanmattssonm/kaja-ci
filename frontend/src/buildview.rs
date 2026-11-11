use futures_channel::mpsc;
use futures_util::{SinkExt, StreamExt};
use gloo_net::websocket::{Message, futures::WebSocket};
use kaja_web::prelude::*;
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, window};

#[component("build-component")]
struct Build {}

impl Component for BuildComponent {
    fn connected(&self) {
        let id = self.get_id();
        log!("Connected:", &id);

        self.render();

        spawn_local(async {
            connect_build_view(id).await;
        });
    }

    fn disconnected(&self) {
        log!("Disconnected:", self.get_id());
    }

    fn attribute_changed(&self, _name: &str, _old_value: &str, _new_value: &str) {}
}

impl BuildComponent {
    fn render(&self) {
        let id = self.get_id();
        let element = match get_element_by_id(&id) {
            Some(element) => element,
            None => {
                log!("No root element for build component");
                return;
            }
        };

        element.set_inner_html("");

        let document = window().unwrap().document().unwrap();

        let output = document.create_element("pre").unwrap();
        output
            .set_attribute("id", &format!("{}-output", id))
            .unwrap();
        output
            .set_attribute(
                "style",
                "white-space: pre-wrap; font-family: monospace; background: #111; color: #ddd; padding: 12px; min-height: 300px; overflow-y: auto;",
            )
            .unwrap();
        element.append_child(&output).unwrap();

        let input = document.create_element("input").unwrap();
        input.set_attribute("id", &format!("{}-stdin", id)).unwrap();
        input
            .set_attribute("placeholder", "Type response and press Enter")
            .unwrap();
        input.set_attribute("autocomplete", "off").unwrap();
        element.append_child(&input).unwrap();
    }
}

async fn connect_build_view(id: String) {
    let ws = match WebSocket::open("ws://127.0.0.1:2003/ws") {
        Ok(ws) => ws,
        Err(err) => {
            log!("failed to open websocket", format!("{err:?}"));
            return;
        }
    };

    log!("Connected websocket");

    let (mut ws_write, mut ws_read) = ws.split();
    let (input_tx, mut input_rx) = mpsc::unbounded::<String>();

    spawn_local(async move {
        while let Some(message) = input_rx.next().await {
            if let Err(err) = ws_write.send(Message::Text(message)).await {
                log!("failed to send websocket message", format!("{err:?}"));
                break;
            }
        }
    });

    setup_stdin_handler(&id, input_tx.clone());

    if let Err(err) = input_tx.unbounded_send("BUILD".to_string()) {
        log!("failed to enqueue BUILD message", format!("{err:?}"));
        return;
    }

    while let Some(message) = ws_read.next().await {
        match message {
            Ok(Message::Text(text)) => {
                append_build_output(&format!("{}-output", id), text);
            }
            Ok(Message::Bytes(_)) => {
                log!("Expecting string got binary data.");
            }
            Err(err) => {
                log!("websocket receive error", format!("{err:?}"));
                return;
            }
        }
    }
}

fn setup_stdin_handler(id: &str, input_tx: mpsc::UnboundedSender<String>) {
    let input_id = format!("{}-stdin", id);
    let element = match get_element_by_id(&input_id) {
        Some(element) => element,
        None => {
            log!("No stdin input box for build component.");
            return;
        }
    };

    let input = match element.dyn_into::<HtmlInputElement>() {
        Ok(input) => input,
        Err(_) => {
            log!("Expected HtmlInputElement for stdin box");
            return;
        }
    };

    let keydown_input = input.clone();
    let keydown_tx = input_tx.clone();
    let keydown = Closure::<dyn FnMut(_)>::new(move |event: web_sys::KeyboardEvent| {
        if event.key() != "Enter" {
            return;
        }

        event.prevent_default();

        let value = keydown_input.value();
        if keydown_tx.unbounded_send(value).is_err() {
            log!("stdin channel is closed");
            return;
        }

        keydown_input.set_value("");
    });

    if input
        .add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref())
        .is_err()
    {
        log!("failed to register stdin keydown handler");
        return;
    }

    keydown.forget();
}

fn append_build_output(id: &str, chunk: String) {
    let element = match get_element_by_id(id) {
        Some(element) => element,
        None => {
            log!("No output box for build.");
            return;
        }
    };

    let plain_chunk = drop_pipe_name(chunk);
    let existing = element.text_content().unwrap_or_default();
    element.set_text_content(Some(&(existing + &plain_chunk)));
    element.set_scroll_top(element.scroll_height());
}

fn drop_pipe_name(line: String) -> String {
    if line.starts_with("stderr: ") {
        let sub = &line["stderr: ".len()..line.len()];
        return sub.to_string();
    } else if line.starts_with("stdout: ") {
        let sub = &line["stdout: ".len()..line.len()];
        return sub.to_string();
    }

    line
}
