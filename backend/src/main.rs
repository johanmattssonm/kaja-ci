use actix_files as fs;
use actix_web::http::header::ContentType;
use actix_web::{App, Error, HttpRequest, HttpResponse, HttpServer, Responder, web};
use actix_ws::{Message, Session, handle};
use kaja_html_macro::html;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

const BUILD_DIR: &str = "/home/johan/Downloads/resting-chair";

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

async fn handle_message(
    msg: Message,
    session: &mut Session,
    stdin_tx: Arc<Mutex<Option<mpsc::Sender<String>>>>,
) {
    match msg {
        Message::Text(text) => {
            let text = text.to_string();

            if text == "BUILD" {
                if stdin_tx.lock().await.is_some() {
                    let _ = session.text("Build already running".to_string()).await;
                    return;
                }

                let mut build_session = session.clone();
                let build_stdin_tx = stdin_tx.clone();
                actix_web::rt::spawn(async move {
                    start_build(&mut build_session, build_stdin_tx).await;
                });
                return;
            }

            let tx = stdin_tx.lock().await.clone();
            if let Some(tx) = tx {
                let _ = tx.send(text).await;
            }
        }
        Message::Close(_) => {
            println!("WebSocket closed");
            return;
        }
        _ => {}
    }
}

async fn start_build(session: &mut Session, stdin_tx: Arc<Mutex<Option<mpsc::Sender<String>>>>) {
    let _ = session.text("Running\r\n".to_string()).await;

    let pty_system = native_pty_system();

    let pty_pair = match pty_system.openpty(PtySize {
        rows: 30,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(err) => {
            let _ = session
                .text(format!("failed to allocate PTY in {BUILD_DIR}: {err}"))
                .await;
            return;
        }
    };

    let mut cmd = CommandBuilder::new("git");
    cmd.arg("pull");
    cmd.cwd(BUILD_DIR);

    let mut child = match pty_pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(err) => {
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
            let _ = session
                .text(format!("failed to open PTY reader: {err}"))
                .await;
            return;
        }
    };

    let mut writer = match pty_pair.master.take_writer() {
        Ok(writer) => writer,
        Err(err) => {
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
        let _ = session
            .text(format!(
                "Task finished, exit code: {:?}",
                status.exit_code()
            ))
            .await;
    } else {
        let _ = session.text("Error".to_string()).await;
    }

    let mut slot = stdin_tx.lock().await;
    *slot = None;
}

#[actix_web::get("/")]
async fn index() -> impl Responder {
    let content = format!(
        "<main><h1>Projects</h1><p>{}</p><p><a href=\"/build?project={}\">Test</a></p></main>",
        BUILD_DIR, BUILD_DIR
    );

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(content)
}

#[actix_web::get("/build")]
async fn build() -> impl Responder {
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
                <h1>Task Output</h1>
                <build-component></build-component>
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
            .route("/ws", web::get().to(websocket))
    })
    .bind(("127.0.0.1", 2003))?
    .run()
    .await
}
