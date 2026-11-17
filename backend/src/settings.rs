use std::fs;
use std::path::PathBuf;

use crate::runner::ProjectGetParams;
use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder, web};
use kaja_html_macro::html;

#[path = "../../frontend/src/rest.rs"]
mod rest;

use rest::*;

#[actix_web::get("/settings")]
pub async fn settings(params: web::Query<ProjectGetParams>) -> impl Responder {
    let mut project_dir = PathBuf::from(params.project.clone());
    project_dir.push("ci.json");

    let mut settings = BuildCommands::default();

    if let Ok(config_string) = std::fs::read_to_string(project_dir.clone()) {
        let config: std::result::Result<BuildCommands, _> = serde_json::from_str(&config_string);

        if let Ok(config) = config {
            settings = config;
        }
    }

    let commands_json = serde_json::to_string(&settings.custom_comands).unwrap();
    let default_tasks = vec!["git".to_string(), "build".to_string(), "test".to_string()];
    let tasks = settings.tasks.unwrap_or(default_tasks);
    let tasks_json = serde_json::to_string(&tasks).unwrap_or_else(|_| "[]".to_string());

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
                        tasks="$(tasks_json)"
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
    mut build_commands: web::Json<BuildCommands>,
) -> impl Responder {
    let mut project_dir = PathBuf::from(params.project.clone());
    project_dir.push("ci.json");

    if let Some(custom_commands) = build_commands.custom_comands.as_mut() {
        custom_commands.retain(|custom_command| {
            custom_command
                .command
                .as_deref()
                .is_some_and(|command| !command.trim().is_empty())
        });
    }

    if let Some(tasks) = build_commands.tasks.as_mut() {
        tasks.retain(|task| !task.trim().is_empty());
        tasks.dedup();
    }

    let update_settings = serde_json::to_string_pretty(&build_commands);

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

    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body("{ \"ok\": true }")
}

#[actix_web::get("/load-settings")]
pub async fn load_settings(params: web::Query<ProjectGetParams>) -> impl Responder {
    let mut project_dir = PathBuf::from(params.project.clone());
    project_dir.push("ci.json");

    if let Ok(config_string) = std::fs::read_to_string(project_dir.clone()) {
        return HttpResponse::Ok()
            .content_type(ContentType::json())
            .body(config_string);
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
    }

    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(content)
}
