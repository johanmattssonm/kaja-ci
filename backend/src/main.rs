use actix_files as fs;
use actix_web::{App, HttpServer, web};

mod build;
mod fetch;
mod index;
mod runner;
mod settings;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Starting Kaja CI build server");
    println!("WebSocket server: ws://127.0.0.1:2003/ws");
    println!("Frontend: http://127.0.0.1:2003/");

    HttpServer::new(|| {
        App::new()
            .service(fs::Files::new("/pkg", "../frontend/pkg"))
            .service(index::index)
            .service(build::build)
            .service(fetch::fetch)
            .service(settings::settings)
            .service(settings::save_settings)
            .route("/ws", web::get().to(runner::websocket))
    })
    .bind(("127.0.0.1", 2003))?
    .run()
    .await
}
