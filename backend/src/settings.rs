use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use crate::runner::ProjectGetParams;
use crate::taskrunner;
use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder, web};
use kaja_html_macro::html;

#[path = "../../frontend/src/rest.rs"]
mod rest;

use rest::*;

#[actix_web::get("/settings")]
pub async fn settings_page(params: web::Query<ProjectGetParams>) -> impl Responder {
    let mut project_dir = PathBuf::from(params.project.clone());
    project_dir.push("ci.json");

    let settings = load_settings_for_ui(project_dir.as_path());

    let commands_json = serde_json::to_string(&settings.custom_comands).unwrap();
    let default_tasks = vec!["git".to_string(), "build".to_string(), "test".to_string()];
    let tasks = settings
        .run_all
        .clone()
        .or(settings.tasks)
        .unwrap_or(default_tasks);
    let tasks_json = serde_json::to_string(&tasks).unwrap_or_else(|_| "[]".to_string());
    let order_json = serde_json::to_string(&settings.order.clone().unwrap_or_default())
        .unwrap_or_else(|_| "{}".to_string());
    let scheduled_json =
        serde_json::to_string(&settings.scheduled_runs.clone().unwrap_or_default())
            .unwrap_or_else(|_| "[]".to_string());

    let content = html! {{
        <!doctype html>
        <html lang="en">
            <head>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                <title>CI Settings View</title>
                <script type="module" src="pkg/main.js" defer></script>
                <link rel="stylesheet" href="pkg/styles.css" />
            </head>
            <body>
                <nav class="top-bar" aria-label="Primary">
                    <a class="top-bar-link" href="/">Home</a>
                    <a class="top-bar-link" onclick="saveSettings(document.querySelector('project-settings')?.id || '')">Save</a>
                </nav>

                <main>
                    <h1>Settings</h1>
                    <project-settings
                        project="$(params.project)"
                        git="$(settings.git.unwrap_or_default())"
                        clone="$(settings.clone.unwrap_or_default())"
                        build="$(settings.build.unwrap_or_default())"
                        test="$(settings.test.unwrap_or_default())"
                        release="$(settings.release.unwrap_or_default())"
                        clean="$(settings.clean.unwrap_or_default())"
                        terminal="$(settings.terminal.unwrap_or_default())"
                        custom="$(commands_json)"
                        scheduled="$(scheduled_json)"
                        tasks="$(tasks_json)"
                        order="$(order_json)"
                    >
                    </project-settings>
                </main>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}

#[actix_web::post("/save-settings")]
pub async fn save_settings(
    params: web::Query<ProjectGetParams>,
    build_commands: web::Json<BuildCommands>,
) -> impl Responder {
    let mut project_dir = PathBuf::from(params.project.clone());
    project_dir.push("ci.json");

    let order_map = build_commands.order.clone().unwrap_or_default();
    let run_all = normalized_run_all(
        build_commands
            .run_all
            .clone()
            .or(build_commands.tasks.clone())
            .unwrap_or_default(),
        &order_map,
    );
    let task_rows = task_rows_from_build_commands(&build_commands);
    let scheduled_runs = build_commands.scheduled_runs.clone().unwrap_or_default();
    let settings_json = serde_json::json!({
        "tasks": task_rows,
        "run-all": run_all,
        "scheduled-run": scheduled_runs
    });

    let update_settings = serde_json::to_string_pretty(&settings_json);

    if update_settings.is_err() {
        println!("Failed to save settings. Invalid JSON.");

        return HttpResponse::BadRequest()
            .content_type(ContentType::json())
            .body("{ \"ok\": false, \"message\": \"Invalid JSON\" }");
    }

    let content = update_settings.unwrap();

    println!(
        "Saving settings {} {}",
        content,
        project_dir.to_str().unwrap()
    );

    let result = fs::write(project_dir, content.as_str());

    if result.is_err() {
        println!("Failed to save settings.");

        return HttpResponse::BadRequest()
            .content_type(ContentType::json())
            .body("{ \"ok\": false, \"message\": \"Can't write settings file.\" }");
    }

    taskrunner::notify_scheduler_reload();

    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body("{ \"ok\": true }")
}

#[actix_web::get("/load-settings")]
pub async fn load_settings(params: web::Query<ProjectGetParams>) -> impl Responder {
    let mut project_dir = PathBuf::from(params.project.clone());
    project_dir.push("ci.json");

    if std::fs::metadata(project_dir.as_path()).is_ok() {
        let settings = load_settings_for_ui(project_dir.as_path());

        let content = match serde_json::to_string_pretty(&settings) {
            Ok(content) => content,
            Err(err) => {
                println!(
                    "Failed to serialize settings for {:?}: {}",
                    project_dir, err
                );
                return HttpResponse::InternalServerError()
                    .content_type(ContentType::json())
                    .body("{ \"ok\": false, \"message\": \"Failed to read settings.\" }");
            }
        };

        return HttpResponse::Ok()
            .content_type(ContentType::json())
            .body(content);
    }

    let defaults = BuildCommands::default();
    let content = match serde_json::to_string_pretty(&defaults) {
        Ok(content) => content,
        Err(err) => {
            println!(
                "Failed to serialize default settings for {:?}: {}",
                project_dir, err
            );
            return HttpResponse::InternalServerError()
                .content_type(ContentType::json())
                .body("{ \"ok\": false, \"message\": \"Failed to create default settings.\" }");
        }
    };

    if let Err(err) = fs::write(project_dir.as_path(), content.as_str()) {
        println!(
            "Failed to write default settings to {:?}: {}",
            project_dir, err
        );
    } else {
        taskrunner::notify_scheduler_reload();
    }

    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(content)
}

fn load_settings_for_ui(project_path: &std::path::Path) -> BuildCommands {
    let defaults = BuildCommands::default();

    let config_string = match std::fs::read_to_string(project_path) {
        Ok(contents) => contents,
        Err(_) => return defaults,
    };

    let parsed: BuildCommands = serde_json::from_str(config_string.as_str()).unwrap_or_default();

    let mut settings = parsed;
    if settings
        .terminal
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        settings.terminal = Some("bash".to_string());
    }

    let config_json = match serde_json::from_str::<serde_json::Value>(config_string.as_str()) {
        Ok(json) => json,
        Err(_) => return settings,
    };

    let Some(tasks) = config_json.get("tasks").and_then(|value| value.as_array()) else {
        return settings;
    };

    let mut task_names = Vec::<String>::new();
    let mut order = HashMap::<String, i32>::new();
    let mut commands = HashMap::<String, String>::new();

    for (index, value) in tasks.iter().enumerate() {
        let Some(map) = value.as_object() else {
            continue;
        };

        let task = map
            .get("task")
            .and_then(|value| value.as_str())
            .or_else(|| map.get("label").and_then(|value| value.as_str()))
            .unwrap_or_default()
            .trim()
            .to_string();

        if task.is_empty() {
            continue;
        }

        let task_order = map
            .get("order")
            .and_then(|value| value.as_i64())
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or((index as i32) + 1);

        if !task_names.iter().any(|name| name == &task) {
            task_names.push(task.clone());
        }

        order.insert(task.clone(), task_order);

        if let Some(command) = map.get("command").and_then(|value| value.as_str()) {
            let command = command.trim();
            if !command.is_empty() {
                commands.insert(task, command.to_string());
            }
        }
    }

    if let Some(command) = commands.get("git") {
        settings.git = Some(command.clone());
    }
    if let Some(command) = commands.get("clone") {
        settings.clone = Some(command.clone());
    }
    if let Some(command) = commands.get("build") {
        settings.build = Some(command.clone());
    }
    if let Some(command) = commands.get("test") {
        settings.test = Some(command.clone());
    }
    if let Some(command) = commands.get("release") {
        settings.release = Some(command.clone());
    }
    if let Some(command) = commands.get("clean") {
        settings.clean = Some(command.clone());
    }
    if let Some(command) = commands.get("terminal") {
        settings.terminal = Some(command.clone());
    }

    let built_in = [
        "git", "clone", "build", "test", "release", "clean", "terminal",
    ];
    let derived_custom_commands = task_names
        .iter()
        .filter(|task| !built_in.contains(&task.as_str()))
        .filter_map(|task| {
            commands.get(task).map(|command| CustomCommand {
                label: Some(task.clone()),
                command: Some(command.clone()),
            })
        })
        .collect::<Vec<CustomCommand>>();

    let mut custom_by_label = HashMap::<String, String>::new();
    for custom in settings.custom_comands.clone().unwrap_or_default() {
        let Some(label) = custom.label else {
            continue;
        };

        let label = label.trim();
        if label.is_empty() {
            continue;
        }

        let command = custom.command.unwrap_or_default().trim().to_string();
        if command.is_empty() {
            continue;
        }

        custom_by_label.insert(label.to_string(), command);
    }

    for custom in derived_custom_commands {
        let Some(label) = custom.label else {
            continue;
        };

        let label = label.trim();
        if label.is_empty() {
            continue;
        }

        let command = custom.command.unwrap_or_default().trim().to_string();
        if command.is_empty() {
            continue;
        }

        custom_by_label.insert(label.to_string(), command);
    }

    let mut custom_commands = custom_by_label
        .into_iter()
        .map(|(label, command)| CustomCommand {
            label: Some(label),
            command: Some(command),
        })
        .collect::<Vec<CustomCommand>>();

    custom_commands.sort_by(|a, b| {
        let a_label = a.label.as_deref().unwrap_or_default();
        let b_label = b.label.as_deref().unwrap_or_default();
        a_label.cmp(b_label)
    });

    settings.custom_comands = Some(custom_commands);
    settings.tasks = Some(task_names);
    settings.order = Some(order);

    let run_all_missing = settings
        .run_all
        .as_ref()
        .map(|value| value.is_empty())
        .unwrap_or(true);
    if run_all_missing {
        settings.run_all = settings.tasks.clone();
    }

    settings
}

fn task_rows_from_build_commands(build_commands: &BuildCommands) -> Vec<serde_json::Value> {
    let order_map = build_commands.order.clone().unwrap_or_default();

    let mut rows = Vec::<(String, String, bool, usize)>::new();
    let mut seen = HashSet::<String>::new();

    let built_in_commands = vec![
        ("git", build_commands.git.clone().unwrap_or_default()),
        ("clone", build_commands.clone.clone().unwrap_or_default()),
        ("build", build_commands.build.clone().unwrap_or_default()),
        ("test", build_commands.test.clone().unwrap_or_default()),
        (
            "release",
            build_commands.release.clone().unwrap_or_default(),
        ),
        ("clean", build_commands.clean.clone().unwrap_or_default()),
        (
            "terminal",
            build_commands.terminal.clone().unwrap_or_default(),
        ),
    ];

    for (name, command) in built_in_commands {
        let task_name = name.trim().to_string();
        let command = command.trim().to_string();
        if command.is_empty() || !seen.insert(task_name.clone()) {
            continue;
        }

        let index = rows.len();
        rows.push((task_name, command, false, index));
    }

    for custom in build_commands.custom_comands.clone().unwrap_or_default() {
        let Some(label) = custom.label else {
            continue;
        };

        let label = label.trim().to_string();
        if label.is_empty() || !seen.insert(label.clone()) {
            continue;
        }

        let command = custom.command.unwrap_or_default().trim().to_string();
        if command.is_empty() {
            continue;
        }

        let index = rows.len();
        rows.push((label, command, true, index));
    }

    let mut ordered_rows = rows
        .into_iter()
        .map(|(task_name, command, custom, index)| {
            let order = order_map
                .get(task_name.as_str())
                .copied()
                .unwrap_or((index as i32) + 1);
            (task_name, command, custom, order, index)
        })
        .collect::<Vec<(String, String, bool, i32, usize)>>();

    ordered_rows.sort_by(
        |(task_a, _, _, order_a, index_a), (task_b, _, _, order_b, index_b)| {
            order_a
                .cmp(order_b)
                .then(index_a.cmp(index_b))
                .then(task_a.cmp(task_b))
        },
    );

    ordered_rows
        .into_iter()
        .map(|(task_name, command, custom, _order, _)| {
            if custom {
                serde_json::json!({
                    "label": task_name,
                    "command": command
                })
            } else {
                serde_json::json!({
                    "task": task_name,
                    "command": command
                })
            }
        })
        .collect::<Vec<serde_json::Value>>()
}

fn normalized_run_all(run_all: Vec<String>, order_map: &HashMap<String, i32>) -> Vec<String> {
    let mut deduped = Vec::<(String, i32, usize)>::new();
    let mut seen = HashSet::<String>::new();

    for (index, task) in run_all.into_iter().enumerate() {
        let task = task.trim().to_string();
        if task.is_empty() || !seen.insert(task.clone()) {
            continue;
        }

        let order = order_map
            .get(task.as_str())
            .copied()
            .unwrap_or((index as i32) + 1);

        deduped.push((task, order, index));
    }

    deduped.sort_by(|(task_a, order_a, index_a), (task_b, order_b, index_b)| {
        order_a
            .cmp(order_b)
            .then(index_a.cmp(index_b))
            .then(task_a.cmp(task_b))
    });

    deduped
        .into_iter()
        .map(|(task, _, _)| task)
        .collect::<Vec<String>>()
}
