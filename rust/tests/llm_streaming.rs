//! End-to-end streaming tests for both LLM adapters.
//!
//! Drives the real `stream_completion` pipeline against a `wiremock`
//! stub-server that replays canned SSE byte streams. Catches regressions in
//! the SSE parser, chunk-aggregation logic, and tool-call decoding without
//! ever talking to a live provider.

use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt;
use nitro::logic::llm::messages::{Message, ToolCall};
use nitro::logic::llm::stream::{StreamEvent, Usage};
use nitro::logic::llm::{stream_completion, GenerationOptions, ToolDef};
use nitro::logic::provider::{ApiType, NamedProvider, ProviderInfo};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn provider(server: &MockServer, api: ApiType) -> NamedProvider {
    NamedProvider {
        name: "test".into(),
        info: ProviderInfo {
            base_url: server.uri(),
            api_key: "k".into(),
            model: "m".into(),
            api_type: api,
        },
    }
}

async fn drain(provider: NamedProvider) -> (Vec<StreamEvent>, Vec<String>) {
    let mut s = stream_completion(
        &provider,
        &[Message::user_text("hi")],
        "system",
        &[ToolDef {
            name: "Bash".into(),
            description: "run".into(),
            input_schema: json!({ "type": "object" }),
        }],
        &GenerationOptions::default(),
    )
    .await;

    let mut events = Vec::new();
    let mut errors = Vec::new();
    while let Some(item) = s.next().await {
        match item {
            Ok(ev) => events.push(ev),
            Err(e) => errors.push(format!("{e}")),
        }
    }
    (events, errors)
}

#[tokio::test(flavor = "multi_thread")]
async fn openai_compatible_streams_text_then_tool_call() {
    let server = MockServer::start().await;
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\
                \n\
                data: {\"choices\":[{\"delta\":{\"content\":\", world.\"}}]}\n\
                \n\
                data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tc1\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"\"}}]}}]}\n\
                \n\
                data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"ls\\\"}\"}}]}}]}\n\
                \n\
                data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\
                \n\
                data: [DONE]\n\
                \n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let (events, errors) = drain(provider(&server, ApiType::OpenAiCompatible)).await;
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");

    // Text deltas must come through in order, then a single tool call, then usage.
    let texts: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["Hello", ", world."]);

    let calls: Vec<&ToolCall> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ToolCall(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 1, "events: {events:?}");
    assert_eq!(calls[0].tool_name, "Bash");
    assert_eq!(calls[0].tool_call_id, "tc1");
    assert_eq!(calls[0].input["command"], "ls");

    let usage = events
        .iter()
        .find_map(|e| match e {
            StreamEvent::Usage(u) => Some(*u),
            _ => None,
        })
        .expect("usage event");
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 5);

    assert!(matches!(events.last(), Some(StreamEvent::Done)));
}

#[tokio::test(flavor = "multi_thread")]
async fn anthropic_streams_text_thinking_and_tool_use() {
    let server = MockServer::start().await;
    let body = "event: message_start\n\
                data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":12,\"cache_read_input_tokens\":3}}}\n\
                \n\
                event: content_block_start\n\
                data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\"}}\n\
                \n\
                event: content_block_delta\n\
                data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"plan-\"}}\n\
                \n\
                event: content_block_delta\n\
                data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"ok\"}}\n\
                \n\
                event: content_block_stop\n\
                data: {\"type\":\"content_block_stop\",\"index\":0}\n\
                \n\
                event: content_block_start\n\
                data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\"}}\n\
                \n\
                event: content_block_delta\n\
                data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\
                \n\
                event: content_block_stop\n\
                data: {\"type\":\"content_block_stop\",\"index\":1}\n\
                \n\
                event: content_block_start\n\
                data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Bash\"}}\n\
                \n\
                event: content_block_delta\n\
                data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\":\\\"\"}}\n\
                \n\
                event: content_block_delta\n\
                data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"ls\\\"}\"}}\n\
                \n\
                event: content_block_stop\n\
                data: {\"type\":\"content_block_stop\",\"index\":2}\n\
                \n\
                event: message_delta\n\
                data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":7}}\n\
                \n\
                event: message_stop\n\
                data: {\"type\":\"message_stop\"}\n\
                \n";
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let (events, errors) = drain(provider(&server, ApiType::Anthropic)).await;
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");

    let reasoning: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ReasoningDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(reasoning, vec!["plan-", "ok"]);

    let texts: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::TextDelta(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["Hi"]);

    let calls: Vec<&ToolCall> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::ToolCall(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 1, "events: {events:?}");
    assert_eq!(calls[0].tool_call_id, "toolu_1");
    assert_eq!(calls[0].input["command"], "ls");

    let usage = events
        .iter()
        .find_map(|e| match e {
            StreamEvent::Usage(u) => Some(*u),
            _ => None,
        })
        .expect("usage");
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.cached_input_tokens, 3);
    assert_eq!(usage.output_tokens, 7);

    assert!(matches!(events.last(), Some(StreamEvent::Done)));
}

#[tokio::test(flavor = "multi_thread")]
async fn openai_compatible_surfaces_http_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("{\"error\":\"unauthorized\"}"))
        .mount(&server)
        .await;

    let (events, errors) = drain(provider(&server, ApiType::OpenAiCompatible)).await;
    assert!(events.is_empty(), "events: {events:?}");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("401"), "error text: {}", errors[0]);
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_models_returns_ids_on_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "data": [{ "id": "a" }, { "id": "b" }] })),
        )
        .mount(&server)
        .await;

    let names = nitro::logic::llm::fetch_models(&server.uri(), "k", ApiType::OpenAiCompatible).await;
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_models_returns_empty_on_failure() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let names = nitro::logic::llm::fetch_models(&server.uri(), "k", ApiType::OpenAiCompatible).await;
    assert!(names.is_empty());
}

// Tiny smoke check that the test harness fixture surface above doesn't drift
// from the public API: imports compile, basic field access works.
#[test]
fn types_have_expected_shape() {
    let _u = Usage {
        input_tokens: 1,
        output_tokens: 2,
        cached_input_tokens: 0,
        reasoning_tokens: 0,
    };
    let _: HashMap<u32, &str> = HashMap::new();
    let _: Duration = Duration::from_millis(1);
}
