#[actix_web::main]
async fn main() -> std::io::Result<()> {
    kaja_ci_backend::run_server().await
}
