use actix_web::http::header::ContentType;
use actix_web::{Error, HttpRequest, HttpResponse, Responder, web};
use actix_ws::{Message, handle};
use chrono::Local;
use serde::{Deserialize, Serialize};

use std::path::PathBuf;
use std::sync::{OnceLock, RwLock as StdRwLock};
use tokio::sync::broadcast;

use crate::app::APP_DATA;
use crate::branch::current_branch;
use crate::runner::{RunTaskError, start_run_task};

#[derive(Deserialize)]
pub struct RunAllParams {
    pub project: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TaskRunningStatus {
    pub ok: bool,
    pub running: bool,
    pub project: String,
    pub task_key: String,
    pub action: String,
    pub time: String,
    pub href: String,
    pub message: Option<String>,
    pub branch: String,
}

static TASK_STATUS_TX: OnceLock<broadcast::Sender<String>> = OnceLock::new();
static LAST_TASK_STATUS: OnceLock<StdRwLock<Option<String>>> = OnceLock::new();

fn task_status_channel() -> &'static broadcast::Sender<String> {
    TASK_STATUS_TX.get_or_init(|| {
        let (tx, _) = broadcast::channel::<String>(100);
        tx
    })
}

fn last_task_status() -> &'static StdRwLock<Option<String>> {
    LAST_TASK_STATUS.get_or_init(|| StdRwLock::new(None))
}

pub fn send_task_status(task_key: &str, ok: bool, action: Option<&str>, project: Option<&str>) {
    send_task_status_with_running(task_key, ok, action, project, false);
}

pub fn send_task_status_with_running(
    task_key: &str,
    ok: bool,
    action: Option<&str>,
    project: Option<&str>,
    running: bool,
) {
    send_task_status_with_running_and_message(task_key, ok, action, project, running, None);
}

pub fn send_task_status_with_running_and_message(
    task_key: &str,
    ok: bool,
    action: Option<&str>,
    project: Option<&str>,
    running: bool,
    message: Option<&str>,
) {
    let project_value = project.unwrap_or("").to_string();

    let status = TaskRunningStatus {
        ok,
        running,
        project: project_value.clone(),
        task_key: task_key.to_string(),
        action: action.unwrap_or("").to_string(),
        time: now_time_string(),
        href: task_href(action.unwrap_or(""), project_value.as_str()),
        message: message.map(|value| value.to_string()),
        branch: if project_value.is_empty() {
            String::new()
        } else {
            current_branch(project_value.as_str()).unwrap_or_default()
        },
    };

    if let Ok(payload) = serde_json::to_string(&status) {
        if let Ok(mut last_status) = last_task_status().write() {
            *last_status = Some(payload.clone());
        }

        let _ = task_status_channel().send(payload);
    }
}

pub async fn websocket(req: HttpRequest, body: web::Payload) -> Result<HttpResponse, Error> {
    #[derive(Deserialize)]
    struct TaskWebsocketParams {
        project: Option<String>,
    }

    let requested_project = web::Query::<TaskWebsocketParams>::from_query(req.query_string())
        .ok()
        .and_then(|query| query.into_inner().project)
        .unwrap_or_default();

    let (response, mut session, mut msg_stream) = handle(&req, body)?;
    let mut status_rx = task_status_channel().subscribe();

    let current_status = last_task_status()
        .read()
        .ok()
        .and_then(|status| status.clone());

    if let Some(current_status) = current_status {
        if requested_project.is_empty() {
            let _ = session.text(current_status).await;
        } else if let Ok(status) = serde_json::from_str::<TaskRunningStatus>(&current_status) {
            if status.project == requested_project {
                let _ = session.text(current_status).await;
            }
        }
    }

    actix_web::rt::spawn(async move {
        loop {
            tokio::select! {
                incoming = msg_stream.recv() => {
                    match incoming {
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(_)) => {}
                        Some(Err(_)) => break,
                    }
                }
                status = status_rx.recv() => {
                    match status {
                        Ok(payload) => {
                            let should_send = if requested_project.is_empty() {
                                true
                            } else {
                                serde_json::from_str::<TaskRunningStatus>(&payload)
                                    .map(|status| status.project == requested_project)
                                    .unwrap_or(false)
                            };

                            if !should_send {
                                continue;
                            }

                            if session.text(payload).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(_)) => {}
                    }
                }
            }
        }
    });

    Ok(response)
}

