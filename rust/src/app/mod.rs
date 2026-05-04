//! Top-level orchestration: maps a parsed [`crate::cli::Command`] to an
//! actual program run. Phase 4 wires up the headless paths
//! (`one-shot`, `continue`, `strict`, `provider list`); the interactive and
//! provider-management screens are wired up in Phases 5+ once ratatui exists.

pub mod chat;
pub mod console;
pub mod eula;
pub mod prompts;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::cli::Command;
use crate::logic::settings::load_settings;

/// Entry point used by `main.rs` and integration tests. The `data_dir` is
/// passed in (rather than computed) to make integration testing trivial.
pub async fn run(command: Command, data_dir: PathBuf) -> ExitCode {
    let settings = match load_settings(&data_dir) {
        Ok(s) => s,
        Err(e) => {
            console::error(&format!("Error: failed to load settings: {e}"));
            return ExitCode::from(1);
        }
    };

    if !eula::is_agreed(&settings) {
        match eula::gate(&data_dir, &settings) {
            eula::Outcome::Accepted => {}
            eula::Outcome::Declined => return ExitCode::from(1),
            eula::Outcome::Error(msg) => {
                console::error(&msg);
                return ExitCode::from(1);
            }
        }
    }

    match command {
        Command::Help => {
            println!("{}", crate::cli::USAGE);
            ExitCode::SUCCESS
        }
        Command::Unknown { command } => {
            console::error(&format!("Unknown subcommand: {command}"));
            println!("{}", crate::cli::USAGE);
            ExitCode::SUCCESS
        }
        Command::Settings => match crate::screens::run_settings_screen(data_dir.clone()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                console::error(&format!("Error: settings screen failed: {e}"));
                ExitCode::from(1)
            }
        },
        Command::Provider { args } => provider_router(&args, &data_dir).await,
        Command::OneShot { request } => chat::run_one_shot(&data_dir, &request, false, None).await,
        Command::Continue { request } => {
            let prev = match crate::logic::conversation::last_conversation_filename(&data_dir) {
                Some(name) => name,
                None => {
                    console::error("Error: No conversation to continue.");
                    return ExitCode::from(1);
                }
            };
            chat::run_one_shot(&data_dir, &request, false, Some(prev)).await
        }
        Command::Strict { request } => chat::run_one_shot(&data_dir, &request, true, None).await,
        Command::Interactive { .. } | Command::Resume { .. } => {
            console::error(
                "interactive/resume require the ratatui chat screen (Phase 7). Use the TS binary, \
                 or run a one-shot request: nitro \"<request>\"",
            );
            ExitCode::from(2)
        }
    }
}

async fn provider_router(args: &[String], data_dir: &Path) -> ExitCode {
    let dir = data_dir.to_path_buf();
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdout());

    let result: std::io::Result<()> = match args.first().map(String::as_str) {
        Some("list") if !interactive => {
            // Headless / CI: print plain text to stdout, no TUI.
            return print_provider_list(data_dir);
        }
        Some("list") => crate::screens::run_provider_list_screen(dir),
        Some("add") => crate::screens::run_provider_add_screen(dir),
        Some("edit") => crate::screens::run_provider_edit_screen(dir),
        Some("remove") => crate::screens::run_provider_remove_screen(dir),
        Some("default") => crate::screens::run_provider_default_screen(dir),
        Some(other) => {
            console::error(&format!(
                "Unknown provider subcommand: {other}. Valid: list, add, edit, remove, default"
            ));
            return ExitCode::from(2);
        }
        None => {
            println!("Provider subcommands: list, add, edit, remove, default");
            return ExitCode::SUCCESS;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            console::error(&format!("Error: provider screen failed: {e}"));
            ExitCode::from(1)
        }
    }
}

fn print_provider_list(data_dir: &Path) -> ExitCode {
    let providers = match crate::logic::provider::list_providers(data_dir) {
        Ok(p) => p,
        Err(e) => {
            console::error(&format!("Error: failed to read providers: {e}"));
            return ExitCode::from(1);
        }
    };
    let default = crate::logic::provider::get_default_provider(data_dir)
        .ok()
        .flatten();
    if providers.is_empty() {
        println!("No providers configured. Run `nitro provider add` to set one up.");
    } else {
        for name in providers {
            let marker = match &default {
                Some(d) if d.name == name => " (default)",
                _ => "",
            };
            println!("- {name}{marker}");
        }
    }
    ExitCode::SUCCESS
}
