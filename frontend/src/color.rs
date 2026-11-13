use std::{cell::RefCell, collections::HashMap};

#[derive(Default)]
struct AnsiRenderState {
    pending_escape: String,
    bold: bool,
    fg_class: Option<&'static str>,
    bg_class: Option<&'static str>,
    hyperlink: Option<String>,
    lines: Vec<String>,
    current_line_cells: Vec<String>,
    cursor_col: usize,
    insert_mode: bool,
}

thread_local! {
    static RENDER_STATES: RefCell<HashMap<String, AnsiRenderState>> = RefCell::new(HashMap::new());
}

pub fn clear_render_state(id: &str) {
    RENDER_STATES.with(|states| {
        states.borrow_mut().remove(id);
    });
}

pub fn append_ansi_chunk(id: &str, chunk: &str, cursor_col: Option<usize>) -> String {
    RENDER_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let state = states.entry(id.to_string()).or_default();
        let mut input = String::new();
        input.push_str(&state.pending_escape);
        input.push_str(chunk);
        state.pending_escape.clear();

        ansi_to_html_streaming(&input, state);

        if let Some(col) = cursor_col {
            state.cursor_col = col;
            render_buffer_with_caret(state)
        } else {
            render_buffer(state)
        }
    })
}

fn render_buffer(state: &AnsiRenderState) -> String {
    let current_line = state.current_line_cells.join("");

    if state.lines.is_empty() {
        return current_line;
    }

    let mut out = state.lines.join("\n");
    out.push('\n');
    out.push_str(&current_line);
    out
}

fn render_buffer_with_caret(state: &AnsiRenderState) -> String {
    let current_line = render_line_with_caret(&state.current_line_cells, state.cursor_col);

    if state.lines.is_empty() {
        return current_line;
    }

    let mut out = state.lines.join("\n");
    out.push('\n');
    out.push_str(&current_line);
    out
}

