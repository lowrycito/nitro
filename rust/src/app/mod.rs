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
        match eula::prompt_and_record(&data_dir, &settings) {
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
        Command::Settings => {
            console::error(
                "settings: not yet ported to Rust headless mode; use the TS binary for the menu, \
                 or edit ~/.nitro/settings.json directly. Ratatui menu lands in Phase 5.",
            );
            ExitCode::from(2)
        }
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
    match args.first().map(String::as_str) {
        Some("list") => {
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
                println!("No providers configured. Use the TS binary's `nitro provider add` (Rust port: Phase 6).");
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
        Some(other) => {
            console::error(&format!(
                "provider {other}: not yet ported to Rust headless mode (Phase 6)."
            ));
            ExitCode::from(2)
        }
        None => {
            println!("Subcommands: list");
            ExitCode::SUCCESS
        }
    }
}
