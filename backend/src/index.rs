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
    let project_dir = PathBuf::from_str(BUILD_DIR);

    if project_dir.is_err() {
        println!("Can't find build dir");

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
                    <main>
                        <h1>Projects</h1>
                        <p>$BUILD_DIR no found</p>
                    </main>
                </body>
            </html>
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
                    <main>
                        <h1>Projects</h1>
                        <p>$BUILD_DIR</p>
                        <p>No config file found. Expecting ci.json in project folder</p>
                    </main>
                </body>
            </html>
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
                    <main>
                        <h1>Projects</h1>
                        <p>$BUILD_DIR</p>
                        <p>Failed to parse ci.json.</p>
                        <p>$err</p>
                    </main>
                </body>
            </html>
        }};

        return HttpResponse::Ok()
            .content_type(ContentType::html())
            .body(content);
    }

    let projects = vec![BUILD_DIR];

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
                <main>
                    <h1>Projects</h1>
                    <rust>
                        for project in projects {
                            <markup>
                                <project-status project="$project"></project-status>
                            </markup>
                        }
                    </rust>
                </main>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}
