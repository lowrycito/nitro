//! ratatui-based screens.
//!
//! Each `*_screen.rs` exports a single `run_*_screen` entry point. Phase 5
//! wires up `EulaScreen` and `SettingsScreen`; later phases add provider
//! management and the chat UI on top of the same scaffolding.

pub mod app_shell;
pub mod eula_screen;
pub mod provider_screens;
pub mod settings_screen;
pub mod theme;

pub use eula_screen::{run_eula_screen, EulaOutcome};
pub use provider_screens::{
    run_provider_add_screen, run_provider_default_screen, run_provider_edit_screen,
    run_provider_list_screen, run_provider_remove_screen,
};
pub use settings_screen::run_settings_screen;
