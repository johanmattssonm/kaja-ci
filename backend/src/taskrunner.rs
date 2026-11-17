use actix_web::http::header::ContentType;
use actix_web::{Error, HttpRequest, HttpResponse, Responder, web};
use actix_ws::{Message, handle};
use chrono::{Local, SecondsFormat};
use serde::{Deserialize, Serialize};

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock as StdRwLock};
use tokio::sync::broadcast;

use crate::app::{TaskHistoryMetadata, log_base_dir};
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
        let history = create_run_all_history(project.as_str());
        let run_all_task_key = format!("run-all-{}", project);

        append_run_all_history_line(
            history.as_ref(),
            format!(
                "[{}] [run-all] started project: {}\n",
                now_time_string(),
                project
            )
            .as_str(),
        );

        for task_key in task_keys {
            println!("[run-all] starting task: {} project: {}", task_key, project);

            append_run_all_history_line(
                history.as_ref(),
                format!(
                    "[{}] [run-all] starting task: {} project: {}\n",
                    now_time_string(),
                    task_key,
                    project
                )
                .as_str(),
            );

            let should_continue = run_task_from_run_all(project.as_str(), task_key.as_str()).await;

            if !should_continue {
                append_run_all_history_line(
                    history.as_ref(),
                    format!(
                        "[{}] [run-all] completed task: {} status: failed project: {}\n",
                        now_time_string(),
                        task_key,
                        project
                    )
                    .as_str(),
                );

                finish_run_all_history(history.as_ref(), false);
                send_task_status_with_running_and_message(
                    run_all_task_key.as_str(),
                    false,
                    Some("run-all"),
                    Some(project.as_str()),
                    false,
                    Some("Run-all stopped after a failed task."),
                );
                return;
            }

            append_run_all_history_line(
                history.as_ref(),
                format!(
                    "[{}] [run-all] completed task: {} status: success project: {}\n",
                    now_time_string(),
                    task_key,
                    project
                )
                .as_str(),
            );
        }

        append_run_all_history_line(
            history.as_ref(),
            format!(
                "[{}] [run-all] completed project: {}\n",
                now_time_string(),
                project
            )
            .as_str(),
        );
        finish_run_all_history(history.as_ref(), true);

        send_task_status_with_running_and_message(
            run_all_task_key.as_str(),
            true,
            Some("run-all"),
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

    let configured_task_keys = config_json
        .get("tasks")
        .and_then(|value| value.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_object())
                .filter_map(|row| {
                    let task_key = row
                        .get("task")
                        .and_then(|value| value.as_str())
                        .or_else(|| row.get("label").and_then(|value| value.as_str()))
                        .unwrap_or_default()
                        .trim()
                        .to_string();

                    if task_key.is_empty() {
                        return None;
                    }

                    let command = row
                        .get("command")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .trim()
                        .to_string();

                    if command.is_empty() {
                        return None;
                    }

                    Some(task_key)
                })
                .collect::<HashSet<String>>()
        })
        .unwrap_or_default();

    let mut tasks = Vec::<String>::new();
    for value in run_all {
        let Some(task_key) = value.as_str() else {
            continue;
        };

        let task_key = task_key.trim();
        if task_key.is_empty() {
            continue;
        }

        if !configured_task_keys.is_empty() && !configured_task_keys.contains(task_key) {
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
        "run-all" => format!("/log?project={project}&action=run-all"),
        "log" => format!("/log?project={project}"),
        custom => format!("/custom?project={project}&custom_label={custom}"),
    }
}

async fn run_task_from_run_all(project: &str, task_key: &str) -> bool {
    let mut rx = task_status_channel().subscribe();
    let task_result = start_run_task(project.to_string(), task_key.to_string(), None, None).await;

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

struct RunAllHistory {
    datestamp: String,
    task_key: String,
    project: String,
    started_at: String,
    branch: String,
    raw_path: PathBuf,
    meta_path: PathBuf,
}

fn create_run_all_history(project: &str) -> Option<RunAllHistory> {
    if project.trim().is_empty() {
        return None;
    }

    let base_dir = log_base_dir();
    if fs::create_dir_all(base_dir.as_path()).is_err() {
        return None;
    }

    let datestamp = Local::now().format("%Y%m%dT%H%M%S%.3f%z").to_string();
    let raw_path = base_dir.join(format!("{}.raw", datestamp));
    let meta_path = base_dir.join(format!("{}.json", datestamp));
    let started_at = Local::now().to_rfc3339_opts(SecondsFormat::Secs, false);
    let task_key = format!("run-all-{project}");
    let branch = current_branch(project).unwrap_or_default();

    let history = RunAllHistory {
        datestamp,
        task_key,
        project: project.to_string(),
        started_at,
        branch,
        raw_path,
        meta_path,
    };

    write_run_all_metadata(&history, "running", None);
    Some(history)
}

fn append_run_all_history_line(history: Option<&RunAllHistory>, line: &str) {
    let Some(history) = history else {
        return;
    };

    let mut file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(history.raw_path.as_path())
    {
        Ok(file) => file,
        Err(_) => return,
    };

    let _ = file.write_all(line.as_bytes());
}

fn finish_run_all_history(history: Option<&RunAllHistory>, ok: bool) {
    let Some(history) = history else {
        return;
    };

    let finished_at = Local::now().to_rfc3339_opts(SecondsFormat::Secs, false);
    let status = if ok { "success" } else { "fail" };
    write_run_all_metadata(history, status, Some(finished_at));
}

impl RunAllHistory {
    fn as_ref_metadata(&self, status: &str, finished_at: Option<String>) -> TaskHistoryMetadata {
        TaskHistoryMetadata {
            datestamp: self.datestamp.clone(),
            task_key: self.task_key.clone(),
            project: self.project.clone(),
            action: "run-all".to_string(),
            status: status.to_string(),
            started_at: self.started_at.clone(),
            finished_at,
            raw_file: self.raw_path.to_string_lossy().to_string(),
            branch: self.branch.clone(),
        }
    }
}

fn write_run_all_metadata(history: &RunAllHistory, status: &str, finished_at: Option<String>) {
    let metadata = history.as_ref_metadata(status, finished_at);
    let payload = match serde_json::to_vec_pretty(&metadata) {
        Ok(payload) => payload,
        Err(_) => return,
    };

    let _ = fs::write(history.meta_path.as_path(), payload);
}

fn now_time_string() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
