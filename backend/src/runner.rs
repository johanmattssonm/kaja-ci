use actix_web::{Error, HttpRequest, web};
use actix_ws::{Message, Session, handle};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use serde::Deserialize;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::{Read, Write};

use std::path::PathBuf;
use std::sync::{Arc, RwLock as StdRwLock};
use std::thread;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};

#[path = "../../frontend/src/rest.rs"]
mod rest;

use crate::{APP_DATA, Task};
use rest::{Action, ActionResponse, BuildCommands};

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

#[derive(Clone, Copy)]
enum EscapeState {
    Esc,
    Csi,
}

struct TerminalScreen {
    rows: usize,
    cols: usize,
    grid: Vec<Vec<char>>,
    cursor_row: usize,
    cursor_col: usize,
    saved_cursor: Option<(usize, usize)>,
    escape_state: Option<EscapeState>,
    csi_params: String,
    scroll_top: usize,
    scroll_bottom: usize,
}

impl TerminalScreen {
    fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            grid: vec![vec![' '; cols]; rows],
            cursor_row: 0,
            cursor_col: 0,
            saved_cursor: None,
            escape_state: None,
            csi_params: String::new(),
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
        }
    }

    fn feed(&mut self, input: &str) {
        for b in input.bytes() {
            self.process_byte(b);
        }
    }

    fn process_byte(&mut self, b: u8) {
        match self.escape_state {
            Some(EscapeState::Esc) => match b {
                b'[' => {
                    self.escape_state = Some(EscapeState::Csi);
                    self.csi_params.clear();
                }
                b'7' => {
                    self.saved_cursor = Some((self.cursor_row, self.cursor_col));
                    self.escape_state = None;
                }
                b'8' => {
                    if let Some((row, col)) = self.saved_cursor {
                        self.cursor_row = row.min(self.rows.saturating_sub(1));
                        self.cursor_col = col.min(self.cols.saturating_sub(1));
                    }
                    self.escape_state = None;
                }
                b'M' => {
                    self.reverse_index();
                    self.escape_state = None;
                }
                _ => {
                    self.escape_state = None;
                }
            },
            Some(EscapeState::Csi) => {
                if (0x40..=0x7e).contains(&b) {
                    let final_byte = b as char;
                    let params = self.csi_params.clone();
                    self.apply_csi(final_byte, &params);
                    self.csi_params.clear();
                    self.escape_state = None;
                } else {
                    self.csi_params.push(b as char);
                }
            }
            None => match b {
                0x1b => self.escape_state = Some(EscapeState::Esc),
                b'\r' => self.cursor_col = 0,
                b'\n' => self.new_line(),
                0x08 => {
                    self.cursor_col = self.cursor_col.saturating_sub(1);
                }
                b'\t' => {
                    let spaces = 8 - (self.cursor_col % 8);
                    for _ in 0..spaces {
                        self.put_char(' ');
                    }
                }
                0x20..=0x7e => self.put_char(b as char),
                _ => {}
            },
        }
    }

    fn apply_csi(&mut self, final_byte: char, params: &str) {
        let params = params.trim_start_matches('?');

        match final_byte {
            'A' => {
                let n = parse_csi_n(params, 1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            'B' => {
                let n = parse_csi_n(params, 1);
                self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
            }
            'C' => {
                let n = parse_csi_n(params, 1);
                self.cursor_col = (self.cursor_col + n).min(self.cols.saturating_sub(1));
            }
            'D' => {
                let n = parse_csi_n(params, 1);
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            'E' => {
                let n = parse_csi_n(params, 1);
                self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
                self.cursor_col = 0;
            }
            'F' => {
                let n = parse_csi_n(params, 1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
                self.cursor_col = 0;
            }
            'G' => {
                let col = parse_csi_n(params, 1).saturating_sub(1);
                self.cursor_col = col.min(self.cols.saturating_sub(1));
            }
            'H' | 'f' => {
                let (row, col) = parse_cup(params);
                self.cursor_row = row.min(self.rows.saturating_sub(1));
                self.cursor_col = col.min(self.cols.saturating_sub(1));
            }
            'd' => {
                let row = parse_csi_n(params, 1).saturating_sub(1);
                self.cursor_row = row.min(self.rows.saturating_sub(1));
            }
            'J' => {
                let mode = parse_csi_n(params, 0);
                self.erase_display(mode);
            }
            'K' => {
                let mode = parse_csi_n(params, 0);
                self.erase_line(mode);
            }
            '@' => {
                let n = parse_csi_n(params, 1);
                self.insert_chars(n);
            }
            'P' => {
                let n = parse_csi_n(params, 1);
                self.delete_chars(n);
            }
            'L' => {
                let n = parse_csi_n(params, 1);
                self.insert_lines(n);
            }
            'M' => {
                let n = parse_csi_n(params, 1);
                self.delete_lines(n);
            }
            'X' => {
                let n = parse_csi_n(params, 1);
                self.erase_chars(n);
            }
            'S' => {
                let n = parse_csi_n(params, 1);
                self.scroll_up_region(n);
            }
            'T' => {
                let n = parse_csi_n(params, 1);
                self.scroll_down_region(n);
            }
            'r' => {
                self.set_scroll_region(params);
            }
            's' => {
                self.saved_cursor = Some((self.cursor_row, self.cursor_col));
            }
            'u' => {
                if let Some((row, col)) = self.saved_cursor {
                    self.cursor_row = row.min(self.rows.saturating_sub(1));
                    self.cursor_col = col.min(self.cols.saturating_sub(1));
                }
            }
            'm' | 'h' | 'l' => {}
            _ => {}
        }
    }

    fn erase_display(&mut self, mode: usize) {
        match mode {
            0 => {
                self.erase_line(0);
                for row in (self.cursor_row + 1)..self.rows {
                    for col in 0..self.cols {
                        self.grid[row][col] = ' ';
                    }
                }
            }
            1 => {
                for row in 0..self.cursor_row {
                    for col in 0..self.cols {
                        self.grid[row][col] = ' ';
                    }
                }
                for col in 0..=self.cursor_col.min(self.cols.saturating_sub(1)) {
                    self.grid[self.cursor_row][col] = ' ';
                }
            }
            2 => {
                for row in 0..self.rows {
                    for col in 0..self.cols {
                        self.grid[row][col] = ' ';
                    }
                }
                self.cursor_row = 0;
                self.cursor_col = 0;
                self.scroll_top = 0;
                self.scroll_bottom = self.rows.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, mode: usize) {
        match mode {
            0 => {
                for col in self.cursor_col..self.cols {
                    self.grid[self.cursor_row][col] = ' ';
                }
            }
            1 => {
                for col in 0..=self.cursor_col.min(self.cols.saturating_sub(1)) {
                    self.grid[self.cursor_row][col] = ' ';
                }
            }
            2 => {
                for col in 0..self.cols {
                    self.grid[self.cursor_row][col] = ' ';
                }
            }
            _ => {}
        }
    }

    fn insert_chars(&mut self, n: usize) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }

        let row = self.cursor_row.min(self.rows - 1);
        let col = self.cursor_col.min(self.cols - 1);
        let shift = n.min(self.cols.saturating_sub(col));

        if shift == 0 {
            return;
        }

        for i in (col..self.cols).rev() {
            let src = i.saturating_sub(shift);
            self.grid[row][i] = if src >= col { self.grid[row][src] } else { ' ' };
        }
    }

    fn delete_chars(&mut self, n: usize) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }

        let row = self.cursor_row.min(self.rows - 1);
        let col = self.cursor_col.min(self.cols - 1);
        let shift = n.min(self.cols.saturating_sub(col));

        if shift == 0 {
            return;
        }

        for i in col..self.cols {
            let src = i + shift;
            self.grid[row][i] = if src < self.cols {
                self.grid[row][src]
            } else {
                ' '
            };
        }
    }

    fn erase_chars(&mut self, n: usize) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }

        let row = self.cursor_row.min(self.rows - 1);
        let col = self.cursor_col.min(self.cols - 1);
        let end = (col + n).min(self.cols);

        for i in col..end {
            self.grid[row][i] = ' ';
        }
    }

    fn insert_lines(&mut self, n: usize) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }

        let row = self.cursor_row.min(self.rows - 1);
        if row < self.scroll_top || row > self.scroll_bottom {
            return;
        }

        let count = n.min(self.scroll_bottom.saturating_sub(row) + 1);

        for _ in 0..count {
            self.grid.insert(row, vec![' '; self.cols]);
            self.grid.remove(self.scroll_bottom + 1);
        }
    }

    fn delete_lines(&mut self, n: usize) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }

        let row = self.cursor_row.min(self.rows - 1);
        if row < self.scroll_top || row > self.scroll_bottom {
            return;
        }

        let count = n.min(self.scroll_bottom.saturating_sub(row) + 1);

        for _ in 0..count {
            self.grid.remove(row);
            self.grid.insert(self.scroll_bottom, vec![' '; self.cols]);
        }
    }

    fn put_char(&mut self, ch: char) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }

        if self.cursor_row >= self.rows {
            self.cursor_row = self.rows - 1;
        }

        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.new_line();
        }

        self.grid[self.cursor_row][self.cursor_col] = ch;
        self.cursor_col += 1;

        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.new_line();
        }
    }

    fn new_line(&mut self) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }

        if self.cursor_row == self.scroll_bottom {
            self.scroll_up_region(1);
            return;
        }

        if self.cursor_row + 1 >= self.rows {
            self.cursor_row = self.rows.saturating_sub(1);
            return;
        }

        self.cursor_row += 1;
    }

    fn reverse_index(&mut self) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }

        if self.cursor_row == self.scroll_top {
            self.scroll_down_region(1);
        } else {
            self.cursor_row = self.cursor_row.saturating_sub(1);
        }
    }

    fn scroll_up_region(&mut self, n: usize) {
        if self.rows == 0 || self.cols == 0 || self.scroll_top > self.scroll_bottom {
            return;
        }

        let count = n.min(self.scroll_bottom.saturating_sub(self.scroll_top) + 1);

        for _ in 0..count {
            self.grid.remove(self.scroll_top);
            self.grid.insert(self.scroll_bottom, vec![' '; self.cols]);
        }
    }

    fn scroll_down_region(&mut self, n: usize) {
        if self.rows == 0 || self.cols == 0 || self.scroll_top > self.scroll_bottom {
            return;
        }

        let count = n.min(self.scroll_bottom.saturating_sub(self.scroll_top) + 1);

        for _ in 0..count {
            self.grid.remove(self.scroll_bottom);
            self.grid.insert(self.scroll_top, vec![' '; self.cols]);
        }
    }

    fn set_scroll_region(&mut self, params: &str) {
        if self.rows == 0 {
            return;
        }

        if params.is_empty() {
            self.scroll_top = 0;
            self.scroll_bottom = self.rows.saturating_sub(1);
            self.cursor_row = 0;
            self.cursor_col = 0;
            return;
        }

        let mut parts = params.split(';');
        let top = parts
            .next()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(1)
            - 1;
        let bottom = parts
            .next()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(self.rows)
            - 1;

        if top < bottom && bottom < self.rows {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        } else {
            self.scroll_top = 0;
            self.scroll_bottom = self.rows.saturating_sub(1);
        }

        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    fn to_text(&self) -> String {
        let mut out = String::with_capacity(self.rows * (self.cols + 1));

        for (idx, row) in self.grid.iter().enumerate() {
            for ch in row {
                let _ = out.write_char(*ch);
            }

            if idx + 1 < self.rows {
                out.push('\n');
            }
        }

        out
    }
}

