//! ratatui-based screens.
//!
//! Each `*_screen.rs` exports a single `run_*_screen` entry point. Phase 5
//! wires up `EulaScreen` and `SettingsScreen`; later phases add provider
//! management and the chat UI on top of the same scaffolding.

pub mod app_shell;
pub mod eula_screen;
pub mod settings_screen;
pub mod theme;

pub use eula_screen::{run_eula_screen, EulaOutcome};
pub use settings_screen::run_settings_screen;
