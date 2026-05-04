//! Headless chat loop. Streams one assistant turn at a time, runs tool
//! calls, and saves the conversation after each step. Mirrors
//! `useChatState.ts` minus the React state machinery.

use std::path::Path;
use std::process::ExitCode;

use futures::StreamExt;
use serde_json::Value;

use super::{console, prompts};
use crate::logic::conversation::{load_conversation, save_conversation};
use crate::logic::llm::messages::{Message, ToolCall, ToolResult, ToolResultContent};
use crate::logic::llm::stream::{StreamEvent, Usage};
use crate::logic::llm::transcripts::AssistantBuilder;
use crate::logic::llm::{stream_completion, GenerationOptions, ToolDef};
use crate::logic::provider::get_default_provider;
use crate::logic::settings::{build_system_prompt, load_settings, Settings};
use crate::tools::ask::{AskTool, QuestionResponse};
use crate::tools::bash::{
    truncate_for_display, BashApproval, BashModelInput, BashTool, BashToolOutput, RiskLevel,
};

/// Drive a single one-shot run. `strict` forces approval for every command
/// (matches `nitro strict` from the TS side).
pub async fn run_one_shot(
    data_dir: &Path,
    request: &str,
    strict: bool,
    initial_filename: Option<String>,
) -> ExitCode {
    let settings = match load_settings(data_dir) {
        Ok(s) => s,
        Err(e) => {
            console::error(&format!("Error: failed to load settings: {e}"));
            return ExitCode::from(1);
        }
    };
    let provider = match get_default_provider(data_dir) {
        Ok(Some(p)) => p,
        Ok(None) => {
            console::error(
                "Error: no default provider configured. Use the TS binary's `nitro provider add` \
                 (Rust port: Phase 6).",
            );
            return ExitCode::from(1);
        }
        Err(e) => {
            console::error(&format!("Error: failed to read providers: {e}"));
            return ExitCode::from(1);
        }
    };

    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let today = chrono_like_date();
    let system_prompt = build_system_prompt(data_dir, &cwd, &today);

    let tools = vec![
        ToolDef {
            name: BashTool::NAME.to_string(),
            description: BashTool::description().to_string(),
            input_schema: BashTool::input_schema(),
        },
        ToolDef {
            name: AskTool::NAME.to_string(),
            description: AskTool::description().to_string(),
            input_schema: AskTool::input_schema(),
        },
    ];

    // Seed the conversation with whatever was on disk for `continue`/`resume`.
    let mut messages: Vec<Message> = match initial_filename.as_deref() {
        Some(name) => match load_conversation(data_dir, name) {
            Some(c) => c
                .messages
                .into_iter()
                .filter_map(|v| serde_json::from_value::<Message>(v).ok())
                .collect(),
            None => {
                console::error("Error: failed to load conversation.");
                return ExitCode::from(1);
            }
        },
        None => Vec::new(),
    };
    if !request.is_empty() {
        messages.push(Message::user_text(request));
    }
    let mut filename = initial_filename;
    let mut total_usage = Usage::default();

    let opts = GenerationOptions {
        max_output_tokens: Some(settings.max_output_tokens),
        reasoning_effort: Some(settings.reasoning_effort),
    };

    // Multi-step agent loop. The model can issue tool calls; we run them and
    // feed results back until it stops issuing calls.
    loop {
        let mut builder = AssistantBuilder::new();
        let mut stream =
            stream_completion(&provider, &messages, &system_prompt, &tools, &opts).await;

        let mut last_was_reasoning = false;
        while let Some(item) = stream.next().await {
            match item {
                Ok(StreamEvent::TextDelta(t)) => {
                    if last_was_reasoning {
                        console::newline();
                        console::newline();
                        last_was_reasoning = false;
                    }
                    builder.push_text(&t);
                    console::stream(&t);
                }
                Ok(StreamEvent::ReasoningDelta(t)) => {
                    if settings.show_thinking {
                        if !last_was_reasoning {
                            console::stream(&format!("\n{}thinking: ", console::fg_secondary()));
                            last_was_reasoning = true;
                        }
                        console::stream(&t);
                    }
                    builder.push_reasoning(&t);
                }
                Ok(StreamEvent::ToolCall(c)) => {
                    builder.push_tool_call(c);
                }
                Ok(StreamEvent::Usage(u)) => {
                    builder.usage = u;
                    total_usage.add(&u);
                }
                Ok(StreamEvent::Done) => break,
                Err(e) => {
                    console::error(&format!("\nError: {e}"));
                    return ExitCode::from(1);
                }
            }
        }
        // Move past streamed text on its own line before tool prompts.
        console::newline();

        let (assistant_msg, tool_calls, _) = builder.finish();
        messages.push(assistant_msg);
        match save_conversation(
            data_dir,
            &serialize_messages(&messages),
            filename.as_deref(),
        ) {
            Ok(name) => filename = Some(name),
            Err(e) => {
                console::error(&format!("Error: failed to save conversation: {e}"));
                return ExitCode::from(1);
            }
        }

        if tool_calls.is_empty() {
            break;
        }

        // Run each tool call and assemble the tool message for the next turn.
        let mut results: Vec<ToolResult> = Vec::with_capacity(tool_calls.len());
        for call in tool_calls {
            let result = run_tool_call(&call, &settings, strict).await;
            results.push(result);
        }
        messages.push(Message::tool_results(results));
        if let Err(e) = save_conversation(
            data_dir,
            &serialize_messages(&messages),
            filename.as_deref(),
        ) {
            console::error(&format!("Error: failed to save conversation: {e}"));
            return ExitCode::from(1);
        }
    }

    if settings.show_token_summary {
        console::dim(&format!(
            "tokens: in={} out={} cached={} reasoning={}",
            total_usage.input_tokens,
            total_usage.output_tokens,
            total_usage.cached_input_tokens,
            total_usage.reasoning_tokens,
        ));
    }

    ExitCode::SUCCESS
}

