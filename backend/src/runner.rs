use actix_web::{Error, HttpRequest, web};
use actix_ws::{Message, Session, handle};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use serde::Deserialize;
use std::collections::VecDeque;
use std::fmt::{Display, Formatter, Write as _};
use std::io::{Read, Write};

use std::path::PathBuf;
use std::sync::{
    Arc, RwLock as StdRwLock,
    atomic::{AtomicU64, Ordering},
};
use std::thread;
use std::time::Duration;
use tokio::sync::{Mutex, broadcast, mpsc};

#[path = "../../frontend/src/rest.rs"]
mod rest;

use crate::app::APP_DATA;
use rest::{Action, ActionResponse, BuildCommands};

pub const BUILD_DIR: &str = "/home/johan/Downloads/resting-chair";

#[derive(Deserialize)]
pub struct ProjectGetParams {
    pub project: String,
}

#[derive(Deserialize)]
pub struct RunGetParams {
    pub project: String,
    pub task_key: String,
}

#[derive(Debug)]
pub enum RunTaskError {
    AlreadyRunning,
    InvalidProjectFolder,
    NoCommandConfigured(String),
    Internal(String),
}

impl Display for RunTaskError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => write!(f, "There is already a task running."),
            Self::InvalidProjectFolder => write!(f, "Can't find writable project folder."),
            Self::NoCommandConfigured(action_name) => {
                write!(f, "No command configured for action '{}'.", action_name)
            }
            Self::Internal(message) => write!(f, "{message}"),
        }
    }
}

#[derive(Default)]
struct BuildInputState {
    task_key: Option<String>,
    pending: VecDeque<String>,
}

struct OutputPacket {
    websocket_payload: String,
    history_chunk: String,
}