#[actix_web::get("/run-all")]
pub async fn run(params: web::Query<RunAllParams>) -> impl Responder {
    let project = params.project.clone();
    let task_keys = load_task_keys(project.as_str());

    if task_keys.is_empty() {
        return HttpResponse::Ok().content_type(ContentType::json()).body(
            serde_json::json!({
                "ok": false,
                "message": "No tasks configured."
            })
            .to_string(),
        );
    }

    actix_web::rt::spawn(async move {
        let run_all_raw_file_name = Some(run_all_raw_file_name());

        for task_key in task_keys {
            let should_continue = run_task_from_run_all(
                project.as_str(),
                task_key.as_str(),
                run_all_raw_file_name.clone(),
            )
            .await;

            if !should_continue {
                return;
            }
        }

        send_task_status_with_running_and_message(
            "all-done",
            true,
            Some("log"),
            Some(project.as_str()),
            false,
            None,
        );
    });

    HttpResponse::Ok().content_type(ContentType::json()).body(
        serde_json::json!({
            "ok": true,
            "message": "Started tasks in background."
        })
        .to_string(),
    )
}

fn load_task_keys(project: &str) -> Vec<String> {
    let mut project_dir = PathBuf::from(project);
    project_dir.push("ci.json");

    let Ok(content) = std::fs::read_to_string(&project_dir) else {
        return Vec::new();
    };

    let Ok(config_json) = serde_json::from_str::<serde_json::Value>(content.as_str()) else {
        return Vec::new();
    };

    let run_all = config_json
        .get("run-all")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut tasks = Vec::<String>::new();
    for value in run_all {
        let Some(task_key) = value.as_str() else {
            continue;
        };

        let task_key = task_key.trim();
        if task_key.is_empty() || tasks.iter().any(|existing| existing == task_key) {
            continue;
        }

        tasks.push(task_key.to_string());
    }

    tasks
}

fn task_href(action: &str, project: &str) -> String {
    match action {
        "git" | "fetch" => format!("/fetch?project={project}"),
        "build" => format!("/build?project={project}"),
        "clone" => format!("/clone?project={project}"),
        "test" => format!("/test?project={project}"),
        "release" => format!("/release?project={project}"),
        "clean" => format!("/clean?project={project}"),
        "terminal" => format!("/terminal?project={project}"),
        "log" => format!("/log?project={project}"),
        custom => format!("/custom?project={project}&custom_label={custom}"),
    }
}

async fn run_task_from_run_all(
    project: &str,
    task_key: &str,
    run_all_raw_file_name: Option<String>,
) -> bool {
    let mut rx = task_status_channel().subscribe();
    let task_result = start_run_task(
        project.to_string(),
        task_key.to_string(),
        None,
        run_all_raw_file_name,
    )
    .await;

    match task_result {
        Ok(started_key) => {
            handle_started_task(&mut rx, started_key.as_str(), task_key, project).await
        }
        Err(err) => {
            send_task_status_for_run_error(&err, task_key, project);
            println!("Run task failed: {err} stop tasks.");
            false
        }
    }
}

fn send_task_status_for_run_error(err: &RunTaskError, task_key: &str, project: &str) {
    match err {
        RunTaskError::AlreadyRunning(blocking_task_key) => {
            let running_action = blocking_task_key
                .split_once('-')
                .map(|(action, _)| action)
                .unwrap_or(blocking_task_key.as_str());

            send_task_status_with_running_and_message(
                blocking_task_key.as_str(),
                false,
                Some(running_action),
                Some(project),
                false,
                Some("Blocked by running task."),
            );
        }
        _ => {
            send_task_status(task_key, false, Some(task_key), Some(project));
        }
    }
}

async fn handle_started_task(
    rx: &mut broadcast::Receiver<String>,
    started_key: &str,
    task_key: &str,
    project: &str,
) -> bool {
    append_task_started_log_line(started_key, task_key).await;

    send_task_status_with_running(started_key, true, Some(task_key), Some(project), true);

    while let Ok(payload) = rx.recv().await {
        let Ok(status) = serde_json::from_str::<TaskRunningStatus>(&payload) else {
            continue;
        };

        if status.task_key != started_key || status.running {
            continue;
        }

        if !status.ok {
            println!("Run task failed: {} stop tasks.", started_key);
            return false;
        }

        return true;
    }

    true
}

async fn append_task_started_log_line(started_task_key: &str, project: &str) {
    let timestamp = now_time_string();
    let task_name = started_task_key
        .split_once('-')
        .map_or(started_task_key, |(name, _)| name);
    let line = format!("[{timestamp}] [run-all] starting task: {task_name} project: {project}\n");

    let mut app = APP_DATA.write().await;
    app.append_task_history(started_task_key, line.as_bytes());
}

fn now_time_string() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn run_all_raw_file_name() -> String {
    format!("run-all-{}.raw", Local::now().format("%Y%m%dT%H%M%S%.3f%z"))
}
