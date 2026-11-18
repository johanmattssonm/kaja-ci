use actix_web::HttpRequest;
use serde::Deserialize;

#[derive(Clone)]
pub struct AppAuth {
    key: String,
}

impl AppAuth {
    pub fn new(key: String) -> Self {
        Self { key }
    }

    pub fn key(&self) -> &str {
        self.key.as_str()
    }
}

#[derive(Deserialize)]
struct AuthQuery {
    kaja_key: Option<String>,
}

pub fn is_authorized(req: &HttpRequest) -> bool {
    let Some(auth) = req.app_data::<actix_web::web::Data<AppAuth>>() else {
        return false;
    };

    if auth.key().is_empty() {
        return true;
    }

    if req
        .headers()
        .get("x-kaja-key")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == auth.key())
    {
        return true;
    }

    actix_web::web::Query::<AuthQuery>::from_query(req.query_string())
        .ok()
        .and_then(|query| query.into_inner().kaja_key)
        .is_some_and(|value| value == auth.key())
}
