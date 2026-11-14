use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder, web};

use crate::runner::{RunGetParams, RunTaskError, start_run_task};

#[actix_web::get("/run")]
pub async fn run(params: web::Query<RunGetParams>) -> impl Responder {
    match start_run_task(params.project.clone(), params.task_key.clone()).await {
        Ok(task_key) => HttpResponse::Ok().json(serde_json::json!({
            "ok": true,
            "task": task_key
        })),
        Err(err) => {
            println!("Run task failed: {}", err);

            let message = err.to_string();
            let mut status = match err {
                RunTaskError::AlreadyRunning => HttpResponse::Conflict(),
                RunTaskError::Internal(_) => HttpResponse::InternalServerError(),
                RunTaskError::InvalidProjectFolder | RunTaskError::NoCommandConfigured(_) => {
                    HttpResponse::BadRequest()
                }
            };

            status.content_type(ContentType::json()).body(
                serde_json::json!({
                    "ok": false,
                    "message": message
                })
                .to_string(),
            )
        }
    }
}
