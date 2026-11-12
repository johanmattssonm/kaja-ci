use actix_web::http::header::ContentType;
use actix_web::{Error, HttpResponse, Responder, web};
use actix_ws::{Message, Session, handle};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use serde::Deserialize;
use std::collections::VecDeque;
use std::io::{Read, Write};
use tokio::task;

use std::path::PathBuf;
use std::sync::{Arc, RwLock as StdRwLock};
use std::thread;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};

use crate::runner::ProjectGetParams;
use crate::{APP_DATA, Task};

#[actix_web::get("/abort")]
pub async fn abort(params: web::Query<ProjectGetParams>) -> impl Responder {
    let mut app = APP_DATA.write().await;

    let Some(task) = app.tasks.get_mut(&params.project) else {
        let mess = format!("No task running for {}", &params.project);
        println!("{}", mess);

        return HttpResponse::InternalServerError().json(serde_json::json!({
            "ok": false,
            "message": mess
        }));
    };

    let Some(command_lock) = task.running_command.take() else {
        let mess = format!("No task running for {}", &params.project);
        println!("{}", mess);

        return HttpResponse::InternalServerError().json(serde_json::json!({
            "ok": false,
            "message": mess
        }));
    };

    let mut command = command_lock.write().unwrap();
    let _ = command.kill();

    let _ = app.tasks.remove(&params.project);

    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body("{ \"ok\": true }")
}
