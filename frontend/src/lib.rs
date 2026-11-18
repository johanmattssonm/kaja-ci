use kaja_web::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

mod auth;
mod buildstatus;
mod color;
mod consolecompoent;
mod projectstatus;
mod settings;
mod tauri;

pub mod rest;

#[callback("browseProjectFolder")]
fn browse_project_folder() {
    if !crate::tauri::is_available() {
        set_project_folder_error("Folder picker is only available in the Tauri desktop app.");
        return;
    }

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

fn update_project_folder_picker_visibility() {
    let Some(window) = web_sys::window() else {
        return;
    };

    let Some(document) = window.document() else {
        return;
    };

    let Some(element) = document.get_element_by_id("add-project-folder-picker") else {
        return;
    };

    if !crate::tauri::is_available() {
        if let Ok(html_element) = element.dyn_into::<web_sys::HtmlElement>() {
            let _ = html_element.style().set_property("display", "none");
        }
    }
}

#[wasm_bindgen(start)]
pub fn init() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_error_panic_hook::set_once();

    spawn_local(async {
        let _ = crate::auth::session_key().await;
    });

    init_callbacks();
    update_project_folder_picker_visibility();
}
