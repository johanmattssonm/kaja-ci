use std::{cell::RefCell, collections::HashMap};

#[derive(Default)]
struct AnsiRenderState {
    pending_escape: String,
    bold: bool,
    fg_color: Option<&'static str>,
    hyperlink: Option<String>,
    lines: Vec<String>,
    current_line: String,
    carriage_return_pending: bool,
}

thread_local! {
    static RENDER_STATES: RefCell<HashMap<String, AnsiRenderState>> = RefCell::new(HashMap::new());
}

pub fn clear_render_state(id: &str) {
    RENDER_STATES.with(|states| {
        states.borrow_mut().remove(id);
    });
}

pub fn append_ansi_chunk(id: &str, chunk: &str) -> String {
    RENDER_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let state = states.entry(id.to_string()).or_default();
        let mut input = String::new();
        input.push_str(&state.pending_escape);
        input.push_str(chunk);
        state.pending_escape.clear();

        ansi_to_html_streaming(&input, state);
        render_buffer(state)
    })
}

fn render_buffer(state: &AnsiRenderState) -> String {
    if state.lines.is_empty() {
        return state.current_line.clone();
    }

    let mut out = state.lines.join("\n");
    out.push('\n');
    out.push_str(&state.current_line);
    out
}

fn ansi_to_html_streaming(input: &str, state: &mut AnsiRenderState) {
    let bytes = input.as_bytes();
    let mut text_start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != 0x1b {
            i += 1;
            continue;
        }

        if text_start < i {
            append_text_with_controls(&input[text_start..i], state);
        }

        if i + 1 >= bytes.len() {
            state.pending_escape.push_str(&input[i..]);
            return;
        }

        match bytes[i + 1] {
            b'[' => {
                let mut j = i + 2;
                while j < bytes.len() {
                    let b = bytes[j];
                    if (0x40..=0x7e).contains(&b) {
                        break;
                    }
                    j += 1;
                }

                if j >= bytes.len() {
                    state.pending_escape.push_str(&input[i..]);
                    return;
                }

                let final_byte = bytes[j];
                let params = &input[i + 2..j];

                match final_byte {
                    b'm' => apply_sgr(params, state),
                    b'K' => apply_erase_in_line(params, state),
                    _ => {}
                }

                i = j + 1;
                text_start = i;
            }
            b']' => {
                let mut j = i + 2;
                let mut found_st = None;
                while j + 1 < bytes.len() {
                    if bytes[j] == 0x1b && bytes[j + 1] == b'\\' {
                        found_st = Some(j);
                        break;
                    }
                    j += 1;
                }

                let Some(st_idx) = found_st else {
                    state.pending_escape.push_str(&input[i..]);
                    return;
                };

                let osc_content = &input[i + 2..st_idx];
                apply_osc(osc_content, state);

                i = st_idx + 2;
                text_start = i;
            }
            _ => {
                // Handle generic ESC sequences like ESC ( B (character set selection).
                // Format: ESC + 0+ intermediates (0x20..=0x2F) + final (0x30..=0x7E).
                let mut j = i + 1;

                while j < bytes.len() && (0x20..=0x2f).contains(&bytes[j]) {
                    j += 1;
                }

                if j >= bytes.len() {
                    state.pending_escape.push_str(&input[i..]);
                    return;
                }

                if (0x30..=0x7e).contains(&bytes[j]) {
                    i = j + 1;
                    text_start = i;
                } else {
                    // Malformed sequence; drop ESC and continue.
                    i += 1;
                    text_start = i;
                }
            }
        }
    }

    if text_start < bytes.len() {
        append_text_with_controls(&input[text_start..], state);
    }
}

fn apply_sgr(params: &str, state: &mut AnsiRenderState) {
    if params.is_empty() {
        state.bold = false;
        state.fg_color = None;
        return;
    }

    for raw in params.split(';') {
        let code = raw.parse::<u16>().unwrap_or(0);
        match code {
            0 => {
                state.bold = false;
                state.fg_color = None;
            }
            1 => state.bold = true,
            22 => state.bold = false,
            39 => state.fg_color = None,
            30..=37 | 90..=97 => {
                state.fg_color = sgr_fg_color(code);
            }
            _ => {}
        }
    }
}

fn apply_osc(osc_content: &str, state: &mut AnsiRenderState) {
    if let Some(rest) = osc_content.strip_prefix("8;;") {
        if rest.is_empty() {
            state.hyperlink = None;
        } else {
            state.hyperlink = Some(rest.to_string());
        }
    }
}

