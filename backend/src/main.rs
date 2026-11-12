use std::collections::HashMap;
use std::sync::LazyLock;

use actix_files as fs;
use actix_web::{App, HttpServer, web};
use portable_pty::Child;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::RwLock;

mod abort;
mod build;
mod fetch;
mod index;
mod runner;
mod settings;

pub struct AppData {
    tasks: HashMap<String, Task>,
}

pub struct Task {
    running_command: Option<Arc<StdRwLock<Box<dyn Child + Send + Sync>>>>,
}

pub static APP_DATA: LazyLock<RwLock<AppData>> = LazyLock::new(|| {
    RwLock::new(AppData {
        tasks: HashMap::new(),
    })
});

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
            .route("/ws", web::get().to(runner::websocket))
    })
    .bind(("127.0.0.1", 2003))?
    .run()
    .await
}