fn render_line_with_caret(cells: &[String], cursor_col: usize) -> String {
    if cells.is_empty() {
        let mut out = String::new();
        if cursor_col > 0 {
            out.push_str(&" ".repeat(cursor_col));
        }
        out.push_str(r#"<span class="terminal-caret">&nbsp;</span>"#);
        return out;
    }

    if cursor_col >= cells.len() {
        let mut out = cells.join("");
        if cursor_col > cells.len() {
            out.push_str(&" ".repeat(cursor_col - cells.len()));
        }
        out.push_str(r#"<span class="terminal-caret">&nbsp;</span>"#);
        return out;
    }

    let mut out = String::new();

    for cell in cells.iter().take(cursor_col) {
        out.push_str(cell);
    }

    out.push_str(r#"<span class="terminal-caret">"#);
    out.push_str(&cells[cursor_col]);
    out.push_str("</span>");

    for cell in cells.iter().skip(cursor_col + 1) {
        out.push_str(cell);
    }

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
                    b'D' => apply_cursor_left(params, state),
                    b'C' => apply_cursor_right(params, state),
                    b'@' => apply_insert_chars(params, state),
                    b'P' => apply_delete_chars(params, state),
                    b'h' => apply_mode_set(params, state, true),
                    b'l' => apply_mode_set(params, state, false),
                    _ => {}
                }

                i = j + 1;
                text_start = i;
            }
            b']' => {
                let mut j = i + 2;
                let mut osc_end: Option<(usize, usize)> = None;

                while j < bytes.len() {
                    if bytes[j] == 0x07 {
                        // BEL terminated OSC
                        osc_end = Some((j, 1));
                        break;
                    }

                    if j + 1 < bytes.len() && bytes[j] == 0x1b && bytes[j + 1] == b'\\' {
                        // ST terminated OSC
                        osc_end = Some((j, 2));
                        break;
                    }

                    j += 1;
                }

                let Some((end_idx, terminator_len)) = osc_end else {
                    state.pending_escape.push_str(&input[i..]);
                    return;
                };

                let osc_content = &input[i + 2..end_idx];
                apply_osc(osc_content, state);

                i = end_idx + terminator_len;
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
        state.fg_class = None;
        state.bg_class = None;
        return;
    }

    for raw in params.split(';') {
        let code = raw.parse::<u16>().unwrap_or(0);
        match code {
            0 => {
                state.bold = false;
                state.fg_class = None;
                state.bg_class = None;
            }
            1 => state.bold = true,
            22 => state.bold = false,
            39 => state.fg_class = None,
            49 => state.bg_class = None,
            30..=37 | 90..=97 => {
                state.fg_class = sgr_fg_class(code);
            }
            40..=47 | 100..=107 => {
                state.bg_class = sgr_bg_class(code);
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

fn sgr_fg_class(code: u16) -> Option<&'static str> {
    match code {
        30 => Some("terminal-fg-black"),
        31 => Some("terminal-fg-red"),
        32 => Some("terminal-fg-green"),
        33 => Some("terminal-fg-yellow"),
        34 => Some("terminal-fg-blue"),
        35 => Some("terminal-fg-magenta"),
        36 => Some("terminal-fg-cyan"),
        37 => Some("terminal-fg-white"),
        90 => Some("terminal-fg-bright-black"),
        91 => Some("terminal-fg-bright-red"),
        92 => Some("terminal-fg-bright-green"),
        93 => Some("terminal-fg-bright-yellow"),
        94 => Some("terminal-fg-bright-blue"),
        95 => Some("terminal-fg-bright-magenta"),
        96 => Some("terminal-fg-bright-cyan"),
        97 => Some("terminal-fg-bright-white"),
        _ => None,
    }
}

fn sgr_bg_class(code: u16) -> Option<&'static str> {
    match code {
        40 => Some("terminal-bg-black"),
        41 => Some("terminal-bg-red"),
        42 => Some("terminal-bg-green"),
        43 => Some("terminal-bg-yellow"),
        44 => Some("terminal-bg-blue"),
        45 => Some("terminal-bg-magenta"),
        46 => Some("terminal-bg-cyan"),
        47 => Some("terminal-bg-white"),
        100 => Some("terminal-bg-bright-black"),
        101 => Some("terminal-bg-bright-red"),
        102 => Some("terminal-bg-bright-green"),
        103 => Some("terminal-bg-bright-yellow"),
        104 => Some("terminal-bg-bright-blue"),
        105 => Some("terminal-bg-bright-magenta"),
        106 => Some("terminal-bg-bright-cyan"),
        107 => Some("terminal-bg-bright-white"),
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

    if state.bold || state.fg_class.is_some() || state.bg_class.is_some() {
        let mut classes = String::from("terminal-span");

        if state.bold {
            classes.push_str(" terminal-bold");
        }

        if let Some(class_name) = state.fg_class {
            classes.push(' ');
            classes.push_str(class_name);
        }

        if let Some(class_name) = state.bg_class {
            classes.push(' ');
            classes.push_str(class_name);
        }

        html = format!(r#"<span class="{classes}">{html}</span>"#);
    }

    html
}

fn normalize_terminal_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());

    for ch in s.chars() {
        match ch {
            // Drop C0 control chars here.
            // Keep tab/newline/carriage-return so screen rendering retains rows.
            '\u{0000}'..='\u{001F}' if ch != '\t' && ch != '\n' && ch != '\r' => {}
            _ => out.push(ch),
        }
    }

    strip_known_terminal_artifacts(&out)
}

pub fn render_terminal_screen(screen: &str) -> String {
    let sanitized = sanitize_screen_text(screen);
    let mut out = String::new();

    for (row_idx, line) in sanitized.split('\n').enumerate() {
        if row_idx > 0 {
            out.push('\n');
        }

        out.push_str(&render_screen_line_with_shortcuts(line));
    }

    out
}

pub fn render_terminal_screen_with_cursor(
    screen: &str,
    cursor_row: usize,
    cursor_col: usize,
) -> String {
    let sanitized = sanitize_screen_text(screen);
    let mut out = String::new();

    for (row_idx, line) in sanitized.split('\n').enumerate() {
        if row_idx > 0 {
            out.push('\n');
        }

        if row_idx != cursor_row {
            out.push_str(&render_screen_line_with_shortcuts(line));
            continue;
        }

        let chars: Vec<char> = line.chars().collect();

        if chars.is_empty() {
            out.push_str(r#"<span class="terminal-caret">&nbsp;</span>"#);
            continue;
        }

        let clamped_col = cursor_col.min(chars.len().saturating_sub(1));

        let before: String = chars.iter().take(clamped_col).collect();
        let cur = chars[clamped_col];
        let after: String = chars.iter().skip(clamped_col + 1).collect();

        out.push_str(&render_screen_line_with_shortcuts(&before));
        out.push_str(r#"<span class="terminal-caret">"#);
        out.push_str(&escape_html(&cur.to_string()));
        out.push_str("</span>");
        out.push_str(&render_screen_line_with_shortcuts(&after));
    }

    out
}

fn sanitize_screen_text(screen: &str) -> String {
    let without_osc = strip_esc_sequences(screen);
    normalize_terminal_text(&without_osc)
}

fn render_screen_line_with_shortcuts(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;

    while i < chars.len() {
        if is_control_shortcut_start(&chars, i) {
            out.push_str(r#"<span class="terminal-inverse">^"#);
            out.push(chars[i + 1]);
            out.push_str("</span>");
            i += 2;
            continue;
        }

        push_escaped_char(&mut out, chars[i]);
        i += 1;
    }

    out
}

fn is_control_shortcut_start(chars: &[char], idx: usize) -> bool {
    if idx + 1 >= chars.len() || chars[idx] != '^' {
        return false;
    }

    let key = chars[idx + 1];
    if !key.is_ascii_uppercase() {
        return false;
    }

    let prev_ok = if idx == 0 {
        true
    } else {
        let prev = chars[idx - 1];
        prev.is_whitespace() || prev == '(' || prev == '[' || prev == '{'
    };

    let next_ok = if idx + 2 >= chars.len() {
        true
    } else {
        chars[idx + 2].is_whitespace()
    };

    prev_ok && next_ok
}

fn push_escaped_char(out: &mut String, ch: char) {
    match ch {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        '\'' => out.push_str("&#39;"),
        _ => out.push(ch),
    }
}

fn strip_esc_sequences(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != 0x1b {
            out.push(bytes[i]);
            i += 1;
            continue;
        }

        if i + 1 >= bytes.len() {
            break;
        }

        match bytes[i + 1] {
            b']' => {
                let mut j = i + 2;
                let mut found = false;

                while j < bytes.len() {
                    if bytes[j] == 0x07 {
                        i = j + 1;
                        found = true;
                        break;
                    }

                    if j + 1 < bytes.len() && bytes[j] == 0x1b && bytes[j + 1] == b'\\' {
                        i = j + 2;
                        found = true;
                        break;
                    }

                    j += 1;
                }

                if !found {
                    break;
                }
            }
            b'[' => {
                let mut j = i + 2;
                while j < bytes.len() {
                    if (0x40..=0x7e).contains(&bytes[j]) {
                        break;
                    }
                    j += 1;
                }

                if j >= bytes.len() {
                    break;
                }

                i = j + 1;
            }
            _ => {
                let mut j = i + 1;
                while j < bytes.len() && (0x20..=0x2f).contains(&bytes[j]) {
                    j += 1;
                }

                if j < bytes.len() && (0x30..=0x7e).contains(&bytes[j]) {
                    i = j + 1;
                } else {
                    i += 1;
                }
            }
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

fn strip_known_terminal_artifacts(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;

    while i < bytes.len() {
        if starts_with_at(bytes, i, b"666;vte.shell.postexec=") {
            i += b"666;vte.shell.postexec=".len();
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            continue;
        }

        if starts_with_at(bytes, i, b"666;vte.shell.precmd!") {
            i += b"666;vte.shell.precmd!".len();
            continue;
        }

        if starts_with_at(bytes, i, b"666;vte.shell.preexec!") {
            i += b"666;vte.shell.preexec!".len();
            continue;
        }

        if starts_with_at(bytes, i, b"7;file://") {
            i = skip_prompt_metadata_fragment(bytes, i);
            continue;
        }

        if starts_with_at(bytes, i, b"0;") && looks_like_title_fragment(bytes, i) {
            i = skip_prompt_metadata_fragment(bytes, i);
            continue;
        }

        out.push(bytes[i]);
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

fn skip_prompt_metadata_fragment(bytes: &[u8], start: usize) -> usize {
    let mut i = start;

    while i < bytes.len() {
        let b = bytes[i];

        if b == b'\n' || b == b'\r' || b == 0x1b || b == 0x07 {
            break;
        }

        if i > start
            && (starts_with_at(bytes, i, b"666;vte.shell.") || starts_with_at(bytes, i, b"0;"))
        {
            break;
        }

        i += 1;
    }

    i
}

fn looks_like_title_fragment(bytes: &[u8], start: usize) -> bool {
    let mut i = start + 2;
    let mut has_at = false;
    let mut has_colon = false;
    let limit = (start + 160).min(bytes.len());

    while i < limit {
        let b = bytes[i];
        if b == b'\n' || b == b'\r' || b == 0x1b || b == 0x07 {
            break;
        }
        if b == b'@' {
            has_at = true;
        } else if b == b':' {
            has_colon = true;
        }

        if i > start && starts_with_at(bytes, i, b"666;vte.shell.") {
            break;
        }

        i += 1;
    }

    has_at && has_colon
}

fn starts_with_at(haystack: &[u8], idx: usize, needle: &[u8]) -> bool {
    idx + needle.len() <= haystack.len() && &haystack[idx..idx + needle.len()] == needle
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
                state.cursor_col = 0;
            }
            '\n' => {
                flush_text_run(&run, state);
                run.clear();
                let rendered_line = state.current_line_cells.join("");
                state.lines.push(rendered_line);
                state.current_line_cells.clear();
                state.cursor_col = 0;
            }
            '\u{0008}' => {
                // BS moves the cursor left by one cell; it should not delete by itself.
                flush_text_run(&run, state);
                run.clear();
                state.cursor_col = state.cursor_col.saturating_sub(1);
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

    for ch in run.chars() {
        let rendered = render_text(&ch.to_string(), state);
        if rendered.is_empty() {
            continue;
        }

        if state.cursor_col < state.current_line_cells.len() {
            if state.insert_mode {
                state.current_line_cells.insert(state.cursor_col, rendered);
            } else {
                state.current_line_cells[state.cursor_col] = rendered;
            }
        } else {
            state.current_line_cells.push(rendered);
        }

        state.cursor_col += 1;
    }
}

fn apply_erase_in_line(params: &str, state: &mut AnsiRenderState) {
    // EL (Erase in Line):
    // - ESC[K / ESC[0K => clear from cursor to end of line
    // - ESC[2K          => clear entire line
    match params {
        "" | "0" => {
            state.current_line_cells.truncate(state.cursor_col);
        }
        "2" => {
            state.current_line_cells.clear();
            state.cursor_col = 0;
        }
        _ => {}
    }
}

fn apply_cursor_left(params: &str, state: &mut AnsiRenderState) {
    let n = parse_cursor_n(params);
    state.cursor_col = state.cursor_col.saturating_sub(n);
}

fn apply_cursor_right(params: &str, state: &mut AnsiRenderState) {
    let n = parse_cursor_n(params);
    state.cursor_col = (state.cursor_col + n).min(state.current_line_cells.len());
}

fn apply_insert_chars(params: &str, state: &mut AnsiRenderState) {
    let n = parse_cursor_n(params);

    if n == 0 {
        return;
    }

    let col = state.cursor_col.min(state.current_line_cells.len());
    let blank_cell = render_text(" ", state);
    let blanks = std::iter::repeat_n(blank_cell, n);
    state.current_line_cells.splice(col..col, blanks);
}

fn apply_delete_chars(params: &str, state: &mut AnsiRenderState) {
    let n = parse_cursor_n(params);

    if state.cursor_col >= state.current_line_cells.len() {
        return;
    }

    let end = (state.cursor_col + n).min(state.current_line_cells.len());
    state.current_line_cells.drain(state.cursor_col..end);
}

fn apply_mode_set(params: &str, state: &mut AnsiRenderState, enable: bool) {
    if params.starts_with('?') {
        return;
    }

    for raw in params.split(';') {
        if raw == "4" {
            state.insert_mode = enable;
        }
    }
}

fn parse_cursor_n(params: &str) -> usize {
    if params.is_empty() {
        return 1;
    }

    params.parse::<usize>().ok().filter(|n| *n > 0).unwrap_or(1)
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

    #[test]
    fn backspace_with_erase_to_eol_does_not_clear_whole_line() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("hello\u{0008}\u{1b}[K", &mut state);
        assert_eq!(render_buffer(&state), "hell");
    }

    #[test]
    fn backspace_without_erase_only_moves_cursor() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("hello\u{0008}", &mut state);
        assert_eq!(render_buffer(&state), "hello");
    }

    #[test]
    fn csi_cursor_left_does_not_delete_character() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("abcd\u{1b}[D", &mut state);
        assert_eq!(render_buffer(&state), "abcd");
    }

    #[test]
    fn delete_char_at_cursor_shifts_tail_left() {
        let mut state = AnsiRenderState::default();
        // Simulate: type abcd, move cursor left once (before d), backspace,
        // then terminal sends DCH to delete at cursor.
        ansi_to_html_streaming("abcd\u{1b}[D\u{0008}\u{1b}[P", &mut state);
        assert_eq!(render_buffer(&state), "abd");
    }

    #[test]
    fn insert_mode_inserts_chars_at_cursor() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("abcd\u{1b}[2D\u{1b}[4hX\u{1b}[4l", &mut state);
        assert_eq!(render_buffer(&state), "abXcd");
    }

    #[test]
    fn default_mode_overwrites_chars_at_cursor() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("abcd\u{1b}[2DX", &mut state);
        assert_eq!(render_buffer(&state), "abXd");
    }

    #[test]
    fn osc_bel_is_consumed_without_leaking_payload() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("A\u{1b}]666;vte.shell.precmd!\u{7}B", &mut state);
        assert_eq!(render_buffer(&state), "AB");
    }

    #[test]
    fn leaked_vte_fragments_are_removed_from_plain_screen_text() {
        let sample = "666;vte.shell.postexec=07;file://summerbird/home/johan/Downloads/resting-chair0;\njohan@summerbird:~/Downloads/resting-chair666;vte.shell.precmd!";
        let rendered = render_terminal_screen(sample);

        assert!(!rendered.contains("666;vte.shell"));
        assert!(!rendered.contains("7;file://"));
        assert!(rendered.contains("johan@summerbird:~/Downloads/resting-chair"));
    }

    #[test]
    fn sgr_wraps_text_in_css_classes() {
        let mut state = AnsiRenderState::default();
        ansi_to_html_streaming("\u{1b}[1;31merror\u{1b}[0m", &mut state);
        let out = render_buffer(&state);

        assert!(out.contains("terminal-bold"));
        assert!(out.contains("terminal-fg-red"));
    }

    #[test]
    fn nano_style_control_shortcuts_are_rendered_in_inverse_spans() {
        let rendered = render_terminal_screen("^X Exit  ^O WriteOut");

        assert!(rendered.contains("<span class=\"terminal-inverse\">^X</span> Exit"));
        assert!(rendered.contains("<span class=\"terminal-inverse\">^O</span> WriteOut"));
    }

    #[test]
    fn caret_sequences_inside_words_are_not_forced_to_inverse() {
        let rendered = render_terminal_screen("token^Xvalue");
        assert!(!rendered.contains("terminal-inverse"));
    }
}
