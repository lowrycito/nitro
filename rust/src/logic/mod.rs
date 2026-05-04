//! Pure-data layer for Nitro.
//!
//! Each submodule mirrors the corresponding `src/logic/*.ts` file. Functions
//! take an explicit `data_dir: &Path` so tests can isolate against
//! `tempfile::TempDir`; the production binary calls [`default_app_dir`] once
//! and threads it through.

pub mod config;
pub mod conversation;
pub mod defaults;
pub mod eula;
pub mod llm;
pub mod provider;
pub mod settings;

pub use config::{default_app_dir, ensure_app_data_dir};