fn parse_csi_n(params: &str, default: usize) -> usize {
    if params.is_empty() {
        return default;
    }

    params
        .split(';')
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

fn parse_cup(params: &str) -> (usize, usize) {
    if params.is_empty() {
        return (0, 0);
    }

    let mut parts = params.split(';');
    let row = parts
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .and_then(|n| n.checked_sub(1))
        .unwrap_or(0);
    let col = parts
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .and_then(|n| n.checked_sub(1))
        .unwrap_or(0);

    (row, col)
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
        cursor_row: None,
        cursor_col: None,
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
        cursor_row: None,
        cursor_col: None,
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

fn command_to_executable(action: &Action) -> Option<CommandBuilder> {
    let folder = action.folder.as_deref()?;

    let mut project_dir = PathBuf::from(folder);
    project_dir.push("ci.json");

    let config_string = match std::fs::read_to_string(&project_dir) {
        Ok(contents) => contents,
        Err(err) => {
            println!("Failed to read {}: {err}", project_dir.display());
            return None;
        }
    };

    let config_commands: BuildCommands = match serde_json::from_str(&config_string) {
        Ok(config) => config,
        Err(err) => {
            println!("Failed to parse {}: {err}", project_dir.display());
            return None;
        }
    };

    match action.action.as_str() {
        "build" => parse_command(config_commands.build, folder).ok(),
        "fetch" => parse_command(config_commands.git, folder).ok(),
        "test" => parse_command(config_commands.test, folder).ok(),
        "release" => parse_command(config_commands.release, folder).ok(),
        "clean" => parse_command(config_commands.clean, folder).ok(),
        "terminal" => parse_command(config_commands.terminal, folder).ok(),
        _ => {
            println!("Unknown action {}", action.action);
            None
        }
    }
}

fn parse_command(command: Option<String>, folder: &str) -> Result<CommandBuilder, String> {
    let command = command.ok_or_else(|| "No command".to_string())?;

    let parts = shell_words::split(&command).map_err(|e| e.to_string())?;
    let (program, args) = parts
        .split_first()
        .ok_or_else(|| "Empty command".to_string())?;
    let mut cmd = CommandBuilder::new(program);

    for arg in args {
        cmd.arg(arg);
    }

    cmd.cwd(folder);
    Ok(cmd)
}

async fn regiter_process(action: &Action, process: Arc<StdRwLock<Box<dyn Child + Send + Sync>>>) {
    if action.folder.is_none() {
        println!("No folder for process");
        return;
    }

    let folder = action.folder.clone().unwrap();

    APP_DATA.write().await.tasks.insert(
        folder,
        Task {
            running_command: Some(process),
            output_lines: Vec::new(),
        },
    );
}

async fn unregister_process(action: &Action) {
    if action.folder.is_none() {
        println!("No folder for process");
        return;
    }

    let folder = action.folder.clone().unwrap();

    APP_DATA.write().await.tasks.insert(
        folder,
        Task {
            running_command: None,
            output_lines: Vec::new(),
        },
    );
}

fn wait_for_exit(
    process: Arc<StdRwLock<Box<dyn Child + Send + Sync>>>,
) -> std::io::Result<portable_pty::ExitStatus> {
    loop {
        let status = {
            let mut child = process
                .write()
                .map_err(|_| std::io::Error::other("process lock poisoned"))?;
            child.try_wait()
        };

        match status {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(err) => return Err(err),
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

    let rows = 20u16;
    let cols = 80u16;

    let pty_pair = match pty_system.openpty(PtySize {
        rows,
        cols,
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

    let cmd = command_to_executable(&action);

    if cmd.is_none() {
        println!("No command to execute.");
        return;
    }

    let cmd = cmd.unwrap();

    // println!("Executing: {:?}", cmd);

    let child = match pty_pair.slave.spawn_command(cmd) {
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

    let child = Arc::new(StdRwLock::new(child));
    regiter_process(&action, child.clone()).await;
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
        let mut screen = TerminalScreen::new(rows as usize, cols as usize);

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                    screen.feed(&chunk);

                    let action = ActionResponse {
                        stdout: Some(screen.to_text()),
                        action: "SCREEN".to_string(),
                        status: None,
                        cursor_row: Some(screen.cursor_row),
                        cursor_col: Some(screen.cursor_col),
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

    let status = wait_for_exit(child);
    println!("Task ended: {:?}", status);

    unregister_process(&action).await;

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
