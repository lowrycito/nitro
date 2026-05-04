//! LLM provider abstraction.
//!
//! Mirrors `src/logic/llm.ts`. The TS code delegates the wire format to the
//! Vercel AI SDK; in Rust we implement the wire format directly so we don't
//! pull in a JS-shaped SDK. The internal `Message` type matches the AI SDK's
//! `ModelMessage` shape closely enough that conversation files written by
//! either binary stay readable by the other.

pub mod messages;
mod sse;
pub mod stream;
mod tool_def;
pub mod transcripts;

pub mod anthropic;
pub mod openai_compatible;

use std::time::Duration;

use serde::Deserialize;

pub use messages::{
    AssistantContent, Message, MessageContent, ToolCall, ToolResult, ToolResultContent, UserPart,
};
pub use stream::{StreamError, StreamEvent, StreamResult, Usage};
pub use tool_def::ToolDef;

use crate::logic::provider::{ApiType, NamedProvider};
use crate::logic::settings::ReasoningEffort;

/// Knobs that map to the TS `GenerationOptions`.
#[derive(Debug, Clone, Default)]
pub struct GenerationOptions {
    pub max_output_tokens: Option<u32>,
    pub reasoning_effort: Option<ReasoningEffort>,
}

/// Async-trait abstraction over the three back-ends. Lives behind `dyn` so
/// the chat loop can swap providers without monomorphising the screen tree.
#[async_trait::async_trait]
pub trait CompletionProvider: Send + Sync {
    /// Stream a single completion turn. The returned stream emits
    /// [`StreamEvent`]s and ends with either `Done` or an `Error`.
    async fn stream_completion(
        &self,
        provider: &NamedProvider,
        messages: &[Message],
        system_prompt: &str,
        tools: &[ToolDef],
        options: &GenerationOptions,
    ) -> StreamResult;
}

/// Build the right back-end for a given provider config. The TS equivalent
/// is `createClient` in `src/logic/llm.ts`.
pub fn build_provider(api_type: ApiType) -> Box<dyn CompletionProvider> {
    match api_type {
        ApiType::Anthropic => Box::new(anthropic::AnthropicClient::new()),
        // Both `openai-responses` and `openai-compatible` route through the
        // chat-completions adapter for now. The TS app uses the OpenAI
        // Responses API for native reasoning controls; we re-implement that
        // path in Phase 8 if users hit reasoning gaps. See PARITY.md.
        ApiType::OpenAiCompatible | ApiType::OpenAiResponses => {
            Box::new(openai_compatible::OpenAiCompatibleClient::new())
        }
    }
}

/// Top-level entry: validates the request and delegates to the correct
/// back-end. Mirrors `generateCompletion` from the TS side.
pub async fn stream_completion(
    provider: &NamedProvider,
    messages: &[Message],
    system_prompt: &str,
    tools: &[ToolDef],
    options: &GenerationOptions,
) -> StreamResult {
    let backend = build_provider(provider.info.api_type);
    backend
        .stream_completion(provider, messages, system_prompt, tools, options)
        .await
}

/// Mirrors the `/exit` shortcut handled by `transformInput` in the TS code.
/// Returning `None` signals the caller to terminate the session.
pub fn transform_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed == "/exit" {
        None
    } else {
        Some(input.to_string())
    }
}

/// HTTP client constructor. Centralised so timeout + UA are consistent.
pub(crate) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("nitro-rust/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(30))
        .build()
        .expect("client builder cannot fail with these defaults")
}

/// `defaultProviders.fetchModels` — list models from a provider's `/models`
/// endpoint, returning an empty list on error to match TS semantics.
pub async fn fetch_models(base_url: &str, api_key: &str, api_type: ApiType) -> Vec<String> {
    #[derive(Deserialize)]
    struct ModelEntry {
        id: String,
    }
    #[derive(Deserialize)]
    struct ModelsResponse {
        data: Vec<ModelEntry>,
    }

    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let mut req = http_client()
        .get(&url)
        .timeout(Duration::from_millis(2_000));
    req = match api_type {
        ApiType::Anthropic => req
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        _ => req.header("Authorization", format!("Bearer {api_key}")),
    };

    let Ok(resp) = req.send().await else {
        return Vec::new();
    };
    if !resp.status().is_success() {
        return Vec::new();
    }
    let Ok(parsed) = resp.json::<ModelsResponse>().await else {
        return Vec::new();
    };
    parsed.data.into_iter().map(|m| m.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_input_passes_through() {
        assert_eq!(transform_input("hello"), Some("hello".to_string()));
    }

    #[test]
    fn transform_input_handles_exit() {
        assert!(transform_input("/exit").is_none());
        assert!(transform_input("  /exit  ").is_none());
    }

    #[test]
    fn usage_wire_is_intuitive() {
        // Sanity: keys we use to deserialise upstream usage objects are stable.
        let v = serde_json::to_value(Usage {
            input_tokens: 1,
            output_tokens: 2,
            ..Default::default()
        })
        .unwrap();
        assert!(v.get("input_tokens").is_some());
    }
}
