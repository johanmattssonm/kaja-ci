use actix_files as fs;
use actix_web::{App, HttpServer, web};
use std::io;
use std::path::PathBuf;

mod abort;
mod app;
mod branch;
mod build;
mod clean;
mod clone;
mod custom;
mod fetch;
mod index;
mod logs;
mod release;
mod run;
mod runner;
mod settings;
mod settingsfile;
mod status;
mod taskrunner;
mod terminal;
mod test;

fn frontend_pkg_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../frontend/pkg")
}

pub async fn run_server() -> io::Result<()> {
    settingsfile::ensure_kaja_ci_dirs_exist().map_err(io::Error::other)?;
    app::ensure_log_base_dir_exists()?;

    println!("Starting Kaja CI build server");
    println!("WebSocket server: ws://127.0.0.1:2003/runner");
    println!("Frontend: http://127.0.0.1:2003/");

    let pkg_dir = frontend_pkg_dir();

    HttpServer::new(move || {
        App::new()
            .service(fs::Files::new("/pkg", pkg_dir.clone()))
            .service(index::index)
            .service(index::clone_handler)
            .service(index::compact_clone_handler)
            .service(branch::branch)
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
            .service(run::run)
            .service(taskrunner::run)
            .service(status::status)
            .service(custom::custom)
            .service(logs::log_page)
            .service(logs::latest_log)
            .service(logs::latest_log_raw)
            .service(logs::history_page)
            .route("/runner", web::get().to(runner::websocket))
            .route("/task", web::get().to(taskrunner::websocket))
    })
    .bind(("127.0.0.1", 2003))?
    .run()
    .await
}
