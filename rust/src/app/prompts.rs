//! Tiny line-mode prompts used by Phase 4. Replaced by ratatui modals in
//! Phase 7 but kept available for non-TTY use (CI, scripted sessions).

use std::io::{BufRead, IsTerminal, Write};

/// Prompts the user with a label and reads a line back. Returns `None` if
/// stdin is closed or not a TTY (so non-interactive callers don't hang).
pub fn read_line(prompt: &str) -> Option<String> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    let _ = write!(std::io::stdout(), "{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let n = std::io::stdin().lock().read_line(&mut line).ok()?;
    if n == 0 {
        return None;
    }
    Some(line.trim_end_matches(['\r', '\n']).to_string())
}

/// y/N prompt. Default is `default`; empty input or non-TTY returns it.
pub fn confirm(prompt: &str, default: bool) -> bool {
    let suffix = if default { "[Y/n]" } else { "[y/N]" };
    let Some(line) = read_line(&format!("{prompt} {suffix} ")) else {
        return default;
    };
    match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        "" => default,
        _ => default,
    }
}
