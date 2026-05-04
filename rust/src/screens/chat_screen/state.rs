//! Chat-screen state machine.
//!
//! `Mode` captures what the user can do at any moment; the event handler
//! dispatches based on it. Streaming runs in a separate tokio task and
//! sends [`StreamEvent`]s back via the same `mpsc` the terminal events
//! flow through.

use crossterm::event::{Event, KeyCode, KeyEvent};
use futures::StreamExt;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use serde_json::Value;
use tokio::sync::mpsc;

use super::{ask_modal::AskModal, bash_modal::BashModal, transcript::Transcript, ChatEvent};
use super::ask_modal::AskModalOutcome;
use super::bash_modal::BashModalOutcome;
use crate::logic::conversation::{load_conversation, save_conversation};
use crate::logic::llm::messages::{Message, ToolCall, ToolResult, ToolResultContent};
use crate::logic::llm::stream::{StreamEvent, Usage};
use crate::logic::llm::transcripts::AssistantBuilder;
use crate::logic::llm::{stream_completion, GenerationOptions, ToolDef};
use crate::screens::theme::{BG_PRIMARY, FG_PRIMARY, FG_SECONDARY, YELLOW};
use crate::tools::ask::{AskTool, Question};
use crate::tools::bash::{BashModelInput, BashTool, RiskLevel};

#[derive(Debug, Clone)]
pub struct ToolDoneEvent {
    pub call_id: String,
    pub result: ToolResult,
    /// When the user `Cancel and Exit`s a Bash modal we propagate that
    /// signal up so the chat screen can quit cleanly.
    pub cancel_session: bool,
}

#[derive(Debug)]
pub enum ChatScreenAction {
    Continue,
    Quit,
}

enum Mode {
    Idle,
    Streaming,
    AwaitingTool {
        pending: Vec<ToolCall>,
        results: Vec<ToolResult>,
        // Boxed because Bash/Ask modal structs are bulky (Vec, BashModelInput…)
        // and we don't want every Mode variant to inherit their stack size.
        active_modal: Box<ActiveModal>,
    },
}

enum ActiveModal {
    Bash(BashModal, ToolCall),
    Ask(AskModal, ToolCall),
    None,
}

pub struct ChatScreen {
    cfg: super::ChatScreenConfig,
    tx: mpsc::Sender<ChatEvent>,
    history: Vec<Message>,
    transcript: Transcript,
    builder: AssistantBuilder,
    streaming_text: String,
    streaming_reasoning: String,
    input: String,
    mode: Mode,
    filename: Option<String>,
    total_usage: Usage,
    status: String,
}

impl ChatScreen {
    pub fn new(cfg: super::ChatScreenConfig, tx: mpsc::Sender<ChatEvent>) -> Self {
        let transcript = Transcript {
            scroll: 0,
            show_thinking: cfg.settings.show_thinking,
        };
        Self {
            history: Vec::new(),
            transcript,
            builder: AssistantBuilder::new(),
            streaming_text: String::new(),
            streaming_reasoning: String::new(),
            input: String::new(),
            mode: Mode::Idle,
            filename: cfg.initial_filename.clone(),
            total_usage: Usage::default(),
            status: String::new(),
            cfg,
            tx,
        }
    }

