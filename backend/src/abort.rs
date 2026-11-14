use actix_web::{HttpResponse, Responder, web};
use serde::Deserialize;

use crate::app::{APP_DATA, AppData};

#[derive(Deserialize)]
pub struct AbortParams {
    pub project: Option<String>,
    pub task_key: Option<String>,
}

fn resolve_task_key(app: &AppData, params: &AbortParams) -> Option<String> {
    if let Some(task_key) = params.task_key.as_deref().filter(|value| !value.is_empty()) {
        if app.tasks.contains_key(task_key) {
            return Some(task_key.to_string());
        }
    }

    let project = params.project.as_deref()?.trim();

    if project.is_empty() {
        return None;
    }

    if app.tasks.contains_key(project) {
        return Some(project.to_string());
    }

    if app.running_project == project && !app.running_task_name.is_empty() {
        return Some(format!("{}-{}", app.running_task_name, app.running_project));
    }

    None
}

async fn abort_with_params(params: AbortParams) -> HttpResponse {
    let mut app = APP_DATA.write().await;

    let Some(task_key) = resolve_task_key(&app, &params) else {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "ok": false,
            "message": "No matching running task found."
        }));
    };

    let command_lock = match app.tasks.get_mut(&task_key) {
        Some(task) => match task.running_command.take() {
            Some(command_lock) => command_lock,
            None => {
                let mess = format!("No task running for key {}", task_key);

                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "ok": false,
                    "message": mess
                }));
            }
        },
        None => {
            let mess = format!("No task running for key {}", task_key);

            return HttpResponse::InternalServerError().json(serde_json::json!({
                "ok": false,
                "message": mess
            }));
        }
    };

    let Ok(mut command) = command_lock.write() else {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "ok": false,
            "message": "Failed to lock running task command."
        }));
    };

    let _ = command.kill();
    drop(command);

    let _ = app.drop_task_by_key(task_key.as_str());

    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "task_key": task_key
    }))
}

#[actix_web::get("/abort")]
pub async fn abort(params: web::Query<AbortParams>) -> impl Responder {
    abort_with_params(params.into_inner()).await
}

#[actix_web::post("/abort")]
pub async fn abort_post(params: web::Query<AbortParams>) -> impl Responder {
    abort_with_params(params.into_inner()).await
}
