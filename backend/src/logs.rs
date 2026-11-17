use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder, web};
use chrono::{DateTime, Local};
use kaja_html_macro::html;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::Path;

use crate::app::{TaskHistoryMetadata, log_base_dir};

#[derive(Deserialize)]
pub struct LogParams {
    pub project: Option<String>,
    pub file: Option<String>,
    pub action: Option<String>,
}

#[derive(Deserialize)]
pub struct HistoryParams {
    pub project: Option<String>,
    pub page: Option<usize>,
}

#[derive(Serialize)]
struct LatestLogResponse {
    ok: bool,
    found: bool,
    metadata: Option<TaskHistoryMetadata>,
}

#[actix_web::get("/log")]
pub async fn log_page(params: web::Query<LogParams>) -> impl Responder {
    let selected_project = params
        .project
        .clone()
        .or_else(|| {
            safe_history_file_name(params.file.as_deref())
                .and_then(|file_name| metadata_for_raw_file(file_name.as_str()))
                .map(|metadata| metadata.project)
        })
        .unwrap_or_default();

    let project = selected_project.clone();

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
                    <a class="top-bar-link" href="/history?project=$(project)">History</a>
                </nav>
                <console-component
                    project="$(selected_project)"
                    file="$(params.file.clone().unwrap_or_default())"
                    log_action="$(params.action.clone().unwrap_or_default())"
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
    } else if let (Some(project), Some(action)) =
        (params.project.as_deref(), params.action.as_deref())
    {
        latest_metadata_for_project_action(project, action)
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
    if let Some(file_name) = safe_history_file_name(params.file.as_deref()) {
        if let Some(metadata) = metadata_for_raw_file(file_name.as_str()) {
            let action = params.action.as_deref().unwrap_or_default().trim();
            let should_return_combined =
                action == "run-all" || (action.is_empty() && metadata.action == "run-all");

            if should_return_combined {
                let body = combined_raw_log_for_run_all(&metadata);

                if body.is_empty() {
                    return HttpResponse::NotFound()
                        .content_type(ContentType::plaintext())
                        .body("No log history found.");
                }

                return HttpResponse::Ok()
                    .content_type(ContentType::plaintext())
                    .body(body);
            }
        }

        let raw_path = log_base_dir().join(file_name);

        return match std::fs::read(raw_path.as_path()) {
            Ok(content) => HttpResponse::Ok()
                .content_type(ContentType::plaintext())
                .body(content),
            Err(err) => HttpResponse::InternalServerError()
                .content_type(ContentType::plaintext())
                .body(format!("Failed to read log file: {}", err)),
        };
    }

    let Some(project) = params.project.as_deref() else {
        return HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body("Missing query: file or project");
    };

    if let Some(action) = params.action.as_deref() {
        let Some(metadata) = latest_metadata_for_project_action(project, action) else {
            return HttpResponse::NotFound()
                .content_type(ContentType::plaintext())
                .body("No log history found.");
        };

        if action.trim() == "run-all" {
            let body = combined_raw_log_for_run_all(&metadata);

            if body.is_empty() {
                return HttpResponse::NotFound()
                    .content_type(ContentType::plaintext())
                    .body("No log history found.");
            }

            return HttpResponse::Ok()
                .content_type(ContentType::plaintext())
                .body(body);
        }

        return match std::fs::read(metadata.raw_file.as_str()) {
            Ok(content) => HttpResponse::Ok()
                .content_type(ContentType::plaintext())
                .body(content),
            Err(err) => HttpResponse::InternalServerError()
                .content_type(ContentType::plaintext())
                .body(format!("Failed to read log file: {}", err)),
        };
    }

    let combined = combined_raw_log_for_project(project);

    if combined.is_empty() {
        return HttpResponse::NotFound()
            .content_type(ContentType::plaintext())
            .body("No log history found.");
    }

    HttpResponse::Ok()
        .content_type(ContentType::plaintext())
        .body(combined)
}

const HISTORY_PAGE_SIZE: usize = 25;

#[actix_web::get("/history")]
pub async fn history_page(params: web::Query<HistoryParams>) -> impl Responder {
    let entries = metadata_for_project(params.project.as_deref());
    let rows = history_rows(entries.as_slice());

    let requested_page = params.page.unwrap_or(1).max(1);
    let total_rows = rows.len();
    let total_pages = total_rows.div_ceil(HISTORY_PAGE_SIZE).max(1);
    let current_page = requested_page.min(total_pages);
    let start = (current_page - 1) * HISTORY_PAGE_SIZE;
    let end = (start + HISTORY_PAGE_SIZE).min(total_rows);
    let visible_rows = if start < end {
        &rows[start..end]
    } else {
        &rows[0..0]
    };

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

                    <include history_table(visible_rows) />
                    <include history_pagination_controls(
                        params.project.as_deref(),
                        current_page,
                        total_pages,
                    ) />
                </main>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}