    /// Load any prior conversation and kick off the initial request, if one
    /// was supplied on the CLI (`nitro interactive "..."`).
    pub async fn bootstrap(&mut self) -> Result<(), String> {
        if let Some(name) = self.cfg.initial_filename.clone() {
            if let Some(c) = load_conversation(&self.cfg.data_dir, &name) {
                self.history = c
                    .messages
                    .into_iter()
                    .filter_map(|v| serde_json::from_value::<Message>(v).ok())
                    .collect();
                if self.cfg.hide_previous_messages {
                    self.history.clear();
                }
            }
        }
        if !self.cfg.initial_request.is_empty() {
            let req = std::mem::take(&mut self.cfg.initial_request);
            self.send_user_message(req).await;
        }
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame<'_>) {
        // Background.
        let area = frame.area();
        let outer = Block::default().style(Style::default().bg(BG_PRIMARY).fg(FG_PRIMARY));
        frame.render_widget(outer, area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(5),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

        // Header
        let mut header_spans = vec![
            Span::styled("Nitro", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  • "),
            Span::styled(
                self.cfg.provider.name.clone(),
                Style::default().fg(YELLOW),
            ),
            Span::raw("  • "),
            Span::styled(self.mode_label(), Style::default().fg(FG_SECONDARY)),
        ];
        if self.cfg.settings.show_token_summary
            && (self.total_usage.input_tokens > 0 || self.total_usage.output_tokens > 0)
        {
            header_spans.push(Span::raw("  • "));
            header_spans.push(Span::styled(
                format!(
                    "tok in={} out={} cache={}",
                    self.total_usage.input_tokens,
                    self.total_usage.output_tokens,
                    self.total_usage.cached_input_tokens,
                ),
                Style::default().fg(FG_SECONDARY),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(header_spans)), layout[0]);

        self.transcript.render(
            frame,
            layout[1],
            &self.history,
            &self.streaming_text,
            &self.streaming_reasoning,
        );

        // Input
        let input_block = Block::default()
            .borders(Borders::ALL)
            .title(if matches!(self.mode, Mode::Idle) {
                " message (Enter to send, Esc to quit) "
            } else {
                " (busy) "
            })
            .style(Style::default().fg(FG_SECONDARY));
        let input = Paragraph::new(self.input.as_str()).block(input_block);
        frame.render_widget(input, layout[2]);

        let footer_text = if self.status.is_empty() {
            String::from("Ctrl-C: quit • PgUp/PgDn: scroll")
        } else {
            self.status.clone()
        };
        let footer = Paragraph::new(footer_text).style(Style::default().fg(FG_SECONDARY));
        frame.render_widget(footer, layout[3]);

        // Modal overlays.
        if let Mode::AwaitingTool { active_modal, .. } = &self.mode {
            match active_modal.as_ref() {
                ActiveModal::Bash(m, _) => m.render(frame, area),
                ActiveModal::Ask(m, _) => m.render(frame, area),
                ActiveModal::None => {}
            }
        }
    }

    pub async fn handle(&mut self, ev: ChatEvent) -> ChatScreenAction {
        match ev {
            ChatEvent::Term(e) => self.handle_term(e).await,
            ChatEvent::Stream(item) => match item {
                Ok(e) => {
                    self.handle_stream(e).await;
                    ChatScreenAction::Continue
                }
                Err(e) => {
                    self.status = format!("error: {e}");
                    self.finish_assistant_turn().await;
                    ChatScreenAction::Continue
                }
            },
            ChatEvent::StreamEnd => {
                self.finish_assistant_turn().await;
                ChatScreenAction::Continue
            }
            ChatEvent::ToolDone(d) => {
                if d.cancel_session {
                    return ChatScreenAction::Quit;
                }
                self.on_tool_done(d).await;
                ChatScreenAction::Continue
            }
        }
    }

    async fn handle_term(&mut self, ev: Event) -> ChatScreenAction {
        if let Event::Key(key) = ev {
            // Ctrl-C is always quit.
            if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)
                && key.code == KeyCode::Char('c')
            {
                return ChatScreenAction::Quit;
            }
            match &mut self.mode {
                Mode::Idle => return self.handle_idle_key(key).await,
                Mode::Streaming => {
                    // Allow scrolling during streaming; ignore typing.
                    self.handle_scroll_key(key);
                }
                Mode::AwaitingTool { .. } => {
                    return self.handle_modal_key(key).await;
                }
            }
        }
        ChatScreenAction::Continue
    }

    async fn handle_idle_key(&mut self, key: KeyEvent) -> ChatScreenAction {
        match key.code {
            KeyCode::Esc => return ChatScreenAction::Quit,
            KeyCode::Enter => {
                if !self.input.is_empty() {
                    let msg = std::mem::take(&mut self.input);
                    self.send_user_message(msg).await;
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => {
                self.input.push(c);
            }
            _ => self.handle_scroll_key(key),
        }
        ChatScreenAction::Continue
    }

    fn handle_scroll_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::PageUp => {
                self.transcript.scroll = self.transcript.scroll.saturating_sub(5);
            }
            KeyCode::PageDown => {
                self.transcript.scroll = self.transcript.scroll.saturating_add(5);
            }
            _ => {}
        }
    }

    async fn handle_modal_key(&mut self, key: KeyEvent) -> ChatScreenAction {
        // Pull the modal out, dispatch, decide what to do next.
        let outcome: Option<ModalOutcome> = match &mut self.mode {
            Mode::AwaitingTool { active_modal, .. } => match active_modal.as_mut() {
                ActiveModal::Bash(m, _) => m.on_key(key).map(ModalOutcome::Bash),
                ActiveModal::Ask(m, _) => m.on_key(key).map(ModalOutcome::Ask),
                ActiveModal::None => None,
            },
            _ => None,
        };

        if let Some(outcome) = outcome {
            self.consume_modal_outcome(outcome).await;
        }
        ChatScreenAction::Continue
    }

    async fn consume_modal_outcome(&mut self, outcome: ModalOutcome) {
        let (call, result, cancel) = match outcome {
            ModalOutcome::Bash(BashModalOutcome::Decided(approval)) => {
                let call = match &self.mode {
                    Mode::AwaitingTool { active_modal, .. } => match active_modal.as_ref() {
                        ActiveModal::Bash(_, c) => c.clone(),
                        _ => return,
                    },
                    _ => return,
                };
                let model_input: BashModelInput = match serde_json::from_value(call.input.clone())
                {
                    Ok(v) => v,
                    Err(e) => {
                        let r = ToolResult {
                            kind: "tool-result".into(),
                            tool_call_id: call.tool_call_id.clone(),
                            tool_name: "Bash".into(),
                            output: ToolResultContent::ErrorText {
                                value: format!("Bash tool call failed validation: {e}"),
                            },
                        };
                        return self.add_result_and_advance(call, r).await;
                    }
                };
                let output = BashTool::execute(&model_input, approval).await;
                let result = ToolResult {
                    kind: "tool-result".into(),
                    tool_call_id: call.tool_call_id.clone(),
                    tool_name: "Bash".into(),
                    output: ToolResultContent::Json {
                        value: serde_json::to_value(&output).unwrap_or(Value::Null),
                    },
                };
                (call, result, false)
            }
            ModalOutcome::Bash(BashModalOutcome::Cancelled) => {
                // Mirror the TS "Cancel and Exit" behaviour.
                let _ = self.tx.try_send(ChatEvent::ToolDone(ToolDoneEvent {
                    call_id: String::new(),
                    result: ToolResult {
                        kind: "tool-result".into(),
                        tool_call_id: String::new(),
                        tool_name: "Bash".into(),
                        output: ToolResultContent::ErrorText {
                            value: "cancelled".into(),
                        },
                    },
                    cancel_session: true,
                }));
                return;
            }
            ModalOutcome::Ask(AskModalOutcome::Done(answers)) => {
                let call = match &self.mode {
                    Mode::AwaitingTool { active_modal, .. } => match active_modal.as_ref() {
                        ActiveModal::Ask(_, c) => c.clone(),
                        _ => return,
                    },
                    _ => return,
                };
                let out = AskTool::execute(answers);
                let result = ToolResult {
                    kind: "tool-result".into(),
                    tool_call_id: call.tool_call_id.clone(),
                    tool_name: "AskUser".into(),
                    output: ToolResultContent::Json {
                        value: serde_json::to_value(&out).unwrap_or(Value::Null),
                    },
                };
                (call, result, false)
            }
            ModalOutcome::Ask(AskModalOutcome::Cancelled) => {
                let _ = self.tx.try_send(ChatEvent::ToolDone(ToolDoneEvent {
                    call_id: String::new(),
                    result: ToolResult {
                        kind: "tool-result".into(),
                        tool_call_id: String::new(),
                        tool_name: "AskUser".into(),
                        output: ToolResultContent::ErrorText {
                            value: "cancelled".into(),
                        },
                    },
                    cancel_session: true,
                }));
                return;
            }
        };
        if cancel {
            return;
        }
        self.add_result_and_advance(call, result).await;
    }

    async fn add_result_and_advance(&mut self, _call: ToolCall, result: ToolResult) {
        let next_modal = match &mut self.mode {
            Mode::AwaitingTool {
                pending,
                results,
                active_modal,
            } => {
                results.push(result);
                **active_modal = ActiveModal::None;
                pending.first().cloned()
            }
            _ => return,
        };

        if let Some(next) = next_modal {
            self.activate_next_modal(next).await;
        } else if let Mode::AwaitingTool { results, .. } = &mut self.mode {
            // All tool calls satisfied. Send the tool message and start the
            // next assistant turn.
            let collected = std::mem::take(results);
            self.history.push(Message::tool_results(collected));
            self.persist();
            self.mode = Mode::Idle;
            self.spawn_assistant_turn().await;
        }
    }

    async fn activate_next_modal(&mut self, call: ToolCall) {
        if let Mode::AwaitingTool { pending, active_modal, .. } = &mut self.mode {
            if !pending.is_empty() {
                pending.remove(0);
            }
            **active_modal = build_modal_for(&call);
        }
    }

    async fn handle_stream(&mut self, ev: StreamEvent) {
        match ev {
            StreamEvent::TextDelta(t) => {
                self.streaming_text.push_str(&t);
                self.builder.push_text(&t);
            }
            StreamEvent::ReasoningDelta(t) => {
                self.streaming_reasoning.push_str(&t);
                self.builder.push_reasoning(&t);
            }
            StreamEvent::ToolCall(c) => {
                self.builder.push_tool_call(c);
            }
            StreamEvent::Usage(u) => {
                self.builder.usage = u;
                self.total_usage.add(&u);
            }
            StreamEvent::Done => {
                let _ = self.tx.try_send(ChatEvent::StreamEnd);
            }
        }
    }

    async fn finish_assistant_turn(&mut self) {
        let builder = std::mem::take(&mut self.builder);
        let (assistant_msg, tool_calls, _) = builder.finish();
        self.streaming_text.clear();
        self.streaming_reasoning.clear();
        self.history.push(assistant_msg);
        self.persist();

        if tool_calls.is_empty() {
            self.mode = Mode::Idle;
            return;
        }

        let mut pending: Vec<ToolCall> = tool_calls.into_iter().collect();
        let first = pending.remove(0);
        let modal = build_modal_for(&first);
        self.mode = Mode::AwaitingTool {
            pending,
            results: Vec::new(),
            active_modal: Box::new(modal),
        };
    }

    async fn send_user_message(&mut self, text: String) {
        self.history.push(Message::user_text(text));
        self.persist();
        self.spawn_assistant_turn().await;
    }

    async fn spawn_assistant_turn(&mut self) {
        self.builder = AssistantBuilder::new();
        self.streaming_text.clear();
        self.streaming_reasoning.clear();
        self.mode = Mode::Streaming;

        let provider = self.cfg.provider.clone();
        let messages = self.history.clone();
        let system_prompt = self.cfg.system_prompt.clone();
        let opts = GenerationOptions {
            max_output_tokens: Some(self.cfg.settings.max_output_tokens),
            reasoning_effort: Some(self.cfg.settings.reasoning_effort),
        };
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

        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut stream =
                stream_completion(&provider, &messages, &system_prompt, &tools, &opts).await;
            while let Some(item) = stream.next().await {
                if tx.send(ChatEvent::Stream(item)).await.is_err() {
                    return;
                }
            }
            let _ = tx.send(ChatEvent::StreamEnd).await;
        });
    }