fn sgr_fg_color(code: u16) -> Option<&'static str> {
    match code {
        30 => Some("#000000"),
        31 => Some("#aa0000"),
        32 => Some("#00aa00"),
        33 => Some("#aa5500"),
        34 => Some("#3366ff"),
        35 => Some("#aa00aa"),
        36 => Some("#00aaaa"),
        37 => Some("#aaaaaa"),
        90 => Some("#555555"),
        91 => Some("#ff5555"),
        92 => Some("#55ff55"),
        93 => Some("#ffff55"),
        94 => Some("#5555ff"),
        95 => Some("#ff55ff"),
        96 => Some("#55ffff"),
        97 => Some("#ffffff"),
        _ => None,
    }
}

fn render_text(text: &str, state: &AnsiRenderState) -> String {
    if text.is_empty() {
        return String::new();
    }

    let normalized = normalize_terminal_text(text);
    if normalized.is_empty() {
        return String::new();
    }

    let mut html = escape_html(&normalized);

    if let Some(url) = &state.hyperlink {
        let href = escape_attr(url);
        html = format!(r#"<a href="{href}" target="_blank" rel="noopener noreferrer">{html}</a>"#);
    }

    if state.bold || state.fg_color.is_some() {
        let mut style = String::new();
        if state.bold {
            style.push_str("font-weight:700;");
        }
        if let Some(color) = state.fg_color {
            style.push_str("color:");
            style.push_str(color);
            style.push(';');
        }
        html = format!(r#"<span style="{style}">{html}</span>"#);
    }

    html
}

fn normalize_terminal_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());

    for ch in s.chars() {
        match ch {
            // Drop C0 control chars here; CR/LF are handled by append_text_with_controls.
            '\u{0000}'..='\u{001F}' if ch != '\t' => {}
            _ => out.push(ch),
        }
    }

    out
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_attr(s: &str) -> String {
    escape_html(s)
}

fn append_text_with_controls(text: &str, state: &mut AnsiRenderState) {
    let mut run = String::new();

    for ch in text.chars() {
        match ch {
            '\r' => {
                flush_text_run(&run, state);
                run.clear();
                // CR moves cursor to start of line; do not clear yet.
                // The next printable text should replace from column 0.
                state.carriage_return_pending = true;
            }
            '\n' => {
                flush_text_run(&run, state);
                run.clear();
                state.lines.push(state.current_line.clone());
                state.current_line.clear();
                state.carriage_return_pending = false;
            }
            '\u{0008}' => {
                // Best-effort handling: drop from the unflushed run.
                run.pop();
            }
            _ => run.push(ch),
        }
    }

    flush_text_run(&run, state);
}

fn flush_text_run(run: &str, state: &mut AnsiRenderState) {
    if run.is_empty() {
        return;
    }

    let rendered = render_text(run, state);
    if rendered.is_empty() {
        return;
    }

    if state.carriage_return_pending {
        state.current_line.clear();
        state.carriage_return_pending = false;
    }

    state.current_line.push_str(&rendered);
}

fn apply_erase_in_line(params: &str, state: &mut AnsiRenderState) {
    // Common progress-meter sequence is ESC[2K (erase whole line).
    // We also treat empty and "0" as clear-to-end; with our cursor model at line end,
    // both effectively clear the current line.
    match params {
        "" | "0" | "2" => {
            state.current_line.clear();
            state.carriage_return_pending = false;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_paren_b_does_not_leak_b() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("ok\u{1b}(B", &mut state);
        let out = render_buffer(&state);
        assert_eq!(out, "ok");
        assert!(state.pending_escape.is_empty());
    }

    #[test]
    fn split_esc_paren_b_across_chunks_does_not_leak_b() {
        let mut state = AnsiRenderState::default();

        ansi_to_html_streaming("ok\u{1b}", &mut state);
        let out1 = render_buffer(&state);
        assert_eq!(out1, "ok");
        assert_eq!(state.pending_escape, "\u{1b}");

        let mut input2 = String::new();
        input2.push_str(&state.pending_escape);
        state.pending_escape.clear();
        input2.push_str("(B");

        ansi_to_html_streaming(&input2, &mut state);
        let out2 = render_buffer(&state);
        assert_eq!(out2, "ok");
        assert!(state.pending_escape.is_empty());
    }

    #[test]
    fn carriage_return_replaces_current_line() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("Compiling foo\rCompiling bar", &mut state);
        assert_eq!(render_buffer(&state), "Compiling bar");
    }

    #[test]
    fn carriage_return_without_rewrite_keeps_line_visible() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("Compiling foo\r", &mut state);
        assert_eq!(render_buffer(&state), "Compiling foo");
    }

    #[test]
    fn erase_line_clears_progress_before_rewrite() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("Step 1\u{1b}[2K\rStep 2", &mut state);
        assert_eq!(render_buffer(&state), "Step 2");
    }
}
