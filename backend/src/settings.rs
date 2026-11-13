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
                </nav>

                <main>
                    <h1>Settings</h1>
                    <project-settings
                        project="$(params.project)"
                        git="$(settings.git.unwrap_or_default())"
                        build="$(settings.build.unwrap_or_default())"
                        test="$(settings.test.unwrap_or_default())"
                        release="$(settings.release.unwrap_or_default())"
                        clean="$(settings.clean.unwrap_or_default())"
                        terminal="$(settings.terminal.unwrap_or_default())"
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

    println!("Failed to load settings from {:?}", project_dir);

    return HttpResponse::BadRequest()
        .content_type(ContentType::json())
        .body("{ \"ok\": false, \"message\": \"Can't write settings file.\" }");
}