    async fn on_tool_done(&mut self, _ev: ToolDoneEvent) {
        // Already handled inline by `add_result_and_advance`. This event is
        // only used to carry session-cancel signals from modal cancellations.
    }

    fn persist(&mut self) {
        let serialised: Vec<Value> = self
            .history
            .iter()
            .filter_map(|m| serde_json::to_value(m).ok())
            .collect();
        match save_conversation(&self.cfg.data_dir, &serialised, self.filename.as_deref()) {
            Ok(name) => self.filename = Some(name),
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    fn mode_label(&self) -> &'static str {
        match self.mode {
            Mode::Idle => "idle",
            Mode::Streaming => "streaming…",
            Mode::AwaitingTool { .. } => "tool approval",
        }
    }
}

enum ModalOutcome {
    Bash(BashModalOutcome),
    Ask(AskModalOutcome),
}

fn build_modal_for(call: &ToolCall) -> ActiveModal {
    match call.tool_name.as_str() {
        "Bash" => match serde_json::from_value::<BashModelInput>(call.input.clone()) {
            Ok(model_input) => ActiveModal::Bash(BashModal::new(model_input), call.clone()),
            Err(_) => ActiveModal::Bash(
                BashModal::new(BashModelInput {
                    command: "(invalid input)".into(),
                    explanation: "tool call failed validation".into(),
                    risk_level: RiskLevel::Dangerous,
                    behavior_tags: vec![],
                    timeout: 0,
                }),
                call.clone(),
            ),
        },
        "AskUser" => {
            #[derive(serde::Deserialize)]
            struct Wrap {
                questions: Vec<Question>,
            }
            match serde_json::from_value::<Wrap>(call.input.clone()) {
                Ok(w) => ActiveModal::Ask(AskModal::new(w.questions), call.clone()),
                Err(_) => ActiveModal::Ask(AskModal::new(Vec::new()), call.clone()),
            }
        }
        _ => ActiveModal::None,
    }
}

