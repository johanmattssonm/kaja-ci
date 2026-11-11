use actix_files as fs;
use actix_web::http::header::ContentType;
use actix_web::{App, Error, HttpRequest, HttpResponse, HttpServer, Responder, web};
use actix_ws::{Message, Session, handle};
use kaja_html_macro::html;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{self, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

const BUILD_DIR: &str = "/home/johan/Downloads/resting-chair";

#[derive(Deserialize)]
struct ProjectGetParams {
    project: String,
}

async fn websocket(req: HttpRequest, body: web::Payload) -> Result<actix_web::HttpResponse, Error> {
    let (response, session, mut msg_stream) = handle(&req, body)?;
    let stdin_send = Arc::new(Mutex::new(None::<mpsc::Sender<String>>));

    actix_web::rt::spawn(async move {
        let mut session = session;
        while let Some(Ok(msg)) = msg_stream.recv().await {
            handle_message(msg, &mut session, stdin_send.clone()).await;
        }
    });

    Ok(response)
}

#[derive(Debug)]
struct Action {
    command: String,
    payload: String,
}

fn parse_action(line: String) -> Option<Action> {
    if !line.starts_with("ACTION ") {
        println!("Not an action.");
        return None;
    }

    let line = &line["ACTION ".len()..line.len()];
    let sepatator = line.find(" ");

    if sepatator.is_none() {
        println!("No separator after action.");
        return None;
    }

    let separator = sepatator.unwrap();

    Some(Action {
        command: line[0..sepatator.unwrap()].to_string(),
        payload: line[separator + 1..line.len()].to_string(),
    })
}

fn folder_exists(path: &str) -> bool {
    let path = PathBuf::from(path);

    if !path.is_dir() {
        return false;
    }

    match std::fs::metadata(&path) {
        Ok(metadata) => !metadata.permissions().readonly(),
        Err(_) => false,
    }
}

async fn process_action(
    text: String,
    session: &mut Session,
    stdin_tx: Arc<Mutex<Option<mpsc::Sender<String>>>>,
) {
    if stdin_tx.lock().await.is_some() {
        let _ = session
            .text("Console is already attached.".to_string())
            .await;
        return;
    }

    let action = parse_action(text);

    println!("Action: {:?}", action);

    if action.is_none() {
        println!("Malformed action");
        return;
    }

    let action = action.unwrap();

    if !folder_exists(&action.payload) {
        println!("Project folder does not exist or is not writable.");

        let _ = session
            .text("stdout: Can't find writable project folder.".to_string())
            .await;

        let _ = session.text("status: FAILED".to_string()).await;
        return;
    }

    let mut build_session = session.clone();
    let build_stdin_tx = stdin_tx.clone();

    actix_web::rt::spawn(async move {
        start_build(&mut build_session, build_stdin_tx, action).await;
    });
}

async fn handle_message(
    msg: Message,
    session: &mut Session,
    stdin_tx: Arc<Mutex<Option<mpsc::Sender<String>>>>,
) {
    match msg {
        Message::Text(text) => {
            let text = text.to_string();

            println!("WS: {}", text);

            if text.starts_with("ACTION ") {
                process_action(text, session, stdin_tx).await;
            } else if text.starts_with("STDIN ") {
                let input = &text["STDIN ".len()..text.len()];
                let build_stdin_tx = stdin_tx.clone();
                let tx = build_stdin_tx.lock().await.clone();

                if let Some(tx) = tx {
                    let _ = tx.try_send(input.to_string());
                }
            } else {
                let tx = stdin_tx.lock().await.clone();
                if let Some(tx) = tx {
                    let _ = tx.send(text).await;
                } else {
                    println!("Not an action: {}", text);
                }
            }
        }
        Message::Close(_) => {
            println!("WebSocket closed");
            return;
        }
        _ => {}
    }
}

fn command_to_executable(action: Action) -> Option<CommandBuilder> {
    match action.command.as_str() {
        "build" => {
            let mut cmd = CommandBuilder::new("/home/johan/Downloads/resting-chair/test.sh");
            cmd.cwd(action.payload);
            Some(cmd)
        }
        "fetch" => {
            let mut cmd = CommandBuilder::new("git");
            cmd.arg("fetch");
            cmd.arg("--all");
            cmd.cwd(action.payload);
            Some(cmd)
        }
        _ => {
            println!("Unknown action {}", action.command);
            None
        }
    }
}

async fn start_build(
    session: &mut Session,
    stdin_tx: Arc<Mutex<Option<mpsc::Sender<String>>>>,
    action: Action,
) {
    let _ = session.text("status: RUNNING".to_string()).await;

    let pty_system = native_pty_system();

    let pty_pair = match pty_system.openpty(PtySize {
        rows: 30,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(err) => {
            let _ = session.text("status: FAILED".to_string()).await;
            let _ = session
                .text(format!("failed to allocate PTY in {BUILD_DIR}: {err}"))
                .await;
            return;
        }
    };

    let cmd = command_to_executable(action);

    if cmd.is_none() {
        println!("No command to execute.");
        return;
    }

    let cmd = cmd.unwrap();

    // println!("Executing: {:?}", cmd);

    let mut child = match pty_pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(err) => {
            let _ = session.text("status: FAILED".to_string()).await;
            let _ = session
                .text(format!(
                    "failed to start build command in {BUILD_DIR}: {err}"
                ))
                .await;
            return;
        }
    };

    drop(pty_pair.slave);

    let reader = match pty_pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(err) => {
            let _ = session.text("status: FAILED".to_string()).await;
            let _ = session
                .text(format!("failed to open PTY reader: {err}"))
                .await;
            return;
        }
    };

    let mut writer = match pty_pair.master.take_writer() {
        Ok(writer) => writer,
        Err(err) => {
            let _ = session.text("status: FAILED".to_string()).await;
            let _ = session
                .text(format!("failed to open PTY writer: {err}"))
                .await;
            return;
        }
    };

    let (sender, mut receiver) = mpsc::channel::<String>(100);
    let tx = sender.clone();

    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0_u8; 4096];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).to_string();

                    if tx.blocking_send(chunk).is_err() {
                        break;
                    }
                }
                Err(err) if err.raw_os_error() == Some(5) => {
                    // EIO: normal PTY termination on Linux.
                    break;
                }
                Err(err) => {
                    let _ = tx.blocking_send("status: FAILED".to_string());
                    let _ = tx.blocking_send(format!("\r\nPTY read error: {err}\r\n"));
                    break;
                }
            }
        }
    });

    let (input_tx, mut input_rx) = mpsc::channel::<String>(100);

    {
        let mut slot = stdin_tx.lock().await;
        *slot = Some(input_tx);
    }

    // Write WebSocket input to PTY.
    std::thread::spawn(move || {
        while let Some(data) = input_rx.blocking_recv() {
            if writer.write_all(data.as_bytes()).is_err() {
                break;
            }

            if writer.write_all(b"\n").is_err() {
                break;
            }

            if writer.flush().is_err() {
                break;
            }
        }
    });

    drop(sender);

    while let Some(chunk) = receiver.recv().await {
        if session.text(chunk).await.is_err() {
            break;
        }
    }

    let status = child.wait();
    println!("Task ended: {:?}", status);

    if let Ok(status) = status {
        let exit_code = status.exit_code();
        let _ = session
            .text(format!("Task finished, exit code: {:?}", exit_code))
            .await;

        if exit_code == 0 {
            let _ = session.text("status: SUCCESS".to_string()).await;
        } else {
            let _ = session.text("status: FAILED".to_string()).await;
        }
    } else {
        let _ = session.text("status: FAILED".to_string()).await;
        let _ = session.text("Error".to_string()).await;
    }

    let mut slot = stdin_tx.lock().await;
    *slot = None;
}

