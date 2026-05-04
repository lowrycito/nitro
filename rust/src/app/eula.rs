//! EULA gate. Routes to the ratatui screen when stdin/stdout are a TTY,
//! and to the line-mode prompt for piped/non-interactive sessions.
//! Either path persists `agreedToEula` to the same `~/.nitro/settings.json`
//! used by the TS app.

use std::io::IsTerminal;
use std::path::Path;

use super::{console, prompts};
use crate::logic::eula::{EULA_TEXT, EULA_VERSION};
use crate::logic::settings::{is_eula_agreed, save_settings, Settings};
use crate::screens::{run_eula_screen, EulaOutcome};

pub enum Outcome {
    Accepted,
    Declined,
    Error(String),
}

pub fn is_agreed(settings: &Settings) -> bool {
    is_eula_agreed(settings)
}

pub fn gate(data_dir: &Path, settings: &Settings) -> Outcome {
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let outcome = if interactive {
        match run_eula_screen() {
            Ok(EulaOutcome::Accepted) => Outcome::Accepted,
            Ok(EulaOutcome::Declined) => Outcome::Declined,
            Err(e) => Outcome::Error(format!("Error: failed to run EULA screen: {e}")),
        }
    } else {
        line_mode(settings).0
    };

    if let Outcome::Accepted = outcome {
        if let Err(e) = save_settings(
            data_dir,
            &Settings {
                agreed_to_eula: Some(EULA_VERSION),
                ..settings.clone()
            },
        ) {
            return Outcome::Error(format!("Error: failed to record EULA acceptance: {e}"));
        }
    }
    outcome
}

/// Line-mode fallback used when stdout/stdin aren't a TTY (CI, pipes).
fn line_mode(_settings: &Settings) -> (Outcome, ()) {
    console::info("");
    console::info(EULA_TEXT);
    console::info("");
    let agreed = prompts::confirm("Do you agree to these terms?", false);
    if !agreed {
        console::error("EULA declined. Exiting.");
        return (Outcome::Declined, ());
    }
    (Outcome::Accepted, ())
}
