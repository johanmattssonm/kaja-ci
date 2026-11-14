use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder};
use serde::Serialize;

use crate::app::APP_DATA;

#[derive(Serialize)]
struct Status {
    project: String,
    task: String,
    num_terminals: usize,
}

#[actix_web::get("/status")]
pub async fn status() -> impl Responder {
    let app = APP_DATA.read().await;

    let status = Status {
        project: app.running_project(),
        task: app.running_task_name(),
        num_terminals: app.terminal_count(),
    };

    HttpResponse::Ok()
        .content_type(ContentType::json())
        .json(status)
}