#[actix_web::get("/")]
async fn index() -> impl Responder {
    let content = html! {{
        <main>
            <h1>Projects</h1>
            <p>$BUILD_DIR</p>
            <p><a href="/build?project=$BUILD_DIR">Test</a></p>
            <p><a href="/fetch?project=$BUILD_DIR">Git Fetch</a></p>
        </main>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}

#[actix_web::get("/build")]
async fn build(params: web::Query<ProjectGetParams>) -> impl Responder {
    let content = html! {{
        <!doctype html>
        <html lang="en">
            <head>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                <title>CI Build View</title>
                <script type="module" src="pkg/main.js" defer></script>
                <link rel="stylesheet" href="pkg/styles.css" />
            </head>

            <body>
                <h1 id="status">Loading</h1>
                <console-component project="$(params.project)" action="build"></console-component>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}

#[actix_web::get("/fetch")]
async fn fetch(params: web::Query<ProjectGetParams>) -> impl Responder {
    let content = html! {{
        <!doctype html>
        <html lang="en">
            <head>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0" />
                <title>CI Git Fetch</title>
                <script type="module" src="pkg/main.js" defer></script>
                <link rel="stylesheet" href="pkg/styles.css" />
            </head>

            <body>
                <h1 id="status">Updating Project</h1>
                <console-component project="$(params.project)" action="fetch"></console-component>
            </body>
        </html>
    }};

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Starting Kaja CI build server");
    println!("WebSocket server: ws://127.0.0.1:2003/ws");
    println!("Frontend: http://127.0.0.1:2003/");

    HttpServer::new(|| {
        App::new()
            .service(fs::Files::new("/pkg", "../frontend/pkg"))
            .service(index)
            .service(build)
            .service(fetch)
            .route("/ws", web::get().to(websocket))
    })
    .bind(("127.0.0.1", 2003))?
    .run()
    .await
}
