use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder};
use kaja_html_macro::html;

use crate::clone::{load_settings, save_settings};

#[actix_web::get("/")]
pub async fn index() -> impl Responder {
    let settings = load_settings();

    let (projects, settings_error) = match settings {
        Ok(mut settings) => {
            let previous_len = settings.projects.len();
            settings
                .projects
                .retain(|project| std::path::Path::new(project.as_str()).exists());

            let save_error = if settings.projects.len() != previous_len {
                save_settings(&settings).err()
            } else {
                None
            };

            (settings.projects, save_error)
        }
        Err(err) => (Vec::new(), Some(err)),
    };

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
                    <h1><span class="ci-icon">&#xE773;</span>Projects</h1>

                    <rust>
                        for project in projects.clone() {
                            <markup>
                                <project-status project="$project"></project-status>
                            </markup>
                        }
                    </rust>

                    <form action="/clone" method="get">
                        <input type="text" name="clone-command" placeholder="git clone https://github.com/org/repo.git" />
                        <button type="submit">Clone repository</button>
                    </form>

                    <form action="/add-project" method="get">
                        <input id="add-project-input" type="text" name="add-project" placeholder="/absolute/path/to/project" />
                        <button type="submit">Add project</button>
                        <button id="add-project-folder-picker" type="button" onclick="browseProjectFolder()">Choose folder</button>
                        <p id="add-project-folder-error"></p>
                    </form>

                    <rust>
                        if let Some(err) = settings_error.as_ref() {
                            <markup>
                                <br>
                                <p>Failed to load ~/.kaja-ci/settings.json</p>
                                <p>$err</p>
                            </markup>
                        }
                    </rust>

                    <rust>
                        if projects.is_empty() {
                            <markup>
                                <br>
                                <p>No projects found. Clone one above.</p>
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
