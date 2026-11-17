use kaja_web::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

mod buildstatus;
mod color;
mod consolecompoent;
mod projectstatus;
mod settings;

pub mod rest;

#[callback("browseProjectFolder")]
fn browse_project_folder() {
    spawn_local(async {
        match tauri_wasm::invoke("pick_project_folder").await {
            Ok(value) => match serde_wasm_bindgen::from_value::<Option<String>>(value) {
                Ok(Some(path)) => {
                    set_project_folder_path(path.as_str());
                    clear_project_folder_error();
                }
                Ok(None) => {}
                Err(error) => set_project_folder_error(
                    format!("Could not read selected folder: {error}").as_str(),
                ),
            },
            Err(error) => {
                set_project_folder_error(format!("Could not choose a folder: {error}").as_str())
            }
        }
    });
}

fn set_project_folder_path(path: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let Some(document) = window.document() else {
        return;
    };

    let Some(element) = document.get_element_by_id("add-project-input") else {
        return;
    };

    let Ok(input) = element.dyn_into::<web_sys::HtmlInputElement>() else {
        return;
    };

    input.set_value(path);

    if let Ok(event) = web_sys::Event::new("input") {
        let _ = input.dispatch_event(&event);
    }

    if let Ok(event) = web_sys::Event::new("change") {
        let _ = input.dispatch_event(&event);
    }
}

fn set_project_folder_error(message: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let Some(document) = window.document() else {
        return;
    };

    if let Some(element) = document.get_element_by_id("add-project-folder-error") {
        element.set_text_content(Some(message));
    }
}

fn clear_project_folder_error() {
    set_project_folder_error("");
}

#[wasm_bindgen(start)]
pub fn init() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_error_panic_hook::set_once();
    init_callbacks();
}
