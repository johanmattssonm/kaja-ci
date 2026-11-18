use actix_web::cookie::{Cookie, SameSite};
use actix_web::{HttpRequest, HttpResponse, Responder};

const AUTH_HEADER_NAME: &str = "x-kaja-key";
const AUTH_COOKIE_NAME: &str = "kaja-session";

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

fn key_from_header(req: &HttpRequest) -> Option<&str> {
    req.headers()
        .get(AUTH_HEADER_NAME)
        .and_then(|value| value.to_str().ok())
}

fn key_from_cookie(req: &HttpRequest) -> Option<String> {
    req.cookie(AUTH_COOKIE_NAME)
        .map(|cookie| cookie.value().to_string())
}

fn request_has_valid_key(req: &HttpRequest, key: &str) -> bool {
    key_from_header(req).is_some_and(|value| value == key)
        || key_from_cookie(req).is_some_and(|value| value == key)
}

pub fn is_authorized(req: &HttpRequest) -> bool {
    let Some(auth) = req.app_data::<actix_web::web::Data<AppAuth>>() else {
        return false;
    };

    if auth.key().is_empty() {
        return true;
    }

    request_has_valid_key(req, auth.key())
}

#[actix_web::post("/auth/session")]
pub async fn establish_session(req: HttpRequest) -> impl Responder {
    let Some(auth) = req.app_data::<actix_web::web::Data<AppAuth>>() else {
        return HttpResponse::Unauthorized().body("Unauthorized");
    };

    if auth.key().is_empty() {
        return HttpResponse::NoContent().finish();
    }

    if !key_from_header(&req).is_some_and(|value| value == auth.key()) {
        return HttpResponse::Unauthorized().body("Unauthorized");
    }

    let cookie = Cookie::build(AUTH_COOKIE_NAME, auth.key().to_string())
        .path("/")
        .http_only(true)
        .same_site(SameSite::Strict)
        .finish();

    HttpResponse::NoContent().cookie(cookie).finish()
}
