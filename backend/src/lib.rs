use actix_files as fs;
use actix_web::dev::Service;
use actix_web::{App, HttpResponse, HttpServer, web};
use futures_util::future::{Either, ready};
use std::io;
use std::path::PathBuf;

mod abort;
mod addproject;
mod app;
mod auth;
mod branch;

mod clean;
mod clone;
mod custom;

mod index;
mod logs;

mod run;
mod runner;
mod settings;
mod settingsfile;
mod status;
mod taskrunner;
mod terminal;

fn frontend_pkg_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../frontend/pkg")
}

fn font_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../font")
}

fn favicon_ico_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../public/favicon.ico")
}

pub async fn run_server(session_key: String) -> io::Result<()> {
    settingsfile::ensure_kaja_ci_dirs_exist().map_err(io::Error::other)?;
    app::ensure_log_base_dir_exists()?;
    taskrunner::start_scheduler();

    let pkg_dir = frontend_pkg_dir();
    let fonts_dir = font_dir();
    let favicon_ico = favicon_ico_path();
    let auth_state = web::Data::new(auth::AppAuth::new(session_key));

    HttpServer::new(move || {
        App::new()
            .app_data(auth_state.clone())
            .wrap_fn(|req, srv| {
                let path = req.path().to_string();
                let public_path = path == "/"
                    || path.starts_with("/pkg/")
                    || path == "/pkg"
                    || path.starts_with("/font/")
                    || path == "/font"
                    || path == "/favicon.ico";

                if public_path {
                    Either::Left(srv.call(req))
                } else if let Some(message) = auth::authorization_failure_message(req.request()) {
                    Either::Right(ready(Ok(
                        req.into_response(HttpResponse::Unauthorized().body(message))
                    )))
                } else {
                    Either::Left(srv.call(req))
                }
            })
            .service(fs::Files::new("/pkg", pkg_dir.clone()))
            .service(fs::Files::new("/font", fonts_dir.clone()))
            .route(
                "/favicon.ico",
                web::get().to({
                    let favicon_ico = favicon_ico.clone();
                    move || {
                        let favicon_ico = favicon_ico.clone();
                        async move { fs::NamedFile::open_async(favicon_ico).await }
                    }
                }),
            )
            .service(index::index)
            .service(auth::establish_session)
            .service(addproject::add_project)
            .service(clone::clone_handler)
            .service(clone::compact_clone_handler)
            .service(branch::branch)
            .service(settings::settings_page)
            .service(settings::save_settings)
            .service(settings::load_settings)
            .service(abort::abort)
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
