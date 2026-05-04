//! Nitro core library.
//!
//! The crate is intentionally split into modules that mirror the TypeScript
//! source tree (`src/logic`, `src/tools`, `src/screens`, …) so the port can
//! proceed phase-by-phase without losing track of what has and hasn't been
//! migrated. See `PARITY.md` at the crate root for the running checklist.

pub mod app;
pub mod cli;
pub mod logic;
pub mod tools;
