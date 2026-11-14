use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder, web};
use kaja_html_macro::html;

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
                    <a class="top-bar-link" href="#" data-project="$(params.project)" data-action="$(custom_label)" onclick="startTask(this.dataset.project, this.dataset.action)">Run</a>
                    <a class="top-bar-link" href="#" data-project="$(params.project)" onclick="abort(this.dataset.project)">Abort</a>
                </nav>
                <console-component project="$(params.project)" action="$(custom_label)"></console-component>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}
