use actix_web::{Error, HttpRequest, web};
use actix_ws::{Message, Session, handle};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

pub const BUILD_DIR: &str = "/home/johan/Downloads/resting-chair";

#[derive(Deserialize)]
pub struct ProjectGetParams {
    pub project: String,
}

#[derive(Default)]
struct BuildInputState {
    tx: Option<mpsc::Sender<String>>,
    pending: VecDeque<String>,
}

pub async fn websocket(
    req: HttpRequest,
    body: web::Payload,
) -> Result<actix_web::HttpResponse, Error> {
    let (response, session, mut msg_stream) = handle(&req, body)?;
    let stdin_state = Arc::new(Mutex::new(BuildInputState::default()));

    actix_web::rt::spawn(async move {
        let mut session = session;
        while let Some(Ok(msg)) = msg_stream.recv().await {
            handle_message(msg, &mut session, stdin_state.clone()).await;
        }
    });

    Ok(response)
}

#[derive(Debug, Deserialize)]
struct Action {
    action: String,
    command: Option<String>,
    folder: Option<String>,
    stdin: Option<String>,
}

#[derive(Serialize)]
struct ActionResponse {
    action: String,
    status: Option<String>,
    stdout: Option<String>,
}

fn parse_action(line: String) -> Option<Action> {
    let action = serde_json::from_str::<Action>(&line);

    if action.is_err() {
        println!("Failed to parse action: {:?}", line);
        return None;
    }

    action.ok()
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
    stdin_state: Arc<Mutex<BuildInputState>>,
) {
    if stdin_state.lock().await.tx.is_some() {
        send_stdout(session, "Console is already attached.").await;
        return;
    }

    let action = parse_action(text);

    if action.is_none() {
        println!("Malformed action");
        return;
    }

    let action = action.unwrap();

    let folder = match action.folder.as_deref() {
        Some(folder) => folder,
        None => {
            send_stdout(session, "Action is missing a project folder.").await;
            send_status(session, "FAILED").await;
            return;
        }
    };

    if !folder_exists(folder) {
        println!("Project folder does not exist or is not writable.");
        send_stdout(session, "Can't find writable project folder.").await;
        send_status(session, "FAILED").await;
        return;
    }

    let mut build_session = session.clone();
    let build_stdin_state = stdin_state.clone();

    actix_web::rt::spawn(async move {
        start_build(&mut build_session, build_stdin_state, action).await;
    });
}

async fn send_stdout(session: &mut Session, line: &str) {
    println!("{}", line);

    let action = ActionResponse {
        stdout: Some(line.to_string()),
        action: "STDOUT".to_string(),
        status: None,
    };

    if let Ok(json) = serde_json::to_string(&action) {
        let _ = session.text(json).await;
    }
}

async fn send_status(session: &mut Session, line: &str) {
    println!("Task status: {}", line);

    let action = ActionResponse {
        stdout: None,
        action: "STATUS".to_string(),
        status: Some(format!("status: {line}")),
    };

    if let Ok(json) = serde_json::to_string(&action) {
        let _ = session.text(json).await;
    }
}

async fn handle_message(
    msg: Message,
    session: &mut Session,
    stdin_state: Arc<Mutex<BuildInputState>>,
) {
    match msg {
        Message::Text(text) => {
            let text = text.to_string();
            process_ws_message(text, session, stdin_state).await
        }
        Message::Close(_) => {
            println!("WebSocket closed");
        }
        _ => {
            println!("Unexpected WebSocket message");
        }
    }
}

async fn process_ws_message(
    text: String,
    session: &mut Session,
    stdin_state: Arc<Mutex<BuildInputState>>,
) {
    #[cfg(debug_assertions)]
    println!("WS: {}", text); // don't print in release mode to protect pin, password etc.

    #[cfg(not(debug_assertions))]
    println!("WS: got websocket message.");

    let action = serde_json::from_str(text.as_str());

    if action.is_err() {
        println!("Failed to parse action. {:?}", text);
        return;
    }

    let action: Action = action.unwrap();

    if action.stdin.is_some() {
        process_std_in(action, stdin_state).await;
    } else {
        process_action(text, session, stdin_state).await;
    }
}

