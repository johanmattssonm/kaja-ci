use actix_web::cookie::{Cookie, SameSite};
use actix_web::{HttpRequest, HttpResponse, Responder, web};

const AUTH_HEADER_NAME: &str = "x-kaja-key";
const AUTHORIZATION_HEADER_NAME: &str = "authorization";
const AUTH_COOKIE_NAME: &str = "kaja-session";

const UNAUTHORIZED_NO_KEY: &str = "Unauthorized: no key was sent.";
const UNAUTHORIZED_INVALID_KEY: &str = "Unauthorized: a key was sent, but it is invalid.";
const UNAUTHORIZED_MISSING_STATE: &str = "Unauthorized: authentication state is unavailable.";

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

fn key_from_custom_header(req: &HttpRequest) -> Option<&str> {
    req.headers()
        .get(AUTH_HEADER_NAME)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn key_from_authorization_header(req: &HttpRequest) -> Option<&str> {
    let value = req
        .headers()
        .get(AUTHORIZATION_HEADER_NAME)
        .and_then(|header| header.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    if let Some((scheme, token)) = value.split_once(' ')
        && scheme.eq_ignore_ascii_case("bearer")
    {
        let token = token.trim();
        if !token.is_empty() {
            return Some(token);
        }
    }

    Some(value)
}

fn key_from_header(req: &HttpRequest) -> Option<&str> {
    key_from_custom_header(req).or_else(|| key_from_authorization_header(req))
}

fn key_from_cookie(req: &HttpRequest) -> Option<String> {
    req.cookie(AUTH_COOKIE_NAME)
        .map(|cookie| cookie.value().to_string())
}

fn request_has_valid_key(req: &HttpRequest, key: &str) -> bool {
    key_from_header(req).is_some_and(|value| value == key)
        || key_from_cookie(req).is_some_and(|value| value == key)
}

fn request_contains_key(req: &HttpRequest) -> bool {
    key_from_header(req).is_some() || key_from_cookie(req).is_some()
}

pub fn authorization_failure_message(req: &HttpRequest) -> Option<&'static str> {
    let Some(auth) = req.app_data::<web::Data<AppAuth>>() else {
        return Some(UNAUTHORIZED_MISSING_STATE);
    };

    if auth.key().is_empty() {
        return None;
    }

    if request_has_valid_key(req, auth.key()) {
        return None;
    }

    if request_contains_key(req) {
        Some(UNAUTHORIZED_INVALID_KEY)
    } else {
        Some(UNAUTHORIZED_NO_KEY)
    }
}

#[actix_web::post("/auth/session")]
pub async fn establish_session(req: HttpRequest) -> impl Responder {
    let Some(auth) = req.app_data::<web::Data<AppAuth>>() else {
        return HttpResponse::Unauthorized().body(UNAUTHORIZED_MISSING_STATE);
    };

    if auth.key().is_empty() {
        return HttpResponse::NoContent().finish();
    }

    let Some(provided_key) = key_from_header(&req) else {
        return HttpResponse::Unauthorized().body(UNAUTHORIZED_NO_KEY);
    };

    if provided_key != auth.key() {
        return HttpResponse::Unauthorized().body(UNAUTHORIZED_INVALID_KEY);
    }

    let cookie = Cookie::build(AUTH_COOKIE_NAME, auth.key().to_string())
        .path("/")
        .http_only(true)
        .same_site(SameSite::Strict)
        .finish();

    HttpResponse::NoContent().cookie(cookie).finish()
}
