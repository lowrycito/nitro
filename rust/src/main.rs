use std::io::Write;
use std::process::ExitCode;

use nitro::cli::{parse, Command, ParseError, USAGE};

fn print_usage() {
    println!("{}", USAGE);
}

fn print_error(message: &str) {
    // Mirror `outputError` from src/utils.ts: red foreground, full-line reset.
    let red = "\x1b[38;2;230;126;128m";
    let reset = "\x1b[0m";
    let _ = writeln!(std::io::stdout(), "{red}{message}{reset}");
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let command = match parse(&args) {
        Ok(cmd) => cmd,
        Err(ParseError::ContinueWithoutRequest) => {
            print_error("Error: continue requires a request argument.");
            print_error("Use resume to interactively resume.");
            return ExitCode::from(1);
        }
    };

    match command {
        Command::Help => {
            print_usage();
            ExitCode::SUCCESS
        }
        Command::Unknown { command } => {
            print_error(&format!("Unknown subcommand: {command}"));
            print_usage();
            ExitCode::SUCCESS
        }
        // Phase 0 stubs — real wiring lands in later phases. Stubs are noisy
        // (not silent) so anyone running `nitro` from the Rust crate today
        // sees exactly which feature has yet to be ported.
        Command::Settings => {
            print_error("settings: not yet implemented in the Rust port (see PARITY.md)");
            ExitCode::from(2)
        }
        Command::Provider { .. } => {
            print_error("provider: not yet implemented in the Rust port (see PARITY.md)");
            ExitCode::from(2)
        }
        Command::Interactive { .. }
        | Command::Continue { .. }
        | Command::Resume { .. }
        | Command::Strict { .. }
        | Command::OneShot { .. } => {
            print_error("chat: not yet implemented in the Rust port (see PARITY.md)");
            ExitCode::from(2)
        }
    }
}
