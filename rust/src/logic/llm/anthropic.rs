//! Anthropic Messages API adapter (`/v1/messages`).
//!
//! The TS implementation enables prompt caching by tagging the system
//! message and the most recent user message with `cache_control:
//! ephemeral`. We mirror that exactly so the cache-hit rate stays the same
//! across binaries.

use async_trait::async_trait;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

use super::messages::{
    AssistantContent, AssistantPart, Message, MessageContent, ToolCall, ToolResultContent, UserPart,
};
use super::sse::into_sse_events;
use super::stream::{StreamError, StreamEvent, StreamResult, Usage};
use super::tool_def::ToolDef;
use super::{http_client, CompletionProvider, GenerationOptions};

use crate::logic::provider::NamedProvider;
use crate::logic::settings::ReasoningEffort;

#[derive(Debug, Default)]
pub struct AnthropicClient;

impl AnthropicClient {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CompletionProvider for AnthropicClient {
    async fn stream_completion(
        &self,
        provider: &NamedProvider,
        messages: &[Message],
        system_prompt: &str,
        tools: &[ToolDef],
        options: &GenerationOptions,
    ) -> StreamResult {
        let url = format!("{}/messages", provider.info.base_url.trim_end_matches('/'));
        let body = build_request_body(messages, system_prompt, tools, provider, options);

        let resp = match http_client()
            .post(&url)
            .header("x-api-key", &provider.info.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "prompt-caching-2024-07-31")
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return error_stream(StreamError::Transport(e)),
        };

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return error_stream(StreamError::Http {
                status: status.as_u16(),
                body: body_text,
            });
        }

