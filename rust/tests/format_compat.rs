//! Cross-binary format compatibility.
//!
//! These tests pin the on-disk JSON shape of every file Nitro persists,
//! using fixtures that were copy-pasted from the TypeScript schemas. If a
//! Rust struct ever drifts from the TS wire format, one of these will fail.

use std::fs;

use nitro::logic::conversation::{
    chats_dir, load_conversation, save_conversation, state_path, Conversation, StateFile,
};
use nitro::logic::provider::{
    auth_path, get_default_provider, list_providers, load_auth, set_default_provider, set_provider,
    ApiType, Auth, ProviderInfo,
};
use nitro::logic::settings::{load_settings, settings_path, ReasoningEffort, Settings};
use serde_json::json;
use tempfile::TempDir;

#[test]
fn settings_fixture_from_ts_parses_correctly() {
    // Shape produced by the TS `loadSettings` after a fresh install. Every
    // field is present so the legacy file path (no missing keys) is exercised.
    let fixture = json!({
        "agreedToEula": 1,
        "setupCompleted": true,
        "alwaysConfirm": false,
        "showThinking": true,
        "showTokenSummary": false,
        "maxOutputTokens": 12345,
        "reasoningEffort": "high",
    });

    let tmp = TempDir::new().unwrap();
    nitro::logic::config::ensure_app_data_dir(tmp.path()).unwrap();
    fs::write(settings_path(tmp.path()), fixture.to_string()).unwrap();

    let s = load_settings(tmp.path()).unwrap();
    assert_eq!(s.agreed_to_eula, Some(1));
    assert!(s.setup_completed);
    assert!(s.show_thinking);
    assert_eq!(s.max_output_tokens, 12345);
    assert_eq!(s.reasoning_effort, ReasoningEffort::High);
}

#[test]
fn settings_written_by_rust_has_ts_compatible_shape() {
    let tmp = TempDir::new().unwrap();
    let s = Settings::default();
    nitro::logic::settings::save_settings(tmp.path(), &s).unwrap();
    let raw = fs::read_to_string(settings_path(tmp.path())).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();

    let expected_keys = [
        "agreedToEula",
        "setupCompleted",
        "alwaysConfirm",
        "showThinking",
        "showTokenSummary",
        "maxOutputTokens",
        "reasoningEffort",
    ];
    for k in expected_keys {
        assert!(v.get(k).is_some(), "missing key {k} in {raw}");
    }
    // Defaults should match TS exactly.
    assert!(v["agreedToEula"].is_null());
    assert_eq!(v["maxOutputTokens"], 16_000);
    assert_eq!(v["reasoningEffort"], "med");
}

#[test]
fn auth_fixture_from_ts_parses_correctly() {
    let fixture = json!({
        "defaultProvider": "openai",
        "providers": {
            "openai": {
                "baseURL": "https://api.openai.com/v1",
                "apiKey": "sk-x",
                "model": "gpt-5",
                "apiType": "openai-responses"
            },
            "anthropic": {
                "baseURL": "https://api.anthropic.com/v1",
                "apiKey": "sk-ant",
                "model": "claude-sonnet-4-6",
                "apiType": "anthropic"
            }
        }
    });
    let tmp = TempDir::new().unwrap();
    nitro::logic::config::ensure_app_data_dir(tmp.path()).unwrap();
    fs::write(auth_path(tmp.path()), fixture.to_string()).unwrap();

    let auth = load_auth(tmp.path()).unwrap();
    assert_eq!(auth.default_provider.as_deref(), Some("openai"));
    let openai = auth.providers.get("openai").unwrap();
    assert_eq!(openai.api_type, ApiType::OpenAiResponses);
    assert_eq!(openai.model, "gpt-5");

    let mut names = list_providers(tmp.path()).unwrap();
    names.sort();
    assert_eq!(names, vec!["anthropic", "openai"]);
    let def = get_default_provider(tmp.path()).unwrap().unwrap();
    assert_eq!(def.name, "openai");
}

