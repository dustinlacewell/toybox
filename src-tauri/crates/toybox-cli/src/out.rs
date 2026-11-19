//! Writing results: aligned text tables, JSON documents, and the labelled lines on
//! standard error. Standard output is always just the result.

use std::io::{IsTerminal, Write};

use serde::Serialize;

use crate::error::{CliError, CliResult};

/// Write to standard output. A closed pipe (`tbx search | head`) ends the program quietly.
pub fn stdout(text: &str) {
    let mut out = std::io::stdout().lock();
    if out.write_all(text.as_bytes()).and_then(|_| out.flush()).is_err() {
        std::process::exit(0);
    }
}

/// One line on standard output.
pub fn line(text: &str) {
    stdout(&format!("{text}\n"));
}

/// Write to standard error (progress, warnings). Failures are ignored.
pub fn stderr(text: &str) {
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(text.as_bytes());
    let _ = err.flush();
}

pub fn stdout_is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

pub fn stdin_is_terminal() -> bool {
    std::io::stdin().is_terminal()
}

pub fn stderr_is_terminal() -> bool {
    std::io::stderr().is_terminal()
}

/// Whether colour is wanted on standard error for `--color <when>`.
pub fn color_wanted(when: &str) -> bool {
    match when {
        "always" => true,
        "never" => false,
        _ => {
            if std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
                return false;
            }
            if !stderr_is_terminal() || std::env::var("TERM").is_ok_and(|term| term == "dumb") {
                return false;
            }
            !cfg!(windows) || std::env::var_os("WT_SESSION").is_some() || std::env::var_os("TERM").is_some() || std::env::var_os("ANSICON").is_some()
        }
    }
}

/// `label` painted for a terminal when `color` is on.
pub fn paint(label: &str, code: &str, color: bool) -> String {
    if color { format!("\x1b[{code}m{label}\x1b[0m") } else { label.to_string() }
}

/// Print an error the way docs/cli.md says: `error: ...` and `hint: ...` lines, or a JSON document
/// on standard error.
pub fn print_error(error: &CliError, json: bool, color: bool) {
    if json {
        let doc = serde_json::json!({ "error": { "code": error.kind.code(), "message": error.message } });
        stderr(&format!("{doc}\n"));
        return;
    }
    stderr(&format!("{}: {}\n", paint("error", "1;31", color), error.message));
    if let Some(hint) = &error.hint {
        stderr(&format!("{}: {}\n", paint("hint", "1;36", color), hint));
    }
}

/// Print one JSON document on standard output.
pub fn json<T: Serialize>(doc: &T) -> CliResult<()> {
    line(&serde_json::to_string_pretty(doc)?);
    Ok(())
}

/// Rows of text in aligned columns. Headers are shown only on a terminal, so piped output is rows only.
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    always_headers: bool,
}

impl Table {
    pub fn new(headers: &[&str]) -> Self {
        Table { headers: headers.iter().map(|header| header.to_string()).collect(), rows: Vec::new(), always_headers: false }
    }

    /// A table whose header row is shown even when output is piped (query results).
    pub fn with_headers(headers: &[String]) -> Self {
        Table { headers: headers.to_vec(), rows: Vec::new(), always_headers: true }
    }

    pub fn row(&mut self, cells: Vec<String>) {
        self.rows.push(cells);
    }

    /// The table as text, ending in a newline (empty when there are no rows).
    pub fn render(&self) -> String {
        if self.rows.is_empty() {
            return String::new();
        }
        let show_headers = self.always_headers || stdout_is_terminal();
        let columns = self.headers.len();
        let mut widths: Vec<usize> = self.headers.iter().map(|header| if show_headers { header.chars().count() } else { 0 }).collect();
        for row in &self.rows {
            for (index, cell) in row.iter().enumerate().take(columns) {
                widths[index] = widths[index].max(cell.chars().count());
            }
        }
        let mut out = String::new();
        let mut write_row = |cells: &mut dyn Iterator<Item = &str>| {
            let mut line = String::new();
            for (index, cell) in cells.enumerate().take(columns) {
                if index > 0 {
                    line.push_str("  ");
                }
                line.push_str(cell);
                if index + 1 < columns {
                    for _ in cell.chars().count()..widths[index] {
                        line.push(' ');
                    }
                }
            }
            out.push_str(line.trim_end());
            out.push('\n');
        };
        if show_headers {
            write_row(&mut self.headers.iter().map(String::as_str));
        }
        for row in &self.rows {
            write_row(&mut row.iter().map(String::as_str));
        }
        out
    }
}

/// Cut `text` to at most `max` characters, ending in `...` when it was cut.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(3)).collect();
    format!("{kept}...")
}

/// `1.5 MB` style sizes.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// `1 item` / `3 items`.
pub fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}
