use std::collections::HashMap;
use std::sync::LazyLock;

use actix_files as fs;
use actix_web::{App, HttpServer, web};
use portable_pty::Child;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::RwLock;

mod abort;
mod build;
mod clean;
mod fetch;
mod index;
mod release;
mod runner;
mod settings;
mod terminal;
mod test;

pub struct AppData {
    running_project: String,
    running_task: Option<Task>,
    tasks: HashMap<String, Task>,
}

struct Task {
    history: Vec<u8>,
    running_command: Option<Arc<StdRwLock<Box<dyn Child + Send + Sync>>>>,
}

pub static APP_DATA: LazyLock<RwLock<AppData>> = LazyLock::new(|| {
    RwLock::new(AppData {
        tasks: HashMap::new(), //
        running_project: String::new(),
        running_task: None,
    })
});

impl AppData {
    fn start_task(
        &mut self,
        task_type: String,
        project_folder: String,
        command: Arc<StdRwLock<Box<dyn Child + Send + Sync>>>,
    ) {
        let key = format!("{}-{}", task_type, project_folder);

        println!("Starting task, key: {}", key);

        let task = Task {
            history: Vec::new(),
            running_command: Some(command),
        };

        self.tasks.insert(key, task);
    }
}

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
            .service(test::test)
            .service(release::release)
            .service(clean::clean)
            .service(terminal::terminal)
            .route("/ws", web::get().to(runner::websocket))
    })
    .bind(("127.0.0.1", 2003))?
    .run()
    .await
}