        Box::pin(async_stream::try_stream! {
            let byte_stream = resp.bytes_stream();
            let mut sse = Box::pin(into_sse_events(byte_stream));

            // Tool calls arrive as a content_block_start (tool_use) followed
            // by partial_json deltas; we accumulate per content-block index
            // and emit ToolCall on content_block_stop.
            let mut blocks: std::collections::HashMap<u32, BlockBuf> =
                std::collections::HashMap::new();
            let mut total_usage = Usage::default();

            while let Some(item) = sse.next().await {
                let ev = item?;
                let ty = ev.event.as_deref().unwrap_or("");
                let data: Value = match serde_json::from_str(&ev.data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match ty {
                    "message_start" => {
                        if let Some(u) = data.get("message").and_then(|m| m.get("usage")) {
                            total_usage.input_tokens = u
                                .get("input_tokens").and_then(|v| v.as_u64())
                                .unwrap_or(0) as u32;
                            total_usage.cached_input_tokens = u
                                .get("cache_read_input_tokens").and_then(|v| v.as_u64())
                                .unwrap_or(0) as u32;
                        }
                    }
                    "content_block_start" => {
                        let idx = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        if let Some(block) = data.get("content_block") {
                            let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match block_type {
                                "text" => {
                                    blocks.insert(idx, BlockBuf::Text);
                                }
                                "thinking" => {
                                    blocks.insert(idx, BlockBuf::Thinking);
                                }
                                "tool_use" => {
                                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                    blocks.insert(idx, BlockBuf::ToolUse {
                                        id,
                                        name,
                                        partial_json: String::new(),
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    "content_block_delta" => {
                        let idx = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let delta = data.get("delta");
                        let delta_type = delta
                            .and_then(|d| d.get("type")).and_then(|t| t.as_str())
                            .unwrap_or("");
                        match delta_type {
                            "text_delta" => {
                                let text = delta
                                    .and_then(|d| d.get("text")).and_then(|t| t.as_str())
                                    .unwrap_or("");
                                if !text.is_empty() {
                                    yield StreamEvent::TextDelta(text.to_string());
                                }
                            }
                            "thinking_delta" => {
                                let text = delta
                                    .and_then(|d| d.get("thinking")).and_then(|t| t.as_str())
                                    .unwrap_or("");
                                if !text.is_empty() {
                                    yield StreamEvent::ReasoningDelta(text.to_string());
                                }
                            }
                            "input_json_delta" => {
                                if let Some(BlockBuf::ToolUse { partial_json, .. }) = blocks.get_mut(&idx) {
                                    if let Some(piece) = delta
                                        .and_then(|d| d.get("partial_json")).and_then(|t| t.as_str())
                                    {
                                        partial_json.push_str(piece);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    "content_block_stop" => {
                        let idx = data.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        if let Some(BlockBuf::ToolUse { id, name, partial_json }) = blocks.remove(&idx) {
                            let input: Value = if partial_json.trim().is_empty() {
                                json!({})
                            } else {
                                serde_json::from_str(&partial_json).unwrap_or(json!({}))
                            };
                            yield StreamEvent::ToolCall(ToolCall {
                                tool_call_id: id,
                                tool_name: name,
                                input,
                            });
                        }
                    }
                    "message_delta" => {
                        if let Some(u) = data.get("usage") {
                            total_usage.output_tokens = u
                                .get("output_tokens").and_then(|v| v.as_u64())
                                .unwrap_or(total_usage.output_tokens as u64) as u32;
                        }
                    }
                    "message_stop" => {
                        // Drain anything still buffered defensively.
                        let mut remaining: Vec<_> = blocks.drain().collect();
                        remaining.sort_by_key(|(i, _)| *i);
                        for (_, block) in remaining {
                            if let BlockBuf::ToolUse { id, name, partial_json } = block {
                                let input: Value = serde_json::from_str(&partial_json).unwrap_or(json!({}));
                                yield StreamEvent::ToolCall(ToolCall {
                                    tool_call_id: id,
                                    tool_name: name,
                                    input,
                                });
                            }
                        }
                        yield StreamEvent::Usage(total_usage);
                    }
                    "error" => {
                        let msg = data
                            .get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str())
                            .unwrap_or("upstream error").to_string();
                        Err(StreamError::Upstream(msg))?;
                    }
                    _ => {}
                }
            }
            yield StreamEvent::Done;
        })
    }
}

fn error_stream(e: StreamError) -> StreamResult {
    Box::pin(async_stream::stream! {
        yield Err(e);
    })
}

enum BlockBuf {
    Text,
    Thinking,
    ToolUse {
        id: String,
        name: String,
        partial_json: String,
    },
}

fn reasoning_effort_to_anthropic(eff: ReasoningEffort) -> &'static str {
    match eff {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Med => "medium",
        ReasoningEffort::High => "high",
    }
}

fn build_request_body(
    messages: &[Message],
    system_prompt: &str,
    tools: &[ToolDef],
    provider: &NamedProvider,
    options: &GenerationOptions,
) -> Value {
    // Prompt caching: tag the system block + the very last message.
    let system_block = json!([{
        "type": "text",
        "text": system_prompt,
        "cache_control": { "type": "ephemeral" },
    }]);

    let last = messages.len().saturating_sub(1);
    let wire_messages: Vec<Value> = messages
        .iter()
        .enumerate()
        .map(|(i, m)| message_to_anthropic(m, i == last))
        .collect();

    let mut body = json!({
        "model": provider.info.model,
        "messages": wire_messages,
        "system": system_block,
        "stream": true,
    });
    body["max_tokens"] = json!(options.max_output_tokens.unwrap_or(16_000));
    if let Some(eff) = options.reasoning_effort {
        body["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": match eff {
                ReasoningEffort::Low => 1024,
                ReasoningEffort::Med => 4096,
                ReasoningEffort::High => 16384,
            }
        });
        // Some Anthropic-compatible gateways (e.g. z.ai) accept an `effort`
        // hint alongside `thinking`; sending it as well is a no-op upstream.
        body["effort"] = json!(reasoning_effort_to_anthropic(eff));
    }

    if !tools.is_empty() {
        let tools_json: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            })
            .collect();
        body["tools"] = json!(tools_json);
    }

    body
}

fn message_to_anthropic(m: &Message, attach_cache: bool) -> Value {
    match m {
        Message::User { content } => {
            let parts = match content {
                MessageContent::Text(s) => {
                    let mut block = json!({ "type": "text", "text": s });
                    if attach_cache {
                        block["cache_control"] = json!({ "type": "ephemeral" });
                    }
                    vec![block]
                }
                MessageContent::Parts(parts) => parts
                    .iter()
                    .map(|p| match p {
                        UserPart::Text { text } => json!({ "type": "text", "text": text }),
                    })
                    .collect(),
            };
            json!({ "role": "user", "content": parts })
        }
        Message::Assistant { content } => {
            let parts = match content {
                AssistantContent::Text(s) => vec![AssistantPart::Text { text: s.clone() }],
                AssistantContent::Parts(p) => p.clone(),
            };
            let blocks: Vec<Value> = parts
                .into_iter()
                .map(|p| match p {
                    AssistantPart::Text { text } => json!({ "type": "text", "text": text }),
                    // Anthropic doesn't accept `thinking` blocks in
                    // *replayed* assistant messages — they're streamed only.
                    // Drop them on history.
                    AssistantPart::Reasoning { .. } => {
                        json!({ "type": "text", "text": "" })
                    }
                    AssistantPart::ToolCall {
                        tool_call_id,
                        tool_name,
                        input,
                    } => json!({
                        "type": "tool_use",
                        "id": tool_call_id,
                        "name": tool_name,
                        "input": input,
                    }),
                })
                .filter(|b| {
                    // Strip the empty-text placeholders introduced for reasoning.
                    !(b["type"] == "text" && b["text"].as_str().is_some_and(str::is_empty))
                })
                .collect();
            json!({ "role": "assistant", "content": blocks })
        }
        Message::Tool { content } => {
            let blocks: Vec<Value> = content
                .iter()
                .map(|r| {
                    let body = match &r.output {
                        ToolResultContent::Json { value } => {
                            serde_json::to_string(value).unwrap_or_default()
                        }
                        ToolResultContent::ErrorText { value } => value.clone(),
                        ToolResultContent::ErrorJson { value } => {
                            serde_json::to_string(value).unwrap_or_default()
                        }
                    };
                    let is_err = matches!(
                        r.output,
                        ToolResultContent::ErrorText { .. } | ToolResultContent::ErrorJson { .. }
                    );
                    let mut block = json!({
                        "type": "tool_result",
                        "tool_use_id": r.tool_call_id,
                        "content": body,
                    });
                    if is_err {
                        block["is_error"] = json!(true);
                    }
                    block
                })
                .collect();
            let mut out = json!({ "role": "user", "content": blocks });
            // The "last message" caching tag goes on the trailing block.
            if attach_cache {
                if let Some(arr) = out["content"].as_array_mut() {
                    if let Some(last) = arr.last_mut() {
                        last["cache_control"] = json!({ "type": "ephemeral" });
                    }
                }
            }
            out
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct AnthropicError {
    #[serde(default)]
    message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::provider::{ApiType, ProviderInfo};

    fn np() -> NamedProvider {
        NamedProvider {
            name: "anthropic".into(),
            info: ProviderInfo {
                base_url: "https://api.anthropic.com/v1".into(),
                api_key: "k".into(),
                model: "claude-test".into(),
                api_type: ApiType::Anthropic,
            },
        }
    }

    #[test]
    fn body_includes_system_with_cache_control() {
        let body = build_request_body(
            &[Message::user_text("hi")],
            "the system",
            &[],
            &np(),
            &GenerationOptions::default(),
        );
        assert_eq!(body["system"][0]["text"], "the system");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        // Last user message also gets cache_control.
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["text"], "hi");
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
    }

    #[test]
    fn only_last_message_gets_cache_marker() {
        let body = build_request_body(
            &[Message::user_text("first"), Message::user_text("second")],
            "sys",
            &[],
            &np(),
            &GenerationOptions::default(),
        );
        assert!(
            body["messages"][0]["content"][0]
                .get("cache_control")
                .is_none(),
            "first user message should not be cached"
        );
        assert_eq!(
            body["messages"][1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
    }

    #[test]
    fn assistant_tool_call_translates_to_tool_use_block() {
        let body = build_request_body(
            &[
                Message::user_text("hi"),
                Message::assistant_parts(vec![AssistantPart::ToolCall {
                    tool_call_id: "t1".into(),
                    tool_name: "Bash".into(),
                    input: json!({ "command": "ls" }),
                }]),
            ],
            "sys",
            &[],
            &np(),
            &GenerationOptions::default(),
        );
        let asst = &body["messages"][1];
        assert_eq!(asst["role"], "assistant");
        assert_eq!(asst["content"][0]["type"], "tool_use");
        assert_eq!(asst["content"][0]["id"], "t1");
        assert_eq!(asst["content"][0]["name"], "Bash");
        assert_eq!(asst["content"][0]["input"]["command"], "ls");
    }

    #[test]
    fn tool_results_become_user_with_tool_result_blocks() {
        let body = build_request_body(
            &[Message::tool_results(vec![
                super::super::messages::ToolResult {
                    kind: "tool-result".into(),
                    tool_call_id: "t1".into(),
                    tool_name: "Bash".into(),
                    output: ToolResultContent::Json {
                        value: json!({ "x": 1 }),
                    },
                },
            ])],
            "sys",
            &[],
            &np(),
            &GenerationOptions::default(),
        );
        let tool_msg = &body["messages"][0];
        assert_eq!(tool_msg["role"], "user");
        assert_eq!(tool_msg["content"][0]["type"], "tool_result");
        assert_eq!(tool_msg["content"][0]["tool_use_id"], "t1");
        // content is a JSON-encoded string carrying the original payload.
        let payload: Value =
            serde_json::from_str(tool_msg["content"][0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(payload["x"], 1);
    }

    #[test]
    fn reasoning_effort_sends_thinking_block() {
        let body = build_request_body(
            &[Message::user_text("hi")],
            "sys",
            &[],
            &np(),
            &GenerationOptions {
                max_output_tokens: Some(2000),
                reasoning_effort: Some(ReasoningEffort::High),
            },
        );
        assert_eq!(body["max_tokens"], 2000);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert!(body["thinking"]["budget_tokens"].as_u64().unwrap() > 0);
        assert_eq!(body["effort"], "high");
    }

    #[test]
    fn tools_translate_to_anthropic_shape() {
        let body = build_request_body(
            &[],
            "sys",
            &[ToolDef {
                name: "Bash".into(),
                description: "run".into(),
                input_schema: json!({ "type": "object" }),
            }],
            &np(),
            &GenerationOptions::default(),
        );
        assert_eq!(body["tools"][0]["name"], "Bash");
        assert_eq!(body["tools"][0]["description"], "run");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        // Top-level "function" wrapper from OpenAI must not leak in.
        assert!(body["tools"][0].get("type").is_none());
    }
}
