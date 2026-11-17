use actix_web::http::header::{ContentType, LOCATION};
use actix_web::{HttpResponse, Responder, web};
use kaja_html_macro::html;
use serde::Deserialize;

use crate::clone::add_existing_project;

#[derive(Debug, Deserialize)]
pub struct AddProjectParams {
    #[serde(rename = "add-project")]
    pub add_project: Option<String>,
}

#[actix_web::get("/add-project")]
pub async fn add_project(params: web::Query<AddProjectParams>) -> impl Responder {
    let project_path = params.add_project.as_deref().unwrap_or_default();

    match add_existing_project(project_path) {
        Ok(path) => {
            let location = format!(
                "/settings?project={}",
                encode_query_component(path.to_string_lossy().as_ref())
            );

            HttpResponse::SeeOther()
                .insert_header((LOCATION, location))
                .finish()
        }
        Err(err) => {
            let content = html! {{
                <!doctype html>
                <html lang="en">
                    <head>
                        <meta charset="UTF-8" />
                        <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                        <title>Failed to add project</title>
                        <script type="module" src="pkg/main.js" defer></script>
                        <link rel="stylesheet" href="pkg/styles.css" />
                    </head>
                    <body>
                        <main>
                            <h1>Failed to add project path</h1>
                            <p>$err</p>
                            <a href="/">Back</a>
                        </main>
                    </body>
                </html>
            }};

            HttpResponse::BadRequest()
                .content_type(ContentType::html())
                .body(content)
        }
    }
}

fn encode_query_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());

    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push_str(format!("{:02X}", byte).as_str());
        }
    }

    output
}
