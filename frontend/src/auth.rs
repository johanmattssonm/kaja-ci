use gloo_net::http::{Request, RequestBuilder, Response};
use std::cell::RefCell;

const AUTH_HEADER_NAME: &str = "x-kaja-key";

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

pub async fn with_auth_header(request: RequestBuilder) -> RequestBuilder {
    match session_key().await {
        Some(key) => request.header(AUTH_HEADER_NAME, key.as_str()),
        None => request,
    }
}

pub async fn bootstrap_session() {
    let request = with_auth_header(Request::post("/auth/session")).await;
    let _ = request.send().await;
}

pub async fn get(endpoint: &str) -> Result<Response, gloo_net::Error> {
    let request = with_auth_header(Request::get(endpoint)).await;
    request.send().await
}

pub async fn post(endpoint: &str) -> Result<Response, gloo_net::Error> {
    let request = with_auth_header(Request::post(endpoint)).await;
    request.send().await
}
