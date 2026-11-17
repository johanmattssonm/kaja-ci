use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder, web};
use kaja_html_macro::html;

use crate::runner::ProjectGetParams;

#[actix_web::get("/fetch")]
pub async fn fetch(params: web::Query<ProjectGetParams>) -> impl Responder {
    let content = html! {{
        <!doctype html>
        <html lang="en">
            <head>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                <title>CI Git Fetch</title>
                <script type="module" src="pkg/main.js" defer></script>
                <link rel="stylesheet" href="pkg/styles.css" />
            </head>

            <body>
                <nav class="top-bar" aria-label="Primary">
                    <a class="top-bar-link" href="/">Home</a>
                    <a class="top-bar-link" href="#" data-project="$(params.project)" data-action="git" onclick="startTask(this.dataset.project, this.dataset.action)">Rerun</a>
                    <a class="top-bar-link" href="#" data-project="$(params.project)" onclick="abort(this.dataset.project)">Abort</a>
                </nav>
                <console-component project="$(params.project)" action="git"></console-component>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}
