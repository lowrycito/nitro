//! Helpers for accumulating streamed deltas into final messages.
//!
//! Mirrors the `appendDelta` reducer + `result.response.messages` collection
//! in `useChatState.ts`. By keeping it provider-agnostic, every adapter can
//! emit deltas naively while the chat loop builds a tidy assistant message.

use super::messages::{AssistantPart, Message, ToolCall};
use super::stream::Usage;

/// Builds an assistant message from a sequence of [`StreamEvent`] deltas.
#[derive(Debug, Default)]
pub struct AssistantBuilder {
    parts: Vec<AssistantPart>,
    pub usage: Usage,
    pub tool_calls: Vec<ToolCall>,
}

impl AssistantBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_text(&mut self, delta: &str) {
        match self.parts.last_mut() {
            Some(AssistantPart::Text { text }) => text.push_str(delta),
            _ => self.parts.push(AssistantPart::Text {
                text: delta.to_string(),
            }),
        }
    }

    pub fn push_reasoning(&mut self, delta: &str) {
        match self.parts.last_mut() {
            Some(AssistantPart::Reasoning { text }) => text.push_str(delta),
            _ => self.parts.push(AssistantPart::Reasoning {
                text: delta.to_string(),
            }),
        }
    }

    pub fn push_tool_call(&mut self, call: ToolCall) {
        self.parts.push(AssistantPart::ToolCall {
            tool_call_id: call.tool_call_id.clone(),
            tool_name: call.tool_name.clone(),
            input: call.input.clone(),
        });
        self.tool_calls.push(call);
    }

    /// Finalise the assistant message. Empty turns produce an empty parts
    /// list — caller decides whether to include or skip them.
    pub fn finish(self) -> (Message, Vec<ToolCall>, Usage) {
        let msg = Message::assistant_parts(self.parts);
        (msg, self.tool_calls, self.usage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_consecutive_text_deltas() {
        let mut b = AssistantBuilder::new();
        b.push_text("Hello, ");
        b.push_text("world.");
        let (m, _, _) = b.finish();
        match m {
            Message::Assistant {
                content: super::super::messages::AssistantContent::Parts(parts),
            } => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    AssistantPart::Text { text } => assert_eq!(text, "Hello, world."),
                    _ => panic!("expected text"),
                }
            }
            _ => panic!("expected assistant"),
        }
    }

    #[test]
    fn separate_reasoning_from_text() {
        let mut b = AssistantBuilder::new();
        b.push_reasoning("think");
        b.push_text("answer");
        b.push_reasoning("more");
        let (m, _, _) = b.finish();
        match m {
            Message::Assistant {
                content: super::super::messages::AssistantContent::Parts(parts),
            } => {
                assert_eq!(parts.len(), 3);
                assert!(matches!(&parts[0], AssistantPart::Reasoning { .. }));
                assert!(matches!(&parts[1], AssistantPart::Text { .. }));
                assert!(matches!(&parts[2], AssistantPart::Reasoning { .. }));
            }
            _ => panic!("expected assistant"),
        }
    }

    #[test]
    fn captures_tool_calls() {
        let mut b = AssistantBuilder::new();
        b.push_text("running...");
        b.push_tool_call(ToolCall {
            tool_call_id: "t1".into(),
            tool_name: "Bash".into(),
            input: json!({ "command": "ls" }),
        });
        let (_, calls, _) = b.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "Bash");
    }
}
