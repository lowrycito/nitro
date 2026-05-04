//! Model-facing tools.
//!
//! Each tool mirrors `src/tools/<name>.tsx`. The Rust port keeps execution
//! and rendering separate: `bash` and `ask` here own command schemas and
//! execution; rendering for ratatui lands in Phase 5+.

pub mod ask;
pub mod bash;

pub use ask::{AskTool, AskToolOutput, Question, QuestionChoice, QuestionResponse};
pub use bash::{BashApproval, BashModelInput, BashTool, BashToolOutput, BehaviorTag, RiskLevel};
