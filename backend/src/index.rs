use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder};
use kaja_html_macro::html;

use crate::runner::BUILD_DIR;

#[actix_web::get("/")]
pub async fn index() -> impl Responder {
    let content = html! {{
        <main>
            <h1>Projects</h1>
            <p>$BUILD_DIR</p>
            <p><a href="/build?project=$BUILD_DIR">Test</a></p>
            <p><a href="/fetch?project=$BUILD_DIR">Git Fetch</a></p>
        </main>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}
