//! Common stream event type used by every provider adapter.
//!
//! The chat loop consumes a `Stream<Item = Result<StreamEvent, StreamError>>`
//! and accumulates the deltas. Each event is independent so partial
//! progress is preserved on errors.

use std::pin::Pin;

use futures::Stream;
use serde::{Deserialize, Serialize};

use super::messages::ToolCall;

/// Token-usage tally as reported by the provider on the final stream event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default)]
    pub cached_input_tokens: u32,
    #[serde(default)]
    pub reasoning_tokens: u32,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cached_input_tokens = self
            .cached_input_tokens
            .saturating_add(other.cached_input_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
    }
}

/// One slice of progress from the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
    /// A complete tool call (some providers emit it incrementally; the
    /// adapter buffers and emits this once the call is finalised).
    ToolCall(ToolCall),
    /// Final usage report.
    Usage(Usage),
    /// Stream finished cleanly.
    Done,
}

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("HTTP error: {status}: {body}")]
    Http { status: u16, body: String },
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("parse: {0}")]
    Parse(String),
    #[error("upstream returned an error: {0}")]
    Upstream(String),
}

pub type StreamItem = Result<StreamEvent, StreamError>;
pub type StreamResult = Pin<Box<dyn Stream<Item = StreamItem> + Send + 'static>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_addition_accumulates() {
        let mut u = Usage::default();
        u.add(&Usage {
            input_tokens: 5,
            output_tokens: 10,
            cached_input_tokens: 1,
            reasoning_tokens: 2,
        });
        u.add(&Usage {
            input_tokens: 3,
            output_tokens: 4,
            cached_input_tokens: 0,
            reasoning_tokens: 0,
        });
        assert_eq!(u.input_tokens, 8);
        assert_eq!(u.output_tokens, 14);
        assert_eq!(u.cached_input_tokens, 1);
        assert_eq!(u.reasoning_tokens, 2);
    }
}
