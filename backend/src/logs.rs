use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder, web};
use chrono::{DateTime, Local};
use kaja_html_macro::html;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::PathBuf;

use crate::app::TaskHistoryMetadata;

#[derive(Deserialize)]
pub struct LogParams {
    pub project: Option<String>,
    pub file: Option<String>,
}

#[derive(Deserialize)]
pub struct HistoryParams {
    pub project: Option<String>,
}

#[derive(Serialize)]
struct LatestLogResponse {
    ok: bool,
    found: bool,
    metadata: Option<TaskHistoryMetadata>,
}

#[actix_web::get("/log")]
pub async fn log_page(params: web::Query<LogParams>) -> impl Responder {
    let content = html! {{
        <!doctype html>
        <html lang="en">
            <head>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                <title>CI Log View</title>
                <script type="module" src="pkg/main.js" defer></script>
                <link rel="stylesheet" href="pkg/styles.css" />
            </head>

            <body>
                <nav class="top-bar" aria-label="Primary">
                    <a class="top-bar-link" href="/">Home</a>
                </nav>
                <console-component
                    project="$(params.project.clone().unwrap_or_default())"
                    file="$(params.file.clone().unwrap_or_default())"
                    action="log"
                ></console-component>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}

#[actix_web::get("/log/latest")]
pub async fn latest_log(params: web::Query<LogParams>) -> impl Responder {
    let metadata = if let Some(file_name) = safe_history_file_name(params.file.as_deref()) {
        metadata_for_raw_file(file_name.as_str())
    } else {
        params
            .project
            .as_deref()
            .and_then(latest_metadata_for_project)
    };

    HttpResponse::Ok().json(LatestLogResponse {
        ok: true,
        found: metadata.is_some(),
        metadata,
    })
}

#[actix_web::get("/log/raw")]
pub async fn latest_log_raw(params: web::Query<LogParams>) -> impl Responder {
    let raw_path = if let Some(file_name) = safe_history_file_name(params.file.as_deref()) {
        history_base_dir().join(file_name)
    } else {
        let Some(project) = params.project.as_deref() else {
            return HttpResponse::BadRequest()
                .content_type(ContentType::plaintext())
                .body("Missing query: file or project");
        };

        let Some(metadata) = latest_metadata_for_project(project) else {
            return HttpResponse::NotFound()
                .content_type(ContentType::plaintext())
                .body("No log history found.");
        };

        PathBuf::from(metadata.raw_file)
    };

    match std::fs::read(raw_path.as_path()) {
        Ok(content) => HttpResponse::Ok()
            .content_type(ContentType::plaintext())
            .body(content),
        Err(err) => HttpResponse::InternalServerError()
            .content_type(ContentType::plaintext())
            .body(format!("Failed to read log file: {}", err)),
    }
}

#[actix_web::get("/history")]
pub async fn history_page(params: web::Query<HistoryParams>) -> impl Responder {
    let entries = metadata_for_project(params.project.as_deref());

    let content = html! {{
        <!doctype html>
        <html lang="en">
            <head>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                <title>CI Log History</title>
                <link rel="stylesheet" href="pkg/styles.css" />
            </head>

            <body>
                <nav class="top-bar" aria-label="Primary">
                    <a class="top-bar-link" href="/">Home</a>
                </nav>
                <main>
                    <h1>Log history</h1>
                    <p>
                        <rust>
                            if let Some(project) = params.project.as_deref() {
                                <markup>Project: $(project)</markup>
                            } else {
                                <markup>All projects</markup>
                            }
                        </rust>
                    </p>
                    <div>
                        <rust>
                            let mut seen_raw_files = HashSet::new();

                            for item in entries {
                                let raw_name = item
                                    .raw_file
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(item.raw_file.as_str());

                                if !seen_raw_files.insert(raw_name.to_string()) {
                                    continue;
                                }

                                let href = format!("/log?file={}", raw_name);

                                let timestamp = item
                                    .finished_at
                                    .as_deref()
                                    .or(Some(item.started_at.as_str()))
                                    .map(format_local_time)
                                    .unwrap_or_else(|| "running".to_string());

                                let status_class = if item.status == "success" {
                                    "terminal-fg-green"
                                } else if item.status == "failed" {
                                    "terminal-fg-red"
                                } else {
                                    ""
                                };

                                <markup>
                                    <div>
                                        <a href="$href">$(raw_name)</a>
                                        <span> - $(timestamp)</span>
                                        <rust>
                                            if !status_class.is_empty() {
                                                <markup>
                                                    <span class="$status_class"> ($(item.status.clone()))</span>
                                                </markup>
                                            }
                                        </rust>
                                    </div>
                                </markup>
                            }
                        </rust>
                    </div>
                </main>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}

pub fn latest_metadata_for_project(project: &str) -> Option<TaskHistoryMetadata> {
    metadata_for_project(Some(project)).into_iter().next()
}

fn metadata_for_project(project: Option<&str>) -> Vec<TaskHistoryMetadata> {
    let mut records = all_history_metadata()
        .into_iter()
        .filter(|metadata| project.is_none_or(|project| metadata.project == project))
        .collect::<Vec<_>>();

    records.sort_by(|a, b| b.datestamp.cmp(&a.datestamp));
    records
}

fn metadata_for_raw_file(raw_file_name: &str) -> Option<TaskHistoryMetadata> {
    all_history_metadata().into_iter().find(|metadata| {
        PathBuf::from(metadata.raw_file.as_str())
            .file_name()
            .map(|name| name == OsStr::new(raw_file_name))
            .unwrap_or(false)
    })
}

fn all_history_metadata() -> Vec<TaskHistoryMetadata> {
    let base_dir = history_base_dir();
    let mut entries = match std::fs::read_dir(base_dir) {
        Ok(entries) => entries.flatten().collect::<Vec<_>>(),
        Err(_) => return Vec::new(),
    };

    entries.sort_by_key(|entry| entry.file_name());
    entries.reverse();

    let mut records = Vec::new();

    for entry in entries {
        let path = entry.path();

        if path.extension() != Some(OsStr::new("json")) {
            continue;
        }

        let Ok(payload) = std::fs::read(path.as_path()) else {
            continue;
        };

        let Ok(metadata) = serde_json::from_slice::<TaskHistoryMetadata>(payload.as_slice()) else {
            continue;
        };

        records.push(metadata);
    }

    records
}

fn safe_history_file_name(input: Option<&str>) -> Option<String> {
    let candidate = input?.trim();

    if candidate.is_empty() || candidate.contains('/') || candidate.contains('\\') {
        return None;
    }

    if !candidate.ends_with(".raw") {
        return None;
    }

    Some(candidate.to_string())
}

fn history_base_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".kaja-ci");
    }

    PathBuf::from(".kaja-ci")
}

fn format_local_time(input: &str) -> String {
    DateTime::parse_from_rfc3339(input)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| input.to_string())
}
