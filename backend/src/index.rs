use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder};
use kaja_html_macro::html;

use crate::clone::{
    CloneParams, clone_command_from_input, clone_target_dir_from_command, load_settings,
    save_settings,
};
use crate::settingsfile::kaja_ci_projects_dir;

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
                    <h1>Projects</h1>

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

pub(crate) fn clone_page(params: &CloneParams, compact: bool) -> HttpResponse {
    let mut project = params.project.clone().unwrap_or_default();
    let mut success_message = String::new();
    let mut error_message = String::new();
    let mut clone_command = String::new();
    let mut clone_target = String::new();
    let mut auto_start = false;
    let repo_input = params.repo.as_ref().or(params.clone_command.as_ref());

    if project.is_empty() {
        project = kaja_ci_projects_dir()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
    }

    if let Some(repo_input) = repo_input {
        match clone_command_from_input(repo_input) {
            Ok(command) => {
                clone_command = command;
                auto_start = true;
                success_message = "Ready to run clone command".to_string();

                if !project.is_empty() {
                    let project_folder = std::path::Path::new(project.as_str());
                    if let Ok(target_dir) =
                        clone_target_dir_from_command(clone_command.as_str(), project_folder)
                    {
                        clone_target = target_dir.to_string_lossy().to_string();
                    }
                }
            }
            Err(err) => {
                error_message = err;
            }
        }
    }

    let title = if compact {
        "CI Clone"
    } else {
        "Clone repository"
    };

    let content = html! {{
        <!doctype html>
        <html lang="en">
            <head>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                <title>$title</title>
                <script type="module" src="pkg/main.js" defer></script>
                <link rel="stylesheet" href="pkg/styles.css" />
            </head>

            <body>
                <nav class="top-bar" aria-label="Primary">
                    <a class="top-bar-link" href="/">Home</a>
                    <rust>
                        if !project.is_empty() {
                            <markup>
                                <a class="top-bar-link" href="#" data-project="$project" data-action="clone" data-command="$clone_command" onclick="startTaskWithCommand(this.dataset.project, this.dataset.action, this.dataset.command)">Rerun</a>
                                <a class="top-bar-link" href="#" data-project="$project" onclick="abort(this.dataset.project)">Abort</a>
                            </markup>
                        }
                    </rust>
                </nav>

                <main>
                    <h1>$title</h1>

                    <rust>
                        if !success_message.is_empty() {
                            <markup>
                                <p>$success_message</p>
                            </markup>
                        }
                    </rust>

                    <rust>
                        if !error_message.is_empty() {
                            <markup>
                                <p>$error_message</p>
                            </markup>
                        }
                    </rust>

                    <console-component project="$project" action="clone" command="$clone_command" clone_target="$clone_target" auto_start="$auto_start"></console-component>
                </main>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}
