//! EULA gate. Phase 4 implementation prompts on the terminal; the same
//! `agreedToEula` field is used as in the TypeScript app so users only ever
//! agree once across either binary.

use std::path::Path;

use super::{console, prompts};
use crate::logic::eula::{EULA_TEXT, EULA_VERSION};
use crate::logic::settings::{is_eula_agreed, save_settings, Settings};

pub enum Outcome {
    Accepted,
    Declined,
    Error(String),
}

pub fn is_agreed(settings: &Settings) -> bool {
    is_eula_agreed(settings)
}

/// Display the EULA, ask y/N, persist on accept. Mirrors `EulaScreen` from
/// `src/screens/EulaScreen.tsx` minus the ratatui chrome.
pub fn prompt_and_record(data_dir: &Path, settings: &Settings) -> Outcome {
    console::info("");
    console::info(EULA_TEXT);
    console::info("");
    let agreed = prompts::confirm("Do you agree to these terms?", false);
    if !agreed {
        console::error("EULA declined. Exiting.");
        return Outcome::Declined;
    }
    let updated = Settings {
        agreed_to_eula: Some(EULA_VERSION),
        ..settings.clone()
    };
    if let Err(e) = save_settings(data_dir, &updated) {
        return Outcome::Error(format!("Error: failed to record EULA acceptance: {e}"));
    }
    Outcome::Accepted
}
