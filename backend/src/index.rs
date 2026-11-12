use std::path::PathBuf;
use std::str::FromStr;

use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder};
use kaja_html_macro::html;

use crate::runner::BUILD_DIR;

#[path = "../../frontend/src/rest.rs"]
mod rest;

use rest::BuildCommands;

#[actix_web::get("/")]
pub async fn index() -> impl Responder {
    let mut project_dir = PathBuf::from_str(BUILD_DIR);

    if project_dir.is_err() {
        println!("Can't find build dir");

        let content = html! {{
            <main>
                <h1>Projects</h1>
                <p>$BUILD_DIR no found</p>
            </main>
        }};

        return HttpResponse::Ok()
            .content_type(ContentType::html())
            .body(content);
    }

    let mut project_dir = project_dir.unwrap();
    project_dir.push("ci.json");

    let config_string = std::fs::read_to_string(project_dir.clone());

    if config_string.is_err() {
        print!("No ci.json found in {}", project_dir.display());

        let content = html! {{
            <main>
                <h1>Projects</h1>
                <p>$BUILD_DIR</p>
                <p>No config file found. Expecting ci.json in project folder</p>
            </main>
        }};

        return HttpResponse::Ok()
            .content_type(ContentType::html())
            .body(content);
    }

    let config_string = config_string.unwrap();
    let config: std::result::Result<BuildCommands, _> = serde_json::from_str(&config_string);

    if config.is_err() {
        let err = config.unwrap_err();

        let content = html! {{
            <main>
                <h1>Projects</h1>
                <p>$BUILD_DIR</p>
                <p>Failed to parse ci.json.</p>
                <p>$err</p>
            </main>
        }};

        return HttpResponse::Ok()
            .content_type(ContentType::html())
            .body(content);
    }

    let content = html! {{
        <main>
            <h1>Projects</h1>
            <p>$BUILD_DIR</p>
            <p><a href="/fetch?project=$BUILD_DIR">Git Fetch</a></p>
            <p><a href="/build?project=$BUILD_DIR">Build</a></p>
            <p><a href="/test?project=$BUILD_DIR">Test</a></p>
            <p><a href="/release?project=$BUILD_DIR">Release</a></p>
            <p><a href="/clean?project=$BUILD_DIR">Clean</a></p>
            <p><a href="/log?project=$BUILD_DIR">Log</a></p>
            <p><a href="/settings?project=$BUILD_DIR">Settings</a></p>
        </main>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}