async fn process_std_in(action: Action, stdin_state: Arc<Mutex<BuildInputState>>) {
    let input_line = action.stdin.unwrap();
    let mut state = stdin_state.lock().await;

    if let Some(tx) = state.tx.clone() {
        if tx.try_send(input_line.clone()).is_err() {
            println!("STDIN send failed, buffering input.");
            state.pending.push_back(input_line);
        }
    } else {
        state.pending.push_back(input_line);
    }
}

fn command_to_executable(action: Action) -> Option<CommandBuilder> {
    let folder = action.folder?;

    match action.action.as_str() {
        "build" => {
            let program = match action.command {
                Some(program) => program,
                None => {
                    println!("Missing command for build action");
                    return None;
                }
            };

            let mut cmd = CommandBuilder::new(program);
            cmd.cwd(folder);
            Some(cmd)
        }
        "fetch" => {
            let mut cmd = CommandBuilder::new("git");
            cmd.arg("fetch");
            cmd.arg("--all");
            cmd.cwd(folder);
            Some(cmd)
        }
        _ => {
            println!("Unknown action {}", action.action);
            None
        }
    }
}

async fn start_build(
    session: &mut Session,
    stdin_state: Arc<Mutex<BuildInputState>>,
    action: Action,
) {
    send_status(session, "RUNNING").await;
    let pty_system = native_pty_system();

    let pty_pair = match pty_system.openpty(PtySize {
        rows: 30,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(err) => {
            send_status(session, "FAILED").await;
            send_stdout(session, format!("failed to allocate PTY: {err}").as_str()).await;
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
            send_status(session, "FAILED").await;
            send_stdout(
                session,
                format!("failed to start build command in: {err}").as_str(),
            )
            .await;
            return;
        }
    };

    drop(pty_pair.slave);

    let reader = match pty_pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(err) => {
            send_status(session, "FAILED").await;
            send_stdout(
                session,
                format!("failed to open PTY reader: {err}").as_str(),
            )
            .await;
            return;
        }
    };

    let mut writer = match pty_pair.master.take_writer() {
        Ok(writer) => writer,
        Err(err) => {
            send_status(session, "FAILED").await;
            send_stdout(
                session,
                format!("failed to open PTY writer: {err}").as_str(),
            )
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

                    let action = ActionResponse {
                        stdout: Some(chunk.to_string()),
                        action: "OUTPUT".to_string(),
                        status: None,
                    };

                    let json = serde_json::to_string(&action);

                    if json.is_err() {
                        println!("Failed to build response package.");
                        break;
                    }

                    if tx.blocking_send(json.unwrap()).is_err() {
                        break;
                    }
                }
                Err(err) if err.raw_os_error() == Some(5) => {
                    // EIO: normal PTY termination on Linux.
                    break;
                }
                Err(err) => {
                    let _ = tx.blocking_send(format!("PTY read error: {err}"));
                    break;
                }
            }
        }
    });

    let (input_tx, mut input_rx) = mpsc::channel::<String>(100);

    {
        let mut state = stdin_state.lock().await;
        state.tx = Some(input_tx.clone());

        while let Some(pending) = state.pending.pop_front() {
            if input_tx.try_send(pending).is_err() {
                send_stdout(session, "Failed to flush buffered stdin input.").await;
                break;
            }
        }
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
        let out = format!("Task finished, exit code: {:?}", exit_code);

        send_stdout(session, out.as_str()).await;

        if exit_code == 0 {
            send_status(session, "SUCCESS").await;
        } else {
            send_status(session, "FAILED").await;
        }
    } else {
        send_status(session, "FAILED").await;
        send_stdout(session, "Error").await;
    }

    let mut state = stdin_state.lock().await;
    state.tx = None;
    state.pending.clear();
}
