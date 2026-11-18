use actix_web::{HttpRequest, HttpResponse, Responder};

pub fn authorization_failure_message(_req: &HttpRequest) -> Option<&'static str> {
    None
}

#[actix_web::post("/auth/session")]
pub async fn establish_session(_req: HttpRequest) -> impl Responder {
    HttpResponse::NoContent().finish()
}
