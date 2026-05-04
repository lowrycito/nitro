//! Plain-text terminal output helpers.
//!
//! Phase 4 doesn't pull in ratatui yet — every renderer here is line-based
//! so the binary works in any TTY (or pipe) without raw-mode shenanigans.
//! When ratatui lands in Phase 5+, these stay as fallbacks for non-TTY use
//! (CI, scripts, etc).

use std::io::{IsTerminal, Write};

const RESET: &str = "\x1b[0m";

fn rgb(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[38;2;{r};{g};{b}m")
}

pub fn red() -> String {
    rgb(0xE6, 0x7E, 0x80)
}
pub fn yellow() -> String {
    rgb(0xDB, 0xBC, 0x7F)
}
pub fn aqua() -> String {
    rgb(0x83, 0xC0, 0x92)
}
pub fn fg_secondary() -> String {
    rgb(0x9D, 0xA9, 0xA0)
}

fn supports_color() -> bool {
    std::io::stdout().is_terminal()
}

fn paint(color: &str, text: &str) -> String {
    if supports_color() {
        format!("{color}{text}{RESET}")
    } else {
        text.to_string()
    }
}

pub fn error(msg: &str) {
    let _ = writeln!(std::io::stderr(), "{}", paint(&red(), msg));
}

pub fn info(msg: &str) {
    let _ = writeln!(std::io::stdout(), "{msg}");
}

pub fn dim(msg: &str) {
    let _ = writeln!(std::io::stdout(), "{}", paint(&fg_secondary(), msg));
}

pub fn warn(msg: &str) {
    let _ = writeln!(std::io::stdout(), "{}", paint(&yellow(), msg));
}

pub fn success(msg: &str) {
    let _ = writeln!(std::io::stdout(), "{}", paint(&aqua(), msg));
}

/// Write streamed text without a trailing newline. Flushes so the user
/// sees deltas as they arrive rather than at end-of-line.
pub fn stream(text: &str) {
    let mut out = std::io::stdout();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

pub fn newline() {
    let _ = writeln!(std::io::stdout());
}
