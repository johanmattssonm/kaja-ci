use actix_web::http::header::ContentType;
use actix_web::{HttpResponse, Responder, web};
use git2::{ErrorCode, Repository};
use serde::Serialize;

use crate::runner::ProjectGetParams;

#[derive(Serialize)]
struct BranchResponse {
    ok: bool,
    branch: String,
    message: Option<String>,
}

#[actix_web::get("/branch")]
pub async fn branch(params: web::Query<ProjectGetParams>) -> impl Responder {
    match current_branch(params.project.as_str()) {
        Ok(branch_name) => {
            HttpResponse::Ok()
                .content_type(ContentType::json())
                .json(BranchResponse {
                    ok: true,
                    branch: branch_name,
                    message: None,
                })
        }
        Err(message) => HttpResponse::BadRequest()
            .content_type(ContentType::json())
            .json(BranchResponse {
                ok: false,
                branch: String::new(),
                message: Some(message),
            }),
    }
}

pub fn current_branch(project_path: &str) -> Result<String, String> {
    let repo = Repository::open(project_path).map_err(|error| {
        format!(
            "Failed to open repository '{}': {}",
            project_path,
            error.message()
        )
    })?;

    let head = repo.head().map_err(|error| {
        if error.code() == ErrorCode::UnbornBranch {
            return "Repository has no commits yet (unborn branch).".to_string();
        }

        format!("Failed to read HEAD: {}", error.message())
    })?;

    if head.is_branch() {
        return Ok(head.shorthand().unwrap_or("unknown").to_string());
    }

    if let Some(target) = head.target() {
        let short = target.to_string();
        let short = short.chars().take(7).collect::<String>();
        return Ok(format!("detached@{}", short));
    }

    Ok("detached".to_string())
}
