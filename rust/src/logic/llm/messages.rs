//! Internal `Message` shape used between the chat loop and provider adapters.
//!
//! Keeps the same content-block taxonomy as the AI SDK's `ModelMessage`
//! (text / reasoning / tool-call / tool-result) so we can serialise to JSON
//! that the TS app's conversation persistence already understands. This
//! makes `~/.nitro/chats/*.json` byte-compatible across both binaries.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single message in a turn. `system` is excluded — it's threaded as the
/// `system` field on each request, exactly like the TS side does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    User { content: MessageContent<UserPart> },
    Assistant { content: AssistantContent },
    Tool { content: Vec<ToolResult> },
}

/// Either a bare string or a list of typed parts. The AI SDK wire format
/// uses both shapes so we accept both on read and produce parts on write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent<P> {
    Text(String),
    Parts(Vec<P>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UserPart {
    Text { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AssistantContent {
    Text(String),
    Parts(Vec<AssistantPart>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AssistantPart {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    #[serde(rename = "tool-call")]
    ToolCall {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        input: Value,
    },
}

/// Streamed tool call. Identical wire format to the assistant content part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(rename = "toolCallId")]
    pub tool_call_id: String,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    #[serde(rename = "type", default = "tool_result_type")]
    pub kind: String, // always "tool-result" — kept explicit so the TS reader is happy.
    #[serde(rename = "toolCallId")]
    pub tool_call_id: String,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    pub output: ToolResultContent,
}

fn tool_result_type() -> String {
    "tool-result".to_string()
}

/// Output payload returned from a tool. Matches `ToolResultOutput` from
/// `@ai-sdk/provider-utils`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolResultContent {
    Json {
        value: Value,
    },
    #[serde(rename = "error-text")]
    ErrorText {
        value: String,
    },
    #[serde(rename = "error-json")]
    ErrorJson {
        value: Value,
    },
}

impl Message {
    /// Convenience constructor for a user-text message.
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::User {
            content: MessageContent::Text(text.into()),
        }
    }

    /// Build an assistant message from streaming-collected parts.
    pub fn assistant_parts(parts: Vec<AssistantPart>) -> Self {
        Self::Assistant {
            content: AssistantContent::Parts(parts),
        }
    }

    /// Build a tool message from a list of results.
    pub fn tool_results(results: Vec<ToolResult>) -> Self {
        Self::Tool { content: results }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn user_text_round_trips_as_string() {
        let m = Message::user_text("hi");
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "user");
        assert_eq!(v["content"], "hi");
        let back: Message = serde_json::from_value(v).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn assistant_parts_have_kebab_type_tags() {
        let m = Message::assistant_parts(vec![
            AssistantPart::Reasoning {
                text: "think".into(),
            },
            AssistantPart::Text {
                text: "answer".into(),
            },
            AssistantPart::ToolCall {
                tool_call_id: "t1".into(),
                tool_name: "Bash".into(),
                input: json!({ "command": "ls" }),
            },
        ]);
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["content"][0]["type"], "reasoning");
        assert_eq!(v["content"][1]["type"], "text");
        assert_eq!(v["content"][2]["type"], "tool-call");
        assert_eq!(v["content"][2]["toolCallId"], "t1");
        assert_eq!(v["content"][2]["toolName"], "Bash");
    }

    #[test]
    fn tool_result_round_trip() {
        let m = Message::tool_results(vec![ToolResult {
            kind: "tool-result".into(),
            tool_call_id: "t1".into(),
            tool_name: "Bash".into(),
            output: ToolResultContent::Json {
                value: json!({ "approved": true }),
            },
        }]);
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "tool");
        assert_eq!(v["content"][0]["toolCallId"], "t1");
        assert_eq!(v["content"][0]["output"]["type"], "json");
        let back: Message = serde_json::from_value(v).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn parses_ts_fixture_with_string_user_content() {
        let raw = json!({ "role": "user", "content": "hello world" });
        let m: Message = serde_json::from_value(raw).unwrap();
        assert_eq!(m, Message::user_text("hello world"));
    }
}