fn serialize_messages(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|m| serde_json::to_value(m).ok())
        .collect()
}

async fn run_tool_call(call: &ToolCall, settings: &Settings, strict: bool) -> ToolResult {
    match call.tool_name.as_str() {
        "Bash" => run_bash_call(call, settings, strict).await,
        "AskUser" => run_ask_call(call),
        other => ToolResult {
            kind: "tool-result".into(),
            tool_call_id: call.tool_call_id.clone(),
            tool_name: call.tool_name.clone(),
            output: ToolResultContent::ErrorText {
                value: format!("Unknown tool \"{other}\". Available tools: AskUser, Bash"),
            },
        },
    }
}

async fn run_bash_call(call: &ToolCall, settings: &Settings, strict: bool) -> ToolResult {
    // Validate inputs against our schema. Failure → error-text result so the
    // model gets a structured complaint back.
    let model_input: BashModelInput = match serde_json::from_value(call.input.clone()) {
        Ok(v) => v,
        Err(e) => {
            return ToolResult {
                kind: "tool-result".into(),
                tool_call_id: call.tool_call_id.clone(),
                tool_name: "Bash".into(),
                output: ToolResultContent::ErrorText {
                    value: format!("Bash tool call failed validation: {e}"),
                },
            };
        }
    };

    print_bash_prompt(&model_input);

    let approval = decide_approval(&model_input, settings, strict);
    let output = BashTool::execute(&model_input, approval).await;

    print_bash_outcome(&output);

    ToolResult {
        kind: "tool-result".into(),
        tool_call_id: call.tool_call_id.clone(),
        tool_name: "Bash".into(),
        output: ToolResultContent::Json {
            value: serde_json::to_value(&output).unwrap_or(Value::Null),
        },
    }
}

fn decide_approval(
    model_input: &BashModelInput,
    settings: &Settings,
    strict: bool,
) -> BashApproval {
    let auto_safe = matches!(model_input.risk_level, RiskLevel::ReadOnly)
        && !settings.always_confirm
        && !strict;
    if auto_safe {
        return BashApproval::Approved;
    }

    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        // Headless / piped: deny non-Read-Only commands unconditionally so
        // unattended runs can't be coerced into destructive actions.
        return BashApproval::Rejected {
            message: Some(
                "Auto-rejected: non-Read-Only command in non-interactive session.".into(),
            ),
        };
    }

    if prompts::confirm("Approve and run?", false) {
        BashApproval::Approved
    } else {
        let msg = prompts::read_line("Optional rejection message (blank to skip): ")
            .filter(|s| !s.is_empty());
        BashApproval::Rejected { message: msg }
    }
}

fn print_bash_prompt(model_input: &BashModelInput) {
    console::warn(&format!("Bash: {}", model_input.command));
    if !model_input.explanation.is_empty() {
        console::dim(&model_input.explanation);
    }
    let risk = serde_json::to_string(&model_input.risk_level)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string();
    let tags = if model_input.behavior_tags.is_empty() {
        String::new()
    } else {
        let tags: Vec<String> = model_input
            .behavior_tags
            .iter()
            .map(|t| {
                serde_json::to_string(t)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string()
            })
            .collect();
        format!(" [{}]", tags.join(", "))
    };
    console::dim(&format!("risk: {risk}{tags}"));
}

fn print_bash_outcome(output: &BashToolOutput) {
    match output {
        BashToolOutput::Approved {
            command,
            command_output,
            exit_code,
            ..
        } => {
            console::warn(&format!("Bash: {command}  (exit {exit_code})"));
            console::dim(&truncate_for_display(command_output));
        }
        BashToolOutput::Rejected {
            command,
            rejection_message,
            ..
        } => {
            console::error(&format!("[Denied] Bash: {command}"));
            if let Some(msg) = rejection_message {
                console::dim(msg);
            }
        }
    }
}

