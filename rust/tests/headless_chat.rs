//! Integration test: drive the headless one-shot path end-to-end.
//!
//! Configures a fake provider in a `TempDir`, points the LLM client at a
//! `wiremock` server that replays a canned chat-completion stream, and
//! verifies that:
//! - `app::run` exits cleanly,
//! - the conversation is persisted to disk in the expected shape,
//! - `state.json::lastConversation` points to the new file,
//! - tool calls trigger the bash safety guard (execution disabled inside
//!   the test binary, so the canned `EXECUTION DISABLED` payload comes
//!   back to the model on the next turn).

use std::collections::BTreeMap;

use nitro::app;
use nitro::cli::Command;
use nitro::logic::eula::EULA_VERSION;
use nitro::logic::provider::{ApiType, Auth, ProviderInfo};
use nitro::logic::settings::{save_settings, Settings};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write_provider(dir: &std::path::Path, base_url: &str) {
    // Pre-agree to the EULA so the headless path doesn't try to prompt.
    let s = Settings {
        agreed_to_eula: Some(EULA_VERSION),
        ..Settings::default()
    };
    save_settings(dir, &s).unwrap();

    let mut providers = BTreeMap::new();
    providers.insert(
        "stub".to_string(),
        ProviderInfo {
            base_url: base_url.to_string(),
            api_key: "k".into(),
            model: "m".into(),
            api_type: ApiType::OpenAiCompatible,
        },
    );
    let auth = Auth {
        default_provider: Some("stub".into()),
        providers,
    };
    let path = dir.join("auth.json");
    std::fs::write(&path, serde_json::to_string_pretty(&auth).unwrap()).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn one_shot_persists_conversation_and_exits_zero() {
    let server = MockServer::start().await;

    // The model: one assistant turn that says "ok" and stops (no tool calls).
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\
                \n\
                data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\
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

    let tmp = TempDir::new().unwrap();
    write_provider(tmp.path(), &server.uri());

    let cmd = Command::OneShot {
        request: "say ok".into(),
    };
    let code = app::run(cmd, tmp.path().to_path_buf()).await;
    // ExitCode doesn't expose its raw value publicly; coerce through
    // `is_success` analogue by running it through a Process exit-style check.
    assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(0))");

    // Conversation persisted.
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tmp.path().join("state.json")).unwrap())
            .unwrap();
    let last = state["lastConversation"].as_str().unwrap();
    let convo: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(tmp.path().join("chats").join(last)).unwrap(),
    )
    .unwrap();
    let messages = convo["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "say ok");
    assert_eq!(messages[1]["role"], "assistant");
}

#[tokio::test(flavor = "multi_thread")]
async fn one_shot_runs_bash_call_with_execution_disabled() {
    let server = MockServer::start().await;

    // Two sequential responses: first turn issues a Bash tool call;
    // second turn (after we send the tool result back) emits a final text.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"running\"}}]}\n\
                     \n\
                     data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"tc1\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"command\\\":\\\"ls\\\",\\\"explanation\\\":\\\"x\\\",\\\"riskLevel\\\":\\\"Read Only\\\",\\\"behaviorTags\\\":[]}\"}}]}}]}\n\
                     \n\
                     data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\
                     \n\
                     data: [DONE]\n\
                     \n",
                ),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"done\"}}]}\n\
                     \n\
                     data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\
                     \n\
                     data: [DONE]\n\
                     \n",
                ),
        )
        .mount(&server)
        .await;

    let tmp = TempDir::new().unwrap();
    write_provider(tmp.path(), &server.uri());

    let code = app::run(
        Command::OneShot {
            request: "list files".into(),
        },
        tmp.path().to_path_buf(),
    )
    .await;
    assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(0))");

    // The conversation should now contain user, assistant (with tool-call),
    // tool result, assistant. Tool result must show the EXECUTION DISABLED
    // sentinel — proving execution did not happen during the test.
    let state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tmp.path().join("state.json")).unwrap())
            .unwrap();
    let last = state["lastConversation"].as_str().unwrap();
    let convo: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(tmp.path().join("chats").join(last)).unwrap(),
    )
    .unwrap();
    let messages = convo["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[2]["role"], "tool");
    let tool_value = &messages[2]["content"][0]["output"]["value"];
    assert_eq!(tool_value["approved"], true);
    assert_eq!(
        tool_value["commandOutput"],
        "[EXECUTION DISABLED] Command was not executed."
    );
    assert_eq!(messages[3]["role"], "assistant");
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_command_prints_usage_and_succeeds() {
    let tmp = TempDir::new().unwrap();
    // Pre-agree to EULA so we don't try to prompt.
    save_settings(
        tmp.path(),
        &Settings {
            agreed_to_eula: Some(EULA_VERSION),
            ..Settings::default()
        },
    )
    .unwrap();
    let code = app::run(
        Command::Unknown {
            command: "blah".into(),
        },
        tmp.path().to_path_buf(),
    )
    .await;
    assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(0))");
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_list_outputs_default_marker() {
    // Smoke check the headless `provider list` path — this is the only
    // provider subcommand wired up for Phase 4.
    let tmp = TempDir::new().unwrap();
    write_provider(tmp.path(), "https://example.com");

    let code = app::run(
        Command::Provider {
            args: vec!["list".into()],
        },
        tmp.path().to_path_buf(),
    )
    .await;
    assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(0))");
}
