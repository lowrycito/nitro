//! OpenAI Chat Completions (`/chat/completions`) adapter.
//!
//! Used for both `openai-compatible` and (currently) `openai-responses`
//! providers. Reasoning effort is forwarded via the documented top-level
//! field `reasoning_effort`; providers that don't recognise it ignore it.

use std::collections::HashMap;

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
pub struct OpenAiCompatibleClient;

impl OpenAiCompatibleClient {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CompletionProvider for OpenAiCompatibleClient {
    async fn stream_completion(
        &self,
        provider: &NamedProvider,
        messages: &[Message],
        system_prompt: &str,
        tools: &[ToolDef],
        options: &GenerationOptions,
    ) -> StreamResult {
        let url = format!(
            "{}/chat/completions",
            provider.info.base_url.trim_end_matches('/')
        );
        let body = build_request_body(messages, system_prompt, tools, provider, options);

        let resp = match http_client()
            .post(&url)
            .bearer_auth(&provider.info.api_key)
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

        let byte_stream = resp.bytes_stream();
        let sse = into_sse_events(byte_stream);

        Box::pin(async_stream::try_stream! {
            // Tool calls stream as deltas indexed by `index`. We accumulate
            // their `arguments` strings until the chunk that finalises the
            // call (finish_reason) and then yield a single ToolCall event.
            let mut active_tool_calls: HashMap<u32, BufferedToolCall> = HashMap::new();
            let mut last_usage: Option<Usage> = None;

            let mut sse = Box::pin(sse);
            while let Some(item) = sse.next().await {
                let ev = item?;
                if ev.data == "[DONE]" {
                    break;
                }
                let chunk: ChatCompletionChunk = match serde_json::from_str(&ev.data) {
                    Ok(c) => c,
                    Err(e) => {
                        Err(StreamError::Parse(format!("chunk: {e}: {}", ev.data)))?;
                        unreachable!()
                    }
                };

                if let Some(usage) = chunk.usage {
                    last_usage = Some(usage_from_oai(&usage));
                }

                for choice in chunk.choices {
                    if let Some(delta) = choice.delta {
                        if let Some(text) = delta.content {
                            if !text.is_empty() {
                                yield StreamEvent::TextDelta(text);
                            }
                        }
                        if let Some(reasoning) = delta.reasoning_content.or(delta.reasoning) {
                            if !reasoning.is_empty() {
                                yield StreamEvent::ReasoningDelta(reasoning);
                            }
                        }
                        if let Some(tool_calls) = delta.tool_calls {
                            for tc in tool_calls {
                                let entry = active_tool_calls.entry(tc.index).or_default();
                                if let Some(id) = tc.id {
                                    entry.id = Some(id);
                                }
                                if let Some(name) = tc.function.as_ref().and_then(|f| f.name.clone()) {
                                    entry.name = Some(name);
                                }
                                if let Some(args) = tc.function.as_ref().and_then(|f| f.arguments.clone()) {
                                    entry.arguments.push_str(&args);
                                }
                            }
                        }
                    }

                    if let Some(reason) = choice.finish_reason {
                        if reason == "tool_calls" || reason == "stop" {
                            // Emit any finalised tool calls.
                            let mut sorted: Vec<_> = active_tool_calls.drain().collect();
                            sorted.sort_by_key(|(i, _)| *i);
                            for (_, buf) in sorted {
                                if let Some(call) = buf.into_call() {
                                    yield StreamEvent::ToolCall(call);
                                }
                            }
                        }
                    }
                }
            }

            // Drain any remaining buffered calls (some providers don't set
            // `finish_reason: tool_calls` reliably).
            let mut remaining: Vec<_> = active_tool_calls.drain().collect();
            remaining.sort_by_key(|(i, _)| *i);
            for (_, buf) in remaining {
                if let Some(call) = buf.into_call() {
                    yield StreamEvent::ToolCall(call);
                }
            }

            if let Some(u) = last_usage {
                yield StreamEvent::Usage(u);
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

#[derive(Default)]
struct BufferedToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl BufferedToolCall {
    fn into_call(self) -> Option<ToolCall> {
        let id = self.id?;
        let name = self.name?;
        // Empty arguments are valid (e.g. tool with no params).
        let input: Value = if self.arguments.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&self.arguments).unwrap_or(json!({}))
        };
        Some(ToolCall {
            tool_call_id: id,
            tool_name: name,
            input,
        })
    }
}

fn usage_from_oai(u: &OpenAiUsage) -> Usage {
    Usage {
        input_tokens: u.prompt_tokens.unwrap_or(0),
        output_tokens: u.completion_tokens.unwrap_or(0),
        cached_input_tokens: u
            .prompt_tokens_details
            .as_ref()
            .and_then(|d| d.cached_tokens)
            .unwrap_or(0),
        reasoning_tokens: u
            .completion_tokens_details
            .as_ref()
            .and_then(|d| d.reasoning_tokens)
            .unwrap_or(0),
    }
}

fn build_request_body(
    messages: &[Message],
    system_prompt: &str,
    tools: &[ToolDef],
    provider: &NamedProvider,
    options: &GenerationOptions,
) -> Value {
    let mut wire_messages: Vec<Value> = Vec::with_capacity(messages.len() + 1);
    wire_messages.push(json!({ "role": "system", "content": system_prompt }));
    for m in messages {
        wire_messages.extend(message_to_oai(m));
    }

    let tools_json: Vec<Value> = tools.iter().map(tool_to_oai).collect();

    let mut body = json!({
        "model": provider.info.model,
        "messages": wire_messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if let Some(max) = options.max_output_tokens {
        body["max_tokens"] = json!(max);
    }
    if let Some(eff) = options.reasoning_effort {
        body["reasoning_effort"] = json!(reasoning_effort_string(eff));
    }
    if !tools_json.is_empty() {
        body["tools"] = json!(tools_json);
        body["tool_choice"] = json!("auto");
    }
    body
}

fn reasoning_effort_string(eff: ReasoningEffort) -> &'static str {
    match eff {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Med => "medium",
        ReasoningEffort::High => "high",
    }
}

fn tool_to_oai(t: &ToolDef) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": t.name,
            "description": t.description,
            "parameters": t.input_schema,
        }
    })
}

fn message_to_oai(m: &Message) -> Vec<Value> {
    match m {
        Message::User { content } => {
            let text = match content {
                MessageContent::Text(s) => s.clone(),
                MessageContent::Parts(parts) => parts
                    .iter()
                    .map(|p| match p {
                        UserPart::Text { text } => text.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(""),
            };
            vec![json!({ "role": "user", "content": text })]
        }
        Message::Assistant { content } => {
            let parts = match content {
                AssistantContent::Text(s) => vec![AssistantPart::Text { text: s.clone() }],
                AssistantContent::Parts(p) => p.clone(),
            };
            let mut text_buf = String::new();
            let mut tool_calls: Vec<Value> = Vec::new();
            for p in parts {
                match p {
                    AssistantPart::Text { text } => text_buf.push_str(&text),
                    // OpenAI chat completions has no native reasoning slot in
                    // *replayed* messages — drop reasoning from history. The
                    // turn's reasoning is sent live via SSE only.
                    AssistantPart::Reasoning { .. } => {}
                    AssistantPart::ToolCall {
                        tool_call_id,
                        tool_name,
                        input,
                    } => tool_calls.push(json!({
                        "id": tool_call_id,
                        "type": "function",
                        "function": {
                            "name": tool_name,
                            "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".into()),
                        }
                    })),
                }
            }
            let mut msg = json!({ "role": "assistant" });
            if !text_buf.is_empty() {
                msg["content"] = json!(text_buf);
            } else {
                // Some providers reject messages without content; an empty
                // string is the canonical placeholder.
                msg["content"] = json!("");
            }
            if !tool_calls.is_empty() {
                msg["tool_calls"] = json!(tool_calls);
            }
            vec![msg]
        }
        Message::Tool { content } => content
            .iter()
            .map(|r| {
                let payload = match &r.output {
                    ToolResultContent::Json { value } => {
                        serde_json::to_string(value).unwrap_or_default()
                    }
                    ToolResultContent::ErrorText { value } => value.clone(),
                    ToolResultContent::ErrorJson { value } => {
                        serde_json::to_string(value).unwrap_or_default()
                    }
                };
                json!({
                    "role": "tool",
                    "tool_call_id": r.tool_call_id,
                    "content": payload,
                })
            })
            .collect(),
    }
}

// --- Wire-format types for streamed chat-completion chunks. -----------------

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    #[serde(default)]
    delta: Option<ChatDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatDelta {
    #[serde(default)]
    content: Option<String>,
    /// Some providers (e.g. DeepSeek) put reasoning here:
    #[serde(default)]
    reasoning_content: Option<String>,
    /// OpenAI uses this:
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct DeltaToolCall {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<DeltaToolCallFn>,
}

#[derive(Debug, Deserialize)]
struct DeltaToolCallFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
    #[serde(default)]
    prompt_tokens_details: Option<PromptDetails>,
    #[serde(default)]
    completion_tokens_details: Option<CompletionDetails>,
}

#[derive(Debug, Deserialize)]
struct PromptDetails {
    #[serde(default)]
    cached_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CompletionDetails {
    #[serde(default)]
    reasoning_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::provider::{ApiType, ProviderInfo};

    fn np() -> NamedProvider {
        NamedProvider {
            name: "test".into(),
            info: ProviderInfo {
                base_url: "https://example.com/v1".into(),
                api_key: "k".into(),
                model: "m".into(),
                api_type: ApiType::OpenAiCompatible,
            },
        }
    }

    #[test]
    fn user_text_translates_directly() {
        let m = Message::user_text("hi");
        let out = message_to_oai(&m);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"], "hi");
    }

    #[test]
    fn assistant_with_tool_call_emits_tool_calls_array() {
        let m = Message::assistant_parts(vec![
            AssistantPart::Text {
                text: "running...".into(),
            },
            AssistantPart::ToolCall {
                tool_call_id: "t1".into(),
                tool_name: "Bash".into(),
                input: json!({ "command": "ls" }),
            },
        ]);
        let out = message_to_oai(&m);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["content"], "running...");
        assert_eq!(out[0]["tool_calls"][0]["id"], "t1");
        assert_eq!(out[0]["tool_calls"][0]["function"]["name"], "Bash");
        let args: Value = serde_json::from_str(
            out[0]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(args["command"], "ls");
    }

    #[test]
    fn assistant_reasoning_is_dropped_in_history() {
        let m = Message::assistant_parts(vec![AssistantPart::Reasoning {
            text: "secret".into(),
        }]);
        let out = message_to_oai(&m);
        // Empty content keyed but reasoning text not present.
        assert_eq!(out[0]["content"], "");
        assert!(out[0].get("tool_calls").is_none());
    }

    #[test]
    fn tool_results_become_tool_role_messages() {
        let m = Message::tool_results(vec![super::super::messages::ToolResult {
            kind: "tool-result".into(),
            tool_call_id: "t1".into(),
            tool_name: "Bash".into(),
            output: ToolResultContent::Json {
                value: json!({ "approved": true }),
            },
        }]);
        let out = message_to_oai(&m);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["role"], "tool");
        assert_eq!(out[0]["tool_call_id"], "t1");
        let content_str = out[0]["content"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(content_str).unwrap();
        assert_eq!(parsed["approved"], true);
    }

    #[test]
    fn body_includes_max_tokens_and_reasoning_effort() {
        let body = build_request_body(
            &[Message::user_text("hi")],
            "system",
            &[],
            &np(),
            &GenerationOptions {
                max_output_tokens: Some(1234),
                reasoning_effort: Some(ReasoningEffort::High),
            },
        );
        assert_eq!(body["max_tokens"], 1234);
        assert_eq!(body["reasoning_effort"], "high");
        assert_eq!(body["stream"], true);
        assert_eq!(body["model"], "m");
        assert_eq!(body["messages"][0]["role"], "system");
    }

    #[test]
    fn tool_def_translates_to_function_tool() {
        let body = build_request_body(
            &[],
            "system",
            &[ToolDef {
                name: "Bash".into(),
                description: "do bash".into(),
                input_schema: json!({ "type": "object" }),
            }],
            &np(),
            &GenerationOptions::default(),
        );
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "Bash");
        assert_eq!(body["tools"][0]["function"]["description"], "do bash");
        assert_eq!(body["tool_choice"], "auto");
    }
}
