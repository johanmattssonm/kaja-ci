use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder, web};

use kaja_html_macro::html;

use crate::branch::{current_branch_or_label, latest_commit_or_empty};
use crate::logs::latest_metadata_for_project_action;
use crate::runner::ProjectGetParams;

#[actix_web::get("/custom")]
pub async fn custom(params: web::Query<ProjectGetParams>) -> impl Responder {
    if params.custom_label.is_none() {
        let content = html! {{
            <!doctype html>
            <html lang="en">
                <head>
                    <meta charset="UTF-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                    <title>CI Build View</title>
                    <script type="module" src="pkg/main.js" defer></script>
                    <link rel="stylesheet" href="pkg/styles.css" />
                </head>

                <body>
                    <nav class="top-bar" aria-label="Primary">
                        <a class="top-bar-link" href="/">Home</a>
                    </nav>
                    <main><h1>Missing custom_label</h1></main>
                </body>
            </html>
        }};

        return HttpResponse::Ok()
            .content_type(ContentType::html())
            .body(content);
    }

    let custom_label = params
        .custom_label
        .as_deref()
        .expect("Parameter custom_label needed.");

    let branch = current_branch_or_label(params.project.as_str());
    let latest_commit = latest_commit_or_empty(params.project.as_str());
    let latest_built_commit =
        latest_metadata_for_project_action(params.project.as_str(), custom_label)
            .map(|metadata| metadata.latest_built_commit)
            .unwrap_or_default();

    let content = html! {{
        <!doctype html>
        <html lang="en">
            <head>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                <title>CI</title>
                <script type="module" src="pkg/main.js" defer></script>
                <link rel="stylesheet" href="pkg/styles.css" />
            </head>

            <body>
                <nav class="top-bar" aria-label="Primary">
                    <a class="top-bar-link" href="/">Home</a>
                    <a class="top-bar-link" href="#" data-project="$(params.project)" data-action="$(custom_label)" onclick="startTask(this.dataset.project, this.dataset.action)">Rerun</a>
                    <a class="top-bar-link" href="#" data-project="$(params.project)" onclick="abort(this.dataset.project)">Abort</a>
                </nav>
                <console-component
                    project="$(params.project)"
                    action="$(custom_label)"
                    branch="$branch"
                    latest_commit="$latest_commit"
                    latest_built_commit="$latest_built_commit"
                ></console-component>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}
