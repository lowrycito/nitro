use std::process::ExitCode;

use nitro::app;
use nitro::cli::{parse, ParseError};
use nitro::logic::config::default_app_dir;
use nitro::tools::bash::enable_execution;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = match parse(&args) {
        Ok(cmd) => cmd,
        Err(ParseError::ContinueWithoutRequest) => {
            eprintln!("Error: continue requires a request argument.");
            eprintln!("Use resume to interactively resume.");
            return ExitCode::from(1);
        }
    };

    let data_dir = match default_app_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: could not determine data directory: {e}");
            return ExitCode::from(1);
        }
    };

    // SAFETY: This is the only call site of `enable_execution`. Tests must
    // never invoke this function via the binary entry point — they go
    // through library APIs that leave the guard untouched.
    enable_execution();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Error: failed to start async runtime: {e}");
            return ExitCode::from(1);
        }
    };
    runtime.block_on(app::run(command, data_dir))
}
