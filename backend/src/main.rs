use actix_files as fs;
use actix_web::{App, HttpServer, web};

mod abort;
mod app;
mod build;
mod clean;
mod fetch;
mod index;
mod release;
mod run;
mod runner;
mod settings;
mod status;
mod terminal;
mod test;

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
            .service(settings::load_settings)
            .service(abort::abort)
            .service(abort::abort_post)
            .service(test::test)
            .service(release::release)
            .service(clean::clean)
            .service(terminal::terminal)
            .service(run::run)
            .service(status::status)
            .route("/ws", web::get().to(runner::websocket))
    })
    .bind(("127.0.0.1", 2003))?
    .run()
    .await
}
