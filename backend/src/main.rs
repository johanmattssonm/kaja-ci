#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let session_key = std::env::var("KAJA_CI_SESSION_KEY").unwrap_or_default();
    kaja_ci_backend::run_server(session_key).await
}
