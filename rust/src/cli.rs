//! Command-line dispatcher.
//!
//! Mirrors `src/index.ts` in the TypeScript implementation. Phase 0 only
//! parses arguments into a `Command` enum; the actual screen runners are
//! wired up in later phases. Keeping the parser pure (no I/O) makes it
//! trivial to unit-test against the TS `cli.test.ts` cases.

pub const USAGE: &str = "\
Usage: nitro <command> [subcommand]

Commands:
  \"<request>\"                    Execute request and exit (2+ words)
  interactive, i [<request>]     Start interactive session
  continue, c <request>          Continue last conversation
  resume, r [<request>]          Resume last conversation interactively
  strict, s [<request>]          Run in strict mode (always confirm commands)
  help                           Print this help message
  settings                       Configure Nitro settings
  provider                       Manage AI providers

Provider Subcommands:
  provider add                   Add a new provider
  provider list                  List all providers
  provider edit                  Edit a provider
  provider remove                Remove a provider
  provider default               Set default provider";

/// Parsed top-level command. `Continue` and `Resume` carry their
/// `requires_filename` semantics so the dispatcher can produce the same error
/// messages the TS version does even before the conversation store is wired.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Settings,
    Provider { args: Vec<String> },
    Interactive { request: String },
    Continue { request: String },
    Resume { request: String },
    Strict { request: String },
    OneShot { request: String },
    Unknown { command: String },
}

/// Errors that can be produced *purely from argument parsing*. I/O-driven
/// errors (no last conversation, etc.) are produced by the dispatcher.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("Error: continue requires a request argument.\nUse resume to interactively resume.")]
    ContinueWithoutRequest,
}

/// Parse argv (without the program name).
///
/// The TS implementation treats any single positional argument that contains
/// a space as a one-shot request; this mirrors that behaviour exactly so the
/// CLI surface remains stable across the port.
pub fn parse(args: &[String]) -> Result<Command, ParseError> {
    if args.is_empty() {
        return Ok(Command::Help);
    }

    let command = args[0].as_str();
    let rest = &args[1..];

    match command {
        "help" => Ok(Command::Help),
        "settings" => Ok(Command::Settings),
        "provider" => Ok(Command::Provider {
            args: rest.to_vec(),
        }),
        "interactive" | "i" => Ok(Command::Interactive {
            request: rest.first().cloned().unwrap_or_default(),
        }),
        "continue" | "c" => match rest.first() {
            Some(req) => Ok(Command::Continue {
                request: req.clone(),
            }),
            None => Err(ParseError::ContinueWithoutRequest),
        },
        "resume" | "r" => Ok(Command::Resume {
            request: rest.first().cloned().unwrap_or_default(),
        }),
        "strict" | "s" => Ok(Command::Strict {
            request: rest.first().cloned().unwrap_or_default(),
        }),
        other => {
            if other.contains(' ') {
                Ok(Command::OneShot {
                    request: other.to_string(),
                })
            } else {
                Ok(Command::Unknown {
                    command: other.to_string(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn no_args_is_help() {
        assert_eq!(parse(&[]).unwrap(), Command::Help);
    }

    #[test]
    fn help_command() {
        assert_eq!(parse(&args(&["help"])).unwrap(), Command::Help);
    }

    #[test]
    fn settings_command() {
        assert_eq!(parse(&args(&["settings"])).unwrap(), Command::Settings);
    }

    #[test]
    fn provider_passes_subcommand_args() {
        let parsed = parse(&args(&["provider", "add"])).unwrap();
        assert_eq!(
            parsed,
            Command::Provider {
                args: vec!["add".to_string()]
            }
        );
    }

    #[test]
    fn interactive_with_and_without_request() {
        assert_eq!(
            parse(&args(&["interactive"])).unwrap(),
            Command::Interactive {
                request: String::new(),
            }
        );
        assert_eq!(
            parse(&args(&["i", "hello world"])).unwrap(),
            Command::Interactive {
                request: "hello world".to_string(),
            }
        );
    }

    #[test]
    fn continue_requires_request() {
        assert_eq!(
            parse(&args(&["continue"])),
            Err(ParseError::ContinueWithoutRequest)
        );
        assert_eq!(
            parse(&args(&["c"])),
            Err(ParseError::ContinueWithoutRequest)
        );
        assert_eq!(
            parse(&args(&["c", "follow up"])).unwrap(),
            Command::Continue {
                request: "follow up".to_string(),
            }
        );
    }

    #[test]
    fn resume_alias_with_and_without_request() {
        assert_eq!(
            parse(&args(&["resume"])).unwrap(),
            Command::Resume {
                request: String::new(),
            }
        );
        assert_eq!(
            parse(&args(&["r", "alias test"])).unwrap(),
            Command::Resume {
                request: "alias test".to_string(),
            }
        );
    }

    #[test]
    fn strict_alias_works() {
        assert_eq!(
            parse(&args(&["s"])).unwrap(),
            Command::Strict {
                request: String::new(),
            }
        );
        assert_eq!(
            parse(&args(&["strict", "audit"])).unwrap(),
            Command::Strict {
                request: "audit".to_string(),
            }
        );
    }

    #[test]
    fn multi_word_default_is_oneshot() {
        assert_eq!(
            parse(&args(&["hello world"])).unwrap(),
            Command::OneShot {
                request: "hello world".to_string(),
            }
        );
    }

    #[test]
    fn single_word_unknown() {
        assert_eq!(
            parse(&args(&["unknown"])).unwrap(),
            Command::Unknown {
                command: "unknown".to_string(),
            }
        );
    }

    #[test]
    fn usage_mentions_nitro() {
        // Cheap parity guard: matches `cli.test.ts`'s `expect(...).toContain("Usage: nitro")`.
        assert!(USAGE.contains("Usage: nitro"));
    }
}