#[derive(Clone, Copy)]
enum EscapeState {
    Esc,
    Csi,
    Osc,
    EscIntermediate,
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
    osc_esc_pending: bool,
    scroll_top: usize,
    scroll_bottom: usize,
    alternate_screen: bool,
    insert_mode: bool,
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
            osc_esc_pending: false,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            alternate_screen: false,
            insert_mode: false,
        }
    }

    fn feed(&mut self, input: &str) {
        for ch in input.chars() {
            self.process_char(ch);
        }
    }

    fn process_char(&mut self, ch: char) {
        if ch.is_ascii() {
            self.process_byte(ch as u8);
            return;
        }

        match self.escape_state {
            None => self.put_char(ch),
            Some(EscapeState::Osc) => {
                if self.osc_esc_pending {
                    self.osc_esc_pending = false;
                }
            }
            Some(EscapeState::Esc) | Some(EscapeState::EscIntermediate) => {
                self.escape_state = None;
            }
            Some(EscapeState::Csi) => {}
        }
    }

    fn process_byte(&mut self, b: u8) {
        match self.escape_state {
            Some(EscapeState::Esc) => match b {
                b'[' => {
                    // ESC [ : CSI (Control Sequence Introducer)
                    self.escape_state = Some(EscapeState::Csi);
                    self.csi_params.clear();
                }
                b']' => {
                    // ESC ] : OSC (Operating System Command)
                    self.escape_state = Some(EscapeState::Osc);
                    self.osc_esc_pending = false;
                }
                b'7' => {
                    // ESC 7 : DECSC (save cursor)
                    self.saved_cursor = Some((self.cursor_row, self.cursor_col));
                    self.escape_state = None;
                }
                b'8' => {
                    // ESC 8 : DECRC (restore cursor)
                    if let Some((row, col)) = self.saved_cursor {
                        self.cursor_row = row.min(self.rows.saturating_sub(1));
                        self.cursor_col = col.min(self.cols.saturating_sub(1));
                    }
                    self.escape_state = None;
                }
                b'M' => {
                    // ESC M : RI (reverse index)
                    self.reverse_index();
                    self.escape_state = None;
                }
                b if (0x20..=0x2f).contains(&b) => {
                    // ESC intermediates (0x20..0x2F)
                    self.escape_state = Some(EscapeState::EscIntermediate);
                }
                b if (0x30..=0x7e).contains(&b) => {
                    // ESC final byte (0x30..0x7E)
                    self.escape_state = None;
                }
                _ => {
                    self.escape_state = None;
                }
            },
            Some(EscapeState::Csi) => {
                if (0x40..=0x7e).contains(&b) {
                    // CSI final byte (0x40..0x7E)
                    let final_byte = b as char;
                    let params = self.csi_params.clone();
                    self.apply_csi(final_byte, &params);
                    self.csi_params.clear();
                    self.escape_state = None;
                } else {
                    // CSI parameter/intermediate bytes
                    self.csi_params.push(b as char);
                }
            }
            Some(EscapeState::Osc) => {
                if self.osc_esc_pending {
                    if b == b'\\' {
                        self.escape_state = None;
                    }
                    self.osc_esc_pending = false;
                    return;
                }

                match b {
                    0x07 => {
                        // BEL (bell) terminator
                        self.escape_state = None;
                    }
                    0x1b => {
                        // ESC (escape), possible ST terminator: ESC \
                        self.osc_esc_pending = true;
                    }
                    _ => {}
                }
            }
            Some(EscapeState::EscIntermediate) => {
                if (0x30..=0x7e).contains(&b) {
                    self.escape_state = None;
                }
            }
            None => match b {
                0x1b => self.escape_state = Some(EscapeState::Esc), // ESC (escape)
                b'\r' => self.cursor_col = 0,                       // CR (carriage return)
                b'\n' => self.new_line(),                           // LF (line feed / newline)
                0x08 => {
                    // BS (backspace)
                    self.cursor_col = self.cursor_col.saturating_sub(1);

                    println!("BACKSPACE");
                }
                b'\t' => {
                    // HT (horizontal tab)
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
        let private_mode = params.starts_with('?');
        let params = params.trim_start_matches('?');

        match final_byte {
            'A' => {
                // CUU: Cursor Up (arrow up)
                let n = parse_csi_n(params, 1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            'B' => {
                // CUD: Cursor Down (arrow down)
                let n = parse_csi_n(params, 1);
                self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
            }
            'C' => {
                // CUF: Cursor Forward (arrow right)
                let n = parse_csi_n(params, 1);
                self.cursor_col = (self.cursor_col + n).min(self.cols.saturating_sub(1));
            }
            'D' => {
                // CUB: Cursor Backward (arrow left)
                let n = parse_csi_n(params, 1);
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            'E' => {
                // CNL: Cursor Next Line
                let n = parse_csi_n(params, 1);
                self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
                self.cursor_col = 0;
            }
            'F' => {
                // CPL: Cursor Previous Line
                let n = parse_csi_n(params, 1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
                self.cursor_col = 0;
            }
            'G' => {
                // CHA: Cursor Horizontal Absolute (set column)
                let col = parse_csi_n(params, 1).saturating_sub(1);
                self.cursor_col = col.min(self.cols.saturating_sub(1));
            }
            'H' | 'f' => {
                // CUP / HVP: Cursor Position / Horizontal and Vertical Position
                let (row, col) = parse_cup(params);
                self.cursor_row = row.min(self.rows.saturating_sub(1));
                self.cursor_col = col.min(self.cols.saturating_sub(1));
            }
            'd' => {
                // VPA: Vertical Position Absolute (set row)
                let row = parse_csi_n(params, 1).saturating_sub(1);
                self.cursor_row = row.min(self.rows.saturating_sub(1));
            }
            'J' => {
                // ED: Erase in Display
                let mode = parse_csi_n(params, 0);
                self.erase_display(mode);
            }
            'K' => {
                // EL: Erase in Line
                let mode = parse_csi_n(params, 0);
                self.erase_line(mode);
            }
            '@' => {
                // ICH: Insert Character(s)
                let n = parse_csi_n(params, 1);
                self.insert_chars(n);
            }
            'P' => {
                // DCH: Delete Character(s)
                let n = parse_csi_n(params, 1);
                self.delete_chars(n);
            }
            'L' => {
                // IL: Insert Line(s)
                let n = parse_csi_n(params, 1);
                self.insert_lines(n);
            }
            'M' => {
                // DL: Delete Line(s)
                let n = parse_csi_n(params, 1);
                self.delete_lines(n);
            }
            'X' => {
                // ECH: Erase Character(s)
                let n = parse_csi_n(params, 1);
                self.erase_chars(n);
            }
            'S' => {
                // SU: Scroll Up
                let n = parse_csi_n(params, 1);
                self.scroll_up_region(n);
            }
            'T' => {
                // SD: Scroll Down
                let n = parse_csi_n(params, 1);
                self.scroll_down_region(n);
            }
            'r' => {
                // DECSTBM: Set top/bottom scrolling region
                self.set_scroll_region(params);
            }
            's' => {
                // SCP: Save cursor position
                self.saved_cursor = Some((self.cursor_row, self.cursor_col));
            }
            'u' => {
                // RCP: Restore cursor position
                if let Some((row, col)) = self.saved_cursor {
                    self.cursor_row = row.min(self.rows.saturating_sub(1));
                    self.cursor_col = col.min(self.cols.saturating_sub(1));
                }
            }
            'm' => {} // SGR: Select Graphic Rendition
            'h' | 'l' => {
                let enable = final_byte == 'h';

                if private_mode {
                    for raw in params.split(';') {
                        let code = raw.parse::<usize>().ok();
                        match code {
                            Some(47) | Some(1047) | Some(1049) => {
                                self.alternate_screen = enable;
                                if enable {
                                    self.erase_display(2);
                                }
                            }
                            _ => {}
                        }
                    }
                } else {
                    for raw in params.split(';') {
                        if raw == "4" {
                            self.insert_mode = enable;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn in_alternate_screen(&self) -> bool {
        self.alternate_screen
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

        if self.insert_mode {
            self.insert_chars(1);
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

fn decode_utf8_chunk(pending: &mut Vec<u8>) -> String {
    let mut out = String::new();

    loop {
        if pending.is_empty() {
            break;
        }

        match std::str::from_utf8(pending) {
            Ok(valid) => {
                out.push_str(valid);
                pending.clear();
                break;
            }
            Err(err) => {
                let valid_up_to = err.valid_up_to();

                if valid_up_to > 0 {
                    let valid = std::str::from_utf8(&pending[..valid_up_to])
                        .expect("UTF-8 prefix should be valid");
                    out.push_str(valid);
                }

                match err.error_len() {
                    Some(error_len) => {
                        out.push('\u{fffd}');
                        pending.drain(..valid_up_to + error_len);
                    }
                    None => {
                        pending.drain(..valid_up_to);
                        break;
                    }
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_charset_designation_is_consumed() {
        let mut screen = TerminalScreen::new(1, 32);
        screen.feed("\u{1b}(B^X Exit");
        let text = screen.to_text();

        assert!(!text.contains("B^X Exit"));
        assert!(text.contains("^X Exit"));
    }

    #[test]
    fn osc_payload_is_consumed() {
        let mut screen = TerminalScreen::new(1, 64);
        screen.feed("A\u{1b}]666;vte.shell.precmd!\u{7}B");
        let text = screen.to_text();

        assert!(text.starts_with("AB"));
        assert!(!text.contains("666;vte.shell"));
    }

    #[test]
    fn insert_mode_inserts_text_at_cursor() {
        let mut screen = TerminalScreen::new(1, 16);
        screen.feed("abcd\u{1b}[2D\u{1b}[4hX\u{1b}[4l");
        let text = screen.to_text();
        assert_eq!(text.trim_end(), "abXcd");
    }

    #[test]
    fn default_mode_overwrites_text_at_cursor() {
        let mut screen = TerminalScreen::new(1, 16);
        screen.feed("abcd\u{1b}[2DX");
        let text = screen.to_text();
        assert_eq!(text.trim_end(), "abXd");
    }

    #[test]
    fn unicode_text_is_rendered() {
        let mut screen = TerminalScreen::new(1, 16);
        screen.feed("åäö");
        assert_eq!(screen.to_text().trim_end(), "åäö");
    }

    #[test]
    fn terminal_payload_uses_screen_action_in_alternate_screen() {
        let mut screen = TerminalScreen::new(2, 8);
        screen.feed("\u{1b}[?1049hhi");

        let payload = build_terminal_action_payload(&screen, "ignored").unwrap();
        let response: ActionResponse = serde_json::from_str(payload.as_str()).unwrap();

        assert_eq!(response.action, "SCREEN");
        assert_eq!(response.stdout.unwrap(), screen.to_text());
        assert_eq!(response.cursor_row, Some(screen.cursor_row));
        assert_eq!(response.cursor_col, Some(screen.cursor_col));
    }

    #[test]
    fn terminal_payload_uses_stdout_action_outside_alternate_screen() {
        let mut screen = TerminalScreen::new(2, 8);
        screen.feed("hello");

        let payload = build_terminal_action_payload(&screen, "hello").unwrap();
        let response: ActionResponse = serde_json::from_str(payload.as_str()).unwrap();

        assert_eq!(response.action, "STDOUT");
        assert_eq!(response.stdout.unwrap(), "hello");
        assert_eq!(response.cursor_row, None);
        assert_eq!(response.cursor_col, Some(screen.cursor_col));
    }

    #[test]
    fn utf8_decoder_handles_split_multibyte_sequences() {
        let mut pending = vec![0xC3];
        assert_eq!(decode_utf8_chunk(&mut pending), "");
        assert_eq!(pending, vec![0xC3]);

        pending.push(0xA5);
        assert_eq!(decode_utf8_chunk(&mut pending), "å");
        assert!(pending.is_empty());
    }

    #[test]
    fn terminal_detached_task_keys_are_unique() {
        let first = build_detached_task_key("terminal", "/tmp/project");
        let second = build_detached_task_key("terminal", "/tmp/project");

        assert_ne!(first, second);
        assert!(is_terminal_task_key(first.as_str()));
        assert!(is_terminal_task_key(second.as_str()));
    }

    #[test]
    fn non_terminal_detached_task_key_uses_default_shape() {
        assert_eq!(
            build_detached_task_key("build", "/tmp/project"),
            "build-/tmp/project"
        );
    }
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

static TERMINAL_TASK_COUNTER: AtomicU64 = AtomicU64::new(1);

fn build_task_key(action: &str, folder: &str) -> String {
    format!("{}-{}", action, folder)
}

fn is_terminal_action(action: &str) -> bool {
    action == "terminal"
}

fn is_terminal_task_key(task_key: &str) -> bool {
    task_key.starts_with("terminal-")
}

fn build_detached_task_key(action_name: &str, folder: &str) -> String {
    if is_terminal_action(action_name) {
        let id = TERMINAL_TASK_COUNTER.fetch_add(1, Ordering::Relaxed);
        return format!("terminal-{}-{id}", folder);
    }

    build_task_key(action_name, folder)
}

fn normalize_run_task_key(task_key: &str) -> Option<&'static str> {
    match task_key {
        "git" => Some("git"),
        "build" => Some("build"),
        "test" => Some("test"),
        "release" => Some("release"),
        "clean" => Some("clean"),
        "terminal" => Some("terminal"),
        _ => None,
    }
}

fn task_key_for_action(action: &Action) -> Option<String> {
    Some(build_task_key(&action.action, action.folder.as_deref()?))
}

async fn task_stdin_sender(task_key: &str) -> Option<mpsc::Sender<String>> {
    APP_DATA
        .read()
        .await
        .tasks
        .get(task_key)
        .and_then(|task| task.stdin_tx.clone())
}

async fn append_task_history(task_key: &str, output: &str) {
    if output.is_empty() || is_terminal_task_key(task_key) {
        return;
    }

    let mut app = APP_DATA.write().await;

    if let Some(task) = app.tasks.get_mut(task_key) {
        task.history.extend_from_slice(output.as_bytes());
    }
}

async fn flush_pending_stdin(task_key: &str, stdin_state: Arc<Mutex<BuildInputState>>) {
    loop {
        let pending = {
            let mut state = stdin_state.lock().await;

            if state.task_key.as_deref() != Some(task_key) {
                return;
            }

            state.pending.pop_front()
        };

        let Some(pending) = pending else {
            break;
        };

        let Some(tx) = task_stdin_sender(task_key).await else {
            let mut state = stdin_state.lock().await;
            state.pending.push_front(pending);
            break;
        };

        if tx.try_send(pending.clone()).is_err() {
            let mut state = stdin_state.lock().await;
            state.pending.push_front(pending);
            break;
        }
    }
}

async fn attach_to_running_task(
    session: &mut Session,
    stdin_state: Arc<Mutex<BuildInputState>>,
    task_key: &str,
) -> bool {
    let (history, mut stdout_rx) = {
        let app = APP_DATA.read().await;

        let Some(task) = app.tasks.get(task_key) else {
            return false;
        };

        if task.running_command.is_none() {
            return false;
        }

        let Some(stdout_tx) = task.stdout_tx.clone() else {
            return false;
        };

        (task.history.clone(), stdout_tx.subscribe())
    };

    if !history.is_empty() && !is_terminal_task_key(task_key) {
        let history_action = ActionResponse {
            stdout: Some(String::from_utf8_lossy(&history).into_owned()),
            action: "STDOUT".to_string(),
            status: None,
            cursor_row: None,
            cursor_col: None,
        };

        if let Ok(json) = serde_json::to_string(&history_action) {
            let _ = session.text(json).await;
        }
    }

    send_status(session, "RUNNING").await;
    flush_pending_stdin(task_key, stdin_state).await;

    let mut attached_session = session.clone();

    actix_web::rt::spawn(async move {
        loop {
            match stdout_rx.recv().await {
                Ok(chunk) => {
                    if attached_session.text(chunk).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    true
}

async fn process_action(
    action: Action,
    session: &mut Session,
    stdin_state: Arc<Mutex<BuildInputState>>,
) {
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

    let Some(task_key) = task_key_for_action(&action) else {
        send_stdout(session, "Unable to build task key.").await;
        send_status(session, "FAILED").await;
        return;
    };

    {
        let mut state = stdin_state.lock().await;
        state.task_key = Some(task_key.clone());
    }

    if attach_to_running_task(session, stdin_state.clone(), task_key.as_str()).await {
        return;
    }

    let mut build_session = session.clone();
    let build_stdin_state = stdin_state.clone();

    actix_web::rt::spawn(async move {
        start_build(&mut build_session, build_stdin_state, action, task_key).await;
    });
}

async fn process_attach_action(
    action: Action,
    session: &mut Session,
    stdin_state: Arc<Mutex<BuildInputState>>,
) {
    let Some(task_key) = action.task_key else {
        send_stdout(session, "Attach action is missing task_key.").await;
        send_status(session, "FAILED").await;
        return;
    };

    {
        let mut state = stdin_state.lock().await;
        state.task_key = Some(task_key.clone());
    }

    if attach_to_running_task(session, stdin_state, task_key.as_str()).await {
        return;
    }

    let message = format!("No running task found for key '{}'.", task_key);
    send_stdout(session, message.as_str()).await;
    send_status(session, "FAILED").await;
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

async fn send_stdout_for_task(session: &mut Session, task_key: &str, line: &str) {
    append_task_history(task_key, line).await;
    send_stdout(session, line).await;
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
        return;
    }

    if action.action == "attach" {
        process_attach_action(action, session, stdin_state).await;
        return;
    }

    process_action(action, session, stdin_state).await;
}

async fn process_std_in(action: Action, stdin_state: Arc<Mutex<BuildInputState>>) {
    let Some(input_line) = action.stdin else {
        return;
    };

    let task_key = {
        let mut state = stdin_state.lock().await;

        let Some(task_key) = state.task_key.clone() else {
            state.pending.push_back(input_line);
            return;
        };

        task_key
    };

    let Some(tx) = task_stdin_sender(task_key.as_str()).await else {
        let mut state = stdin_state.lock().await;
        state.pending.push_back(input_line);
        return;
    };

    if tx.try_send(input_line.clone()).is_err() {
        println!("STDIN send failed, buffering input.");
        let mut state = stdin_state.lock().await;
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
        "git" => parse_command(config_commands.git, folder).ok(),
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

async fn register_process(
    action: &Action,
    process: Arc<StdRwLock<Box<dyn Child + Send + Sync>>>,
    stdin_tx: mpsc::Sender<String>,
    stdout_tx: broadcast::Sender<String>,
) {
    let _ = APP_DATA.write().await.start_task(
        Some(action.action.clone()),
        action.folder.clone(),
        None,
        process,
        stdin_tx,
        stdout_tx,
    );
}

async fn unregister_process(action: &Action) {
    let _ = APP_DATA
        .write()
        .await
        .drop_task(Some(action.action.clone()), action.folder.clone());
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

pub async fn start_run_task(
    project_folder: String,
    run_task_key: String,
) -> Result<String, RunTaskError> {
    let Some(action_name) = normalize_run_task_key(run_task_key.as_str()) else {
        return Err(RunTaskError::NoCommandConfigured(run_task_key));
    };

    start_detached_action(action_name, project_folder).await
}

async fn check_requested_task_state(
    action_name: &str,
    requested_task_key: &str,
) -> Result<Option<String>, RunTaskError> {
    let app = APP_DATA.read().await;

    if is_terminal_action(action_name) {
        return Ok(None);
    }

    if let Some(task) = app.tasks.get(requested_task_key) {
        if task.running_command.is_some() {
            return Ok(Some(requested_task_key.to_string()));
        }
    }

    if app.has_running_task() {
        return Err(RunTaskError::AlreadyRunning);
    }

    Ok(None)
}

async fn start_detached_action(
    action_name: &str,
    project_folder: String,
) -> Result<String, RunTaskError> {
    if !folder_exists(project_folder.as_str()) {
        return Err(RunTaskError::InvalidProjectFolder);
    }

    let requested_task_key = build_detached_task_key(action_name, project_folder.as_str());
    let existing_task_key =
        check_requested_task_state(action_name, requested_task_key.as_str()).await?;

    if let Some(existing_task_key) = existing_task_key {
        return Ok(existing_task_key);
    }

    let action_name = action_name.to_string();

    let action = Action {
        action: action_name.clone(),
        command: None,
        folder: Some(project_folder.clone()),
        stdin: None,
        task_key: None,
    };

    let Some(cmd) = command_to_executable(&action) else {
        return Err(RunTaskError::NoCommandConfigured(action_name.clone()));
    };

    let pty_system = native_pty_system();
    let rows = 30u16;
    let cols = 120u16;

    let pty_pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|err| RunTaskError::Internal(format!("failed to allocate PTY: {err}")))?;

    let mut child = pty_pair
        .slave
        .spawn_command(cmd)
        .map_err(|err| RunTaskError::Internal(format!("failed to start run command: {err}")))?;

    let reader = match pty_pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(err) => {
            let _ = child.kill();
            return Err(RunTaskError::Internal(format!(
                "failed to open PTY reader: {err}"
            )));
        }
    };

    let mut writer = match pty_pair.master.take_writer() {
        Ok(writer) => writer,
        Err(err) => {
            let _ = child.kill();
            return Err(RunTaskError::Internal(format!(
                "failed to open PTY writer: {err}"
            )));
        }
    };

    let (input_tx, mut input_rx) = mpsc::channel::<String>(100);
    let (stdout_tx, _) = broadcast::channel::<String>(200);
    let child = Arc::new(StdRwLock::new(child));

    let task_key = {
        let mut app = APP_DATA.write().await;

        match app.start_task(
            Some(action_name.clone()),
            Some(project_folder.clone()),
            Some(requested_task_key.clone()),
            child.clone(),
            input_tx,
            stdout_tx.clone(),
        ) {
            Ok(task_key) => task_key,
            Err(err) => {
                if let Ok(mut process) = child.write() {
                    let _ = process.kill();
                }

                return Err(RunTaskError::Internal(err));
            }
        }
    };

    broadcast_task_status(&stdout_tx, "RUNNING");

    drop(pty_pair.slave);

    let task_key_stdout = task_key.clone();
    let stdout_tx_reader = stdout_tx.clone();

    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0_u8; 4096];
        let mut utf8_pending = Vec::new();
        let mut state = TerminalScreen::new(rows as usize, cols as usize);

        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    if !utf8_pending.is_empty() {
                        let mut chunk = decode_utf8_chunk(&mut utf8_pending);

                        if !utf8_pending.is_empty() {
                            chunk.push_str(&String::from_utf8_lossy(&utf8_pending));
                            utf8_pending.clear();
                        }

                        if !chunk.is_empty() {
                            state.feed(chunk.as_str());
                            broadcast_terminal_update(
                                task_key_stdout.as_str(),
                                &stdout_tx_reader,
                                &state,
                                chunk.as_str(),
                            );
                        }
                    }

                    break;
                }
                Ok(n) => {
                    utf8_pending.extend_from_slice(&buf[..n]);
                    let chunk = decode_utf8_chunk(&mut utf8_pending);

                    if chunk.is_empty() {
                        continue;
                    }

                    state.feed(chunk.as_str());
                    broadcast_terminal_update(
                        task_key_stdout.as_str(),
                        &stdout_tx_reader,
                        &state,
                        chunk.as_str(),
                    );
                }
                Err(err) if err.raw_os_error() == Some(5) => {
                    break;
                }
                Err(err) => {
                    let line = format!("PTY read error: {err}");
                    broadcast_task_stdout(task_key_stdout.as_str(), &stdout_tx_reader, line);
                    break;
                }
            }
        }
    });

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

    let task_key_exit = task_key.clone();
    let stdout_tx_exit = stdout_tx.clone();
    let child_exit = child.clone();

    std::thread::spawn(move || {
        let status = wait_for_exit(child_exit);

        if let Ok(status) = status {
            let exit_code = status.exit_code();
            let out = format!("Task finished, exit code: {:?}", exit_code);
            broadcast_task_stdout(task_key_exit.as_str(), &stdout_tx_exit, out);

            if exit_code == 0 {
                broadcast_task_status(&stdout_tx_exit, "SUCCESS");
            } else {
                broadcast_task_status(&stdout_tx_exit, "FAILED");
            }
        } else {
            let out = "Task failed while waiting for exit.".to_string();
            broadcast_task_stdout(task_key_exit.as_str(), &stdout_tx_exit, out);
            broadcast_task_status(&stdout_tx_exit, "FAILED");
        }

        let mut app = APP_DATA.blocking_write();
        let _ = app.drop_task_by_key(task_key_exit.as_str());
    });

    Ok(task_key)
}

fn stdout_response_json(line: String) -> Option<String> {
    serde_json::to_string(&ActionResponse {
        stdout: Some(line),
        action: "STDOUT".to_string(),
        status: None,
        cursor_row: None,
        cursor_col: None,
    })
    .ok()
}

fn status_response_json(line: &str) -> Option<String> {
    serde_json::to_string(&ActionResponse {
        stdout: None,
        action: "STATUS".to_string(),
        status: Some(format!("status: {line}")),
        cursor_row: None,
        cursor_col: None,
    })
    .ok()
}

fn append_task_history_blocking(task_key: &str, output: &str) {
    if output.is_empty() || is_terminal_task_key(task_key) {
        return;
    }

    let mut app = APP_DATA.blocking_write();

    if let Some(task) = app.tasks.get_mut(task_key) {
        task.history.extend_from_slice(output.as_bytes());
    }
}

fn broadcast_task_stdout(task_key: &str, stdout_tx: &broadcast::Sender<String>, line: String) {
    append_task_history_blocking(task_key, line.as_str());

    if let Some(payload) = stdout_response_json(line) {
        let _ = stdout_tx.send(payload);
    }
}

fn broadcast_task_status(stdout_tx: &broadcast::Sender<String>, status: &str) {
    if let Some(payload) = status_response_json(status) {
        let _ = stdout_tx.send(payload);
    }
}

fn build_terminal_action_payload(state: &TerminalScreen, chunk: &str) -> Option<String> {
    let action = if state.in_alternate_screen() {
        ActionResponse {
            stdout: Some(state.to_text()),
            action: "SCREEN".to_string(),
            status: None,
            cursor_row: Some(state.cursor_row),
            cursor_col: Some(state.cursor_col),
        }
    } else {
        ActionResponse {
            stdout: Some(chunk.to_string()),
            action: "STDOUT".to_string(),
            status: None,
            cursor_row: None,
            cursor_col: Some(state.cursor_col),
        }
    };

    serde_json::to_string(&action).ok()
}

fn queue_terminal_update(
    tx: &mpsc::Sender<OutputPacket>,
    state: &TerminalScreen,
    chunk: String,
) -> bool {
    let Some(websocket_payload) = build_terminal_action_payload(state, chunk.as_str()) else {
        println!("Failed to build response package.");
        return false;
    };

    tx.blocking_send(OutputPacket {
        websocket_payload,
        history_chunk: chunk,
    })
    .is_ok()
}

fn broadcast_terminal_update(
    task_key: &str,
    stdout_tx: &broadcast::Sender<String>,
    state: &TerminalScreen,
    chunk: &str,
) {
    append_task_history_blocking(task_key, chunk);

    let Some(payload) = build_terminal_action_payload(state, chunk) else {
        println!("Failed to build response package.");
        return;
    };

    let _ = stdout_tx.send(payload);
}

async fn start_build(
    session: &mut Session,
    stdin_state: Arc<Mutex<BuildInputState>>,
    action: Action,
    task_key: String,
) {
    send_status(session, "RUNNING").await;
    let pty_system = native_pty_system();

    let rows = 30u16;
    let cols = 120u16;

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

    let (input_tx, mut input_rx) = mpsc::channel::<String>(100);
    let (stdout_tx, _) = broadcast::channel::<String>(200);

    let child = Arc::new(StdRwLock::new(child));

    let reader = match pty_pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(err) => {
            send_status(session, "FAILED").await;
            send_stdout_for_task(
                session,
                task_key.as_str(),
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
            send_stdout_for_task(
                session,
                task_key.as_str(),
                format!("failed to open PTY writer: {err}").as_str(),
            )
            .await;
            return;
        }
    };

    register_process(&action, child.clone(), input_tx.clone(), stdout_tx.clone()).await;
    flush_pending_stdin(task_key.as_str(), stdin_state.clone()).await;
    drop(pty_pair.slave);

    let (sender, mut receiver) = mpsc::channel::<OutputPacket>(100);
    let tx = sender.clone();

    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0_u8; 4096];
        let mut state = TerminalScreen::new(rows as usize, cols as usize);
        let mut utf8_pending = Vec::new();

        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    if !utf8_pending.is_empty() {
                        let mut chunk = decode_utf8_chunk(&mut utf8_pending);

                        if !utf8_pending.is_empty() {
                            chunk.push_str(&String::from_utf8_lossy(&utf8_pending));
                            utf8_pending.clear();
                        }

                        if !chunk.is_empty() {
                            state.feed(&chunk);
                            if !queue_terminal_update(&tx, &state, chunk) {
                                break;
                            }
                        }
                    }

                    break;
                }
                Ok(n) => {
                    utf8_pending.extend_from_slice(&buf[..n]);
                    let chunk = decode_utf8_chunk(&mut utf8_pending);

                    if chunk.is_empty() {
                        continue;
                    }

                    state.feed(&chunk);

                    if !queue_terminal_update(&tx, &state, chunk) {
                        break;
                    }
                }
                Err(err) if err.raw_os_error() == Some(5) => {
                    // EIO: normal PTY termination on Linux.
                    break;
                }
                Err(err) => {
                    let line = format!("PTY read error: {err}");

                    if let Some(payload) = stdout_response_json(line.clone()) {
                        let _ = tx.blocking_send(OutputPacket {
                            websocket_payload: payload,
                            history_chunk: line,
                        });
                    }

                    break;
                }
            }
        }
    });

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

    let mut session_open = true;

    while let Some(packet) = receiver.recv().await {
        append_task_history(task_key.as_str(), packet.history_chunk.as_str()).await;
        let _ = stdout_tx.send(packet.websocket_payload.clone());

        if session_open && session.text(packet.websocket_payload).await.is_err() {
            session_open = false;
        }
    }

    let status = wait_for_exit(child);
    println!("Task ended: {:?}", status);

    unregister_process(&action).await;

    if let Ok(status) = status {
        let exit_code = status.exit_code();
        let out = format!("Task finished, exit code: {:?}", exit_code);

        send_stdout_for_task(session, task_key.as_str(), out.as_str()).await;

        if exit_code == 0 {
            send_status(session, "SUCCESS").await;
        } else {
            send_status(session, "FAILED").await;
        }
    } else {
        send_status(session, "FAILED").await;
        send_stdout_for_task(session, task_key.as_str(), "Error").await;
    }

    let mut state = stdin_state.lock().await;

    if state.task_key.as_deref() == Some(task_key.as_str()) {
        state.task_key = None;
        state.pending.clear();
    }
}