fn run_ask_call(call: &ToolCall) -> ToolResult {
    #[derive(serde::Deserialize)]
    struct AskInput {
        questions: Vec<crate::tools::ask::Question>,
    }
    let parsed: AskInput = match serde_json::from_value(call.input.clone()) {
        Ok(v) => v,
        Err(e) => {
            return ToolResult {
                kind: "tool-result".into(),
                tool_call_id: call.tool_call_id.clone(),
                tool_name: "AskUser".into(),
                output: ToolResultContent::ErrorText {
                    value: format!("AskUser tool call failed validation: {e}"),
                },
            };
        }
    };

    let mut answers: Vec<QuestionResponse> = Vec::with_capacity(parsed.questions.len());
    for q in parsed.questions {
        console::warn(&format!("\n{}", q.title));
        console::info(&q.question);
        for (i, c) in q.choices.iter().enumerate() {
            let desc = c
                .description
                .as_ref()
                .map(|d| format!(" — {d}"))
                .unwrap_or_default();
            console::info(&format!("  {}) {}{desc}", i + 1, c.label));
        }
        let answer = prompts::read_line("Your answer (number or free text): ").unwrap_or_default();
        let chosen = answer
            .parse::<usize>()
            .ok()
            .and_then(|i| q.choices.get(i.saturating_sub(1)).map(|c| c.label.clone()))
            .unwrap_or(answer);
        answers.push(QuestionResponse {
            question: q.question,
            answer: chosen,
        });
    }

    let out = AskTool::execute(answers);
    ToolResult {
        kind: "tool-result".into(),
        tool_call_id: call.tool_call_id.clone(),
        tool_name: "AskUser".into(),
        output: ToolResultContent::Json {
            value: serde_json::to_value(&out).unwrap_or(Value::Null),
        },
    }
}

/// Tiny "Month Day, Year" formatter using `time` semantics without pulling in
/// the `time` or `chrono` crate. UTC is fine for this — the system prompt
/// just orients the model, calendar precision isn't important.
fn chrono_like_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    days_to_date_string(secs / 86_400)
}

/// Convert a count of days since the Unix epoch (1970-01-01) into a
/// human-readable "Month D, Y" string. Uses the proleptic Gregorian calendar
/// — accurate for any plausible system clock.
fn days_to_date_string(days: u64) -> String {
    // Algorithm: Howard Hinnant's "days_from_civil" inverse. Same as the
    // C++ <chrono> implementation, ported to plain integer arithmetic.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    let month_name = match m {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "",
    };
    format!("{month_name} {d}, {year}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::settings::ReasoningEffort;

    fn s_default() -> Settings {
        Settings::default()
    }

    fn bi(risk: RiskLevel) -> BashModelInput {
        BashModelInput {
            command: "ls".into(),
            explanation: "list".into(),
            risk_level: risk,
            behavior_tags: vec![],
            timeout: 30_000,
        }
    }

    #[test]
    fn read_only_auto_approved_in_normal_mode() {
        let mut s = s_default();
        s.always_confirm = false;
        // Non-TTY in the test environment; for ReadOnly + non-strict +
        // !always_confirm we should auto-approve regardless.
        let r = decide_approval(&bi(RiskLevel::ReadOnly), &s, false);
        assert!(matches!(r, BashApproval::Approved));
    }

    #[test]
    fn strict_mode_forces_prompt_path_for_read_only() {
        // In strict mode + non-TTY, decide_approval should NOT auto-approve;
        // it falls through to the rejection branch.
        let s = s_default();
        let r = decide_approval(&bi(RiskLevel::ReadOnly), &s, true);
        assert!(matches!(r, BashApproval::Rejected { .. }));
    }

    #[test]
    fn dangerous_command_rejected_in_non_tty() {
        let s = s_default();
        let r = decide_approval(&bi(RiskLevel::Dangerous), &s, false);
        match r {
            BashApproval::Rejected { message } => {
                assert!(message.is_some_and(|m| m.contains("non-interactive")));
            }
            _ => panic!("expected Rejected"),
        }
    }

    #[test]
    fn always_confirm_makes_read_only_prompt_too() {
        let mut s = s_default();
        s.always_confirm = true;
        let r = decide_approval(&bi(RiskLevel::ReadOnly), &s, false);
        assert!(matches!(r, BashApproval::Rejected { .. }));
    }

    #[test]
    fn date_string_formatting_smoke() {
        // 2024-01-01 was 19_723 days after the Unix epoch; Jul 4 of the
        // same year is 185 days later. Spot-check both anchor points so we
        // catch off-by-one in the proleptic Gregorian conversion.
        assert_eq!(days_to_date_string(19_723), "January 1, 2024");
        assert_eq!(days_to_date_string(19_723 + 185), "July 4, 2024");
        // 2027-01-01: 19_723 + 366 (2024 leap) + 365 (2025) + 365 (2026).
        assert_eq!(days_to_date_string(19_723 + 366 + 365 + 365), "January 1, 2027");
    }

    #[test]
    fn _unused_import_silencer() {
        let _ = ReasoningEffort::Med;
    }
}