fn history_table(rows: &[HistoryRow]) -> String {
    html! {{
        <table>
            <thead>
                <tr>
                    <th>Timestamp</th>
                    <th>Project</th>
                    <th>Task</th>
                    <th>Status</th>
                    <th>Log file</th>
                </tr>
            </thead>
            <tbody>
                <rust>
                    for row in rows {
                        let status_class = row.status_class.as_str();

                        <markup>
                            <tr>
                                <td>$(row.timestamp)</td>
                                <td>$(row.project)</td>
                                <td>$(row.task)</td>
                                <td>
                                    <rust>
                                        if status_class.is_empty() {
                                            <markup>$(row.status)</markup>
                                        } else {
                                            <markup><span class="$status_class">$(row.status)</span></markup>
                                        }
                                    </rust>
                                </td>
                                <td><a href="$(row.href)">$(row.raw_name)</a></td>
                            </tr>
                        </markup>
                    }
                </rust>
            </tbody>
        </table>
    }}
}

struct HistoryRow {
    timestamp: String,
    project: String,
    task: String,
    status: String,
    status_class: String,
    href: String,
    raw_name: String,
}

fn history_pagination_controls(
    project: Option<&str>,
    current_page: usize,
    total_pages: usize,
) -> String {
    let prev_href = (current_page > 1).then(|| history_href(project, current_page - 1));
    let next_href = (current_page < total_pages).then(|| history_href(project, current_page + 1));

    html! {{
        <nav aria-label="History pagination">
            <p>Page $(current_page) of $(total_pages)</p>
            <div>
                <rust>
                    if let Some(href) = prev_href.as_deref() {
                        <markup><a class="top-bar-link" href="$(href)">Prev</a></markup>
                    } else {
                        <markup><span>Prev</span></markup>
                    }
                </rust>
                <rust>
                    if let Some(href) = next_href.as_deref() {
                        <markup><a class="top-bar-link" href="$(href)">Next</a></markup>
                    } else {
                        <markup><span>Next</span></markup>
                    }
                </rust>
            </div>
        </nav>
    }}
}

fn history_href(project: Option<&str>, page: usize) -> String {
    let mut href = format!("/history?page={}", page.max(1));

    if let Some(project) = project {
        let project = project.trim();

        if !project.is_empty() {
            href.push_str("&project=");
            href.push_str(encode_query_component(project).as_str());
        }
    }

    href
}

fn history_rows(entries: &[TaskHistoryMetadata]) -> Vec<HistoryRow> {
    let mut seen_raw_files = HashSet::new();
    let mut rows = Vec::new();

    for item in entries {
        let raw_name = raw_file_name(item.raw_file.as_str()).to_string();

        if !seen_raw_files.insert(raw_name.clone()) {
            continue;
        }

        let mut href = format!("/log?file={}", encode_query_component(raw_name.as_str()));

        if !item.action.trim().is_empty() {
            href.push_str("&action=");
            href.push_str(encode_query_component(item.action.as_str()).as_str());
        }

        let timestamp = item
            .finished_at
            .as_deref()
            .or(Some(item.started_at.as_str()))
            .map(format_local_time)
            .unwrap_or_else(|| "running".to_string());

        let status_class = if item.status == "success" {
            "terminal-fg-green"
        } else if item.status == "failed" || item.status == "fail" {
            "terminal-fg-red"
        } else {
            ""
        }
        .to_string();

        rows.push(HistoryRow {
            timestamp,
            project: item.project.clone(),
            task: normalized_task_name(item.action.as_str(), item.task_key.as_str()),
            status: item.status.clone(),
            status_class,
            href,
            raw_name,
        });
    }

    rows
}

pub fn latest_metadata_for_project(project: &str) -> Option<TaskHistoryMetadata> {
    metadata_for_project(Some(project)).into_iter().next()
}

pub fn latest_metadata_for_project_action(
    project: &str,
    action: &str,
) -> Option<TaskHistoryMetadata> {
    metadata_for_project_action(project, action)
        .into_iter()
        .next()
}

fn metadata_for_project(project: Option<&str>) -> Vec<TaskHistoryMetadata> {
    let mut records = all_history_metadata()
        .into_iter()
        .filter(|metadata| project.is_none_or(|project| metadata.project == project))
        .collect::<Vec<_>>();

    records.sort_by(|a, b| b.datestamp.cmp(&a.datestamp));
    records
}

