use wasm_bindgen::JsValue;

pub fn is_available() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };

    let window: JsValue = window.into();

    js_sys::Reflect::has(&window, &JsValue::from_str("__TAURI_INTERNALS__")).unwrap_or(false)
        || js_sys::Reflect::has(&window, &JsValue::from_str("__TAURI__")).unwrap_or(false)
}
