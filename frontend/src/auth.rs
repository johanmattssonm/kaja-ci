use gloo_net::http::{Request, Response};
use std::cell::RefCell;

thread_local! {
    static SESSION_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub async fn session_key() -> Option<String> {
    if let Some(cached) = SESSION_KEY.with(|value| value.borrow().clone()) {
        return Some(cached);
    }

    if !crate::tauri::is_available() {
        return None;
    }

    let value = tauri_wasm::invoke("get_session_key").await.ok()?;
    let key = serde_wasm_bindgen::from_value::<String>(value).ok()?;

    SESSION_KEY.with(|value| {
        *value.borrow_mut() = Some(key.clone());
    });

    Some(key)
}

pub async fn with_key(endpoint: &str) -> Option<String> {
    let key = session_key().await?;
    Some(with_key_value(endpoint, key.as_str()))
}

pub fn with_cached_key(endpoint: &str) -> String {
    let key = SESSION_KEY.with(|value| value.borrow().clone());

    match key {
        Some(key) => with_key_value(endpoint, key.as_str()),
        None => endpoint.to_string(),
    }
}

pub async fn get(endpoint: &str) -> Result<Response, gloo_net::Error> {
    let endpoint = with_key(endpoint)
        .await
        .unwrap_or_else(|| endpoint.to_string());
    Request::get(endpoint.as_str()).send().await
}

pub async fn post(endpoint: &str) -> Result<Response, gloo_net::Error> {
    let endpoint = with_key(endpoint)
        .await
        .unwrap_or_else(|| endpoint.to_string());
    Request::post(endpoint.as_str()).send().await
}

fn with_key_value(endpoint: &str, key: &str) -> String {
    if endpoint.contains('?') {
        format!("{endpoint}&kaja_key={}", js_sys::encode_uri_component(key))
    } else {
        format!("{endpoint}?kaja_key={}", js_sys::encode_uri_component(key))
    }
}