fn metadata_for_project_action(project: &str, action: &str) -> Vec<TaskHistoryMetadata> {
    let normalized_action = action.trim();

    if normalized_action.is_empty() {
        return metadata_for_project(Some(project));
    }

    let mut records = all_history_metadata()
        .into_iter()
        .filter(|metadata| metadata.project == project)
        .filter(|metadata| metadata.action == normalized_action)
        .collect::<Vec<_>>();

    records.sort_by(|a, b| b.datestamp.cmp(&a.datestamp));
    records
}

fn combined_raw_log_for_project(project: &str) -> Vec<u8> {
    let mut seen_raw_files = HashSet::new();
    let mut combined = Vec::<u8>::new();

    for metadata in metadata_for_project(Some(project)) {
        if !seen_raw_files.insert(metadata.raw_file.clone()) {
            continue;
        }

        let Ok(content) = std::fs::read(metadata.raw_file.as_str()) else {
            continue;
        };

        append_combined_log_entry(&mut combined, &metadata, content.as_slice());
    }

    combined
}

fn combined_raw_log_for_run_all(run_all: &TaskHistoryMetadata) -> Vec<u8> {
    let mut seen_raw_files = HashSet::new();
    let mut combined = Vec::<u8>::new();

    let started_at = parse_rfc3339(run_all.started_at.as_str());
    let finished_at = run_all
        .finished_at
        .as_deref()
        .and_then(parse_rfc3339)
        .or_else(|| parse_rfc3339(run_all.started_at.as_str()));

    let mut selected = metadata_for_project(Some(run_all.project.as_str()))
        .into_iter()
        .filter(|metadata| {
            if !seen_raw_files.insert(metadata.raw_file.clone()) {
                return false;
            }

            metadata.action != "run-all"
                && is_within_run_all_window(metadata, started_at, finished_at)
        })
        .collect::<Vec<_>>();

    selected.sort_by(|a, b| a.datestamp.cmp(&b.datestamp));

    for metadata in selected {
        let Ok(content) = std::fs::read(metadata.raw_file.as_str()) else {
            continue;
        };

        append_combined_log_entry(&mut combined, &metadata, content.as_slice());
    }

    combined
}

fn append_combined_log_entry(target: &mut Vec<u8>, metadata: &TaskHistoryMetadata, content: &[u8]) {
    if !target.is_empty() {
        target.extend_from_slice(b"\n\n");
    }

    let header = format!(
        "===== {} ({}) {} =====\n",
        metadata.action, metadata.status, metadata.project
    );
    target.extend_from_slice(header.as_bytes());
    target.extend_from_slice(content);
}

fn is_within_run_all_window(
    metadata: &TaskHistoryMetadata,
    started_at: Option<DateTime<chrono::FixedOffset>>,
    finished_at: Option<DateTime<chrono::FixedOffset>>,
) -> bool {
    let Some(run_started_at) = started_at else {
        return false;
    };

    let Some(task_started_at) = parse_rfc3339(metadata.started_at.as_str()) else {
        return false;
    };

    if task_started_at < run_started_at {
        return false;
    }

    if let Some(run_finished_at) = finished_at {
        return task_started_at <= run_finished_at;
    }

    true
}

fn parse_rfc3339(value: &str) -> Option<DateTime<chrono::FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

fn metadata_for_raw_file(target_raw_file_name: &str) -> Option<TaskHistoryMetadata> {
    all_history_metadata()
        .into_iter()
        .find(|metadata| raw_file_name(metadata.raw_file.as_str()) == target_raw_file_name)
}

fn all_history_metadata() -> Vec<TaskHistoryMetadata> {
    let base_dir = log_base_dir();
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

fn encode_query_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());

    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push_str(format!("{:02X}", byte).as_str());
        }
    }

    output
}

fn safe_history_file_name(input: Option<&str>) -> Option<String> {
    let candidate = input?.trim().replace(' ', "+");

    if candidate.is_empty() || candidate.contains('/') || candidate.contains('\\') {
        return None;
    }

    if !candidate.ends_with(".raw") {
        return None;
    }

    Some(candidate)
}

fn raw_file_name(raw_file: &str) -> &str {
    Path::new(raw_file)
        .file_name()
        .and_then(|name| name.to_str())
        .or_else(|| raw_file.rsplit('/').next())
        .or_else(|| raw_file.rsplit('\\').next())
        .unwrap_or(raw_file)
}

fn normalized_task_name(action: &str, task_key: &str) -> String {
    let action = action.trim();

    if !action.is_empty() {
        return action.to_string();
    }

    task_key
        .split_once('-')
        .map(|(name, _)| name)
        .unwrap_or(task_key)
        .trim()
        .to_string()
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