#[test]
fn auth_written_by_rust_has_ts_compatible_shape() {
    let tmp = TempDir::new().unwrap();
    set_provider(
        tmp.path(),
        "groq",
        ProviderInfo {
            base_url: "https://api.groq.com/openai/v1".into(),
            api_key: "k".into(),
            model: "m".into(),
            api_type: ApiType::OpenAiCompatible,
        },
    )
    .unwrap();
    set_default_provider(tmp.path(), "groq").unwrap();

    let raw = fs::read_to_string(auth_path(tmp.path())).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["defaultProvider"], "groq");
    let p = &v["providers"]["groq"];
    assert!(p["baseURL"].is_string());
    assert!(p["apiKey"].is_string());
    assert!(p["apiType"].is_string());
    assert_eq!(p["apiType"], "openai-compatible");

    // Defensive: no snake_case keys leaked through.
    assert!(p.get("base_url").is_none());
    assert!(p.get("api_key").is_none());
    assert!(p.get("api_type").is_none());
}

#[test]
fn empty_auth_round_trips() {
    let tmp = TempDir::new().unwrap();
    nitro::logic::config::ensure_app_data_dir(tmp.path()).unwrap();
    fs::write(
        auth_path(tmp.path()),
        json!({ "defaultProvider": null, "providers": {} }).to_string(),
    )
    .unwrap();
    let auth = load_auth(tmp.path()).unwrap();
    assert_eq!(auth, Auth::default());
}

#[test]
fn conversation_messages_are_arbitrary_json() {
    let tmp = TempDir::new().unwrap();
    // Mirrors a real assistant message produced by the TS app: includes
    // reasoning + tool call parts.
    let messages = vec![
        json!({ "role": "user", "content": "list files" }),
        json!({
            "role": "assistant",
            "content": [
                { "type": "reasoning", "text": "I'll use ls." },
                { "type": "text", "text": "Here you go:" },
                { "type": "tool-call", "toolCallId": "t1", "toolName": "Bash",
                  "input": { "command": "ls", "explanation": "list",
                             "riskLevel": "Read Only", "behaviorTags": ["Safe"], "timeout": 30000 } },
            ],
        }),
        json!({ "role": "tool", "content": [
            { "type": "tool-result", "toolCallId": "t1", "toolName": "Bash",
              "output": { "type": "json", "value": { "command": "ls", "approved": true,
                          "commandOutput": "Cargo.toml\n", "exitCode": 0 } } },
        ]}),
    ];
    let name = save_conversation(tmp.path(), &messages, None).unwrap();

    // Wire shape: { "messages": [...] } only.
    let raw = fs::read_to_string(chats_dir(tmp.path()).join(&name)).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        v.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["messages"]
    );

    // State wire shape.
    let state_raw = fs::read_to_string(state_path(tmp.path())).unwrap();
    let s: StateFile = serde_json::from_str(&state_raw).unwrap();
    assert_eq!(s.last_conversation.as_deref(), Some(name.as_str()));

    // Loading produces the exact same opaque Values we stored.
    let loaded: Conversation = load_conversation(tmp.path(), &name).unwrap();
    assert_eq!(loaded.messages, messages);
}

#[test]
fn ts_state_file_with_unknown_keys_is_tolerated() {
    let tmp = TempDir::new().unwrap();
    nitro::logic::config::ensure_app_data_dir(tmp.path()).unwrap();
    // Future TS versions may add new keys; we should ignore them rather than
    // refuse to load.
    fs::write(
        state_path(tmp.path()),
        json!({ "lastConversation": "abc.json", "futureField": 42 }).to_string(),
    )
    .unwrap();
    assert_eq!(
        nitro::logic::conversation::last_conversation_filename(tmp.path()).as_deref(),
        Some("abc.json")
    );
}
