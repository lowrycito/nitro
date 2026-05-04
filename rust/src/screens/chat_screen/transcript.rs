//! Renders the message history into a scrollable Paragraph.
//!
//! Streaming deltas are buffered into a "tail" line that the screen prepends
//! when rendering, so the user sees text growing live without rebuilding the
//! whole transcript on every char.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::logic::llm::messages::{AssistantContent, AssistantPart, Message, MessageContent, ToolResultContent, UserPart};
use crate::screens::theme::{AQUA, FG_PRIMARY, FG_SECONDARY, GREEN, ORANGE, PURPLE, YELLOW};

/// In-memory transcript state.
#[derive(Default)]
pub struct Transcript {
    pub scroll: u16,
    pub show_thinking: bool,
}

impl Transcript {
    pub fn render(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        history: &[Message],
        streaming_text: &str,
        streaming_reasoning: &str,
    ) {
        let mut lines: Vec<Line<'static>> = Vec::new();
        for msg in history {
            self.append_message(&mut lines, msg);
            lines.push(Line::raw(""));
        }

        if !streaming_reasoning.is_empty() && self.show_thinking {
            lines.push(Line::from(Span::styled(
                "thinking",
                Style::default()
                    .fg(FG_SECONDARY)
                    .add_modifier(Modifier::BOLD),
            )));
            for line in streaming_reasoning.split('\n') {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(FG_SECONDARY),
                )));
            }
            lines.push(Line::raw(""));
        }
        if !streaming_text.is_empty() {
            lines.push(Line::from(Span::styled(
                "assistant",
                Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
            )));
            for line in streaming_text.split('\n') {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(FG_PRIMARY),
                )));
            }
        }

        let widget = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" chat ")
                    .style(Style::default().fg(FG_SECONDARY)),
            );
        frame.render_widget(widget, area);
    }

    fn append_message(&self, lines: &mut Vec<Line<'static>>, msg: &Message) {
        match msg {
            Message::User { content } => {
                lines.push(self.role_line("user", PURPLE));
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
                self.append_text(lines, &text, FG_PRIMARY);
            }
            Message::Assistant { content } => {
                let parts: Vec<AssistantPart> = match content {
                    AssistantContent::Text(s) => vec![AssistantPart::Text { text: s.clone() }],
                    AssistantContent::Parts(p) => p.clone(),
                };
                lines.push(self.role_line("assistant", GREEN));
                for p in parts {
                    match p {
                        AssistantPart::Text { text } => {
                            self.append_text(lines, &text, FG_PRIMARY);
                        }
                        AssistantPart::Reasoning { text } if self.show_thinking => {
                            lines.push(Line::from(Span::styled(
                                "  thinking:",
                                Style::default().fg(FG_SECONDARY),
                            )));
                            self.append_text(lines, &text, FG_SECONDARY);
                        }
                        AssistantPart::Reasoning { .. } => {}
                        AssistantPart::ToolCall {
                            tool_name, input, ..
                        } => {
                            let summary =
                                input.get("command").and_then(|v| v.as_str()).unwrap_or("");
                            lines.push(Line::from(vec![
                                Span::styled(
                                    format!("  {tool_name} → "),
                                    Style::default().fg(YELLOW),
                                ),
                                Span::styled(summary.to_string(), Style::default().fg(FG_PRIMARY)),
                            ]));
                        }
                    }
                }
            }
            Message::Tool { content } => {
                lines.push(self.role_line("tool", ORANGE));
                for r in content {
                    let tool_label = format!("  {}:", r.tool_name);
                    lines.push(Line::from(Span::styled(
                        tool_label,
                        Style::default().fg(FG_SECONDARY),
                    )));
                    let body = match &r.output {
                        ToolResultContent::Json { value } => {
                            // Try to summarise BashToolOutput; fall through
                            // to compact JSON for everything else.
                            if let Some(out) = value.get("commandOutput").and_then(|v| v.as_str()) {
                                crate::tools::bash::truncate_for_display(out)
                            } else if let Some(false) = value.get("approved").and_then(|v| v.as_bool()) {
                                "[denied]".to_string()
                            } else {
                                serde_json::to_string(value).unwrap_or_default()
                            }
                        }
                        ToolResultContent::ErrorText { value } => value.clone(),
                        ToolResultContent::ErrorJson { value } => {
                            serde_json::to_string(value).unwrap_or_default()
                        }
                    };
                    self.append_text(lines, &body, AQUA);
                }
            }
        }
    }

    fn role_line(&self, label: &'static str, color: ratatui::style::Color) -> Line<'static> {
        Line::from(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
    }

    fn append_text(&self, lines: &mut Vec<Line<'static>>, text: &str, color: ratatui::style::Color) {
        for raw in text.split('\n') {
            lines.push(Line::from(Span::styled(
                raw.to_string(),
                Style::default().fg(color),
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use serde_json::json;

    fn render_string(history: &[Message], streaming: &str, reasoning: &str) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let t = Transcript::default();
        terminal
            .draw(|f| t.render(f, f.area(), history, streaming, reasoning))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_user_and_assistant() {
        let history = vec![
            Message::user_text("hello"),
            Message::assistant_parts(vec![AssistantPart::Text {
                text: "hi back".into(),
            }]),
        ];
        let s = render_string(&history, "", "");
        assert!(s.contains("user"));
        assert!(s.contains("hello"));
        assert!(s.contains("assistant"));
        assert!(s.contains("hi back"));
    }

    #[test]
    fn shows_streaming_text() {
        let s = render_string(&[], "thinking out loud", "");
        assert!(s.contains("assistant"));
        assert!(s.contains("thinking out loud"));
    }

    #[test]
    fn tool_call_summary_includes_command() {
        let history = vec![Message::assistant_parts(vec![AssistantPart::ToolCall {
            tool_call_id: "t1".into(),
            tool_name: "Bash".into(),
            input: json!({ "command": "ls -la" }),
        }])];
        let s = render_string(&history, "", "");
        assert!(s.contains("Bash"));
        assert!(s.contains("ls -la"));
    }

    #[test]
    fn reasoning_hidden_when_show_thinking_off() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let t = Transcript {
            show_thinking: false,
            ..Transcript::default()
        };
        let history = vec![Message::assistant_parts(vec![
            AssistantPart::Reasoning {
                text: "secret thoughts".into(),
            },
            AssistantPart::Text {
                text: "answer".into(),
            },
        ])];
        terminal
            .draw(|f| t.render(f, f.area(), &history, "", ""))
            .unwrap();
        let buf = terminal.backend().buffer();
        let s: String = (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .map(|(x, y)| buf[(x, y)].symbol().to_string())
            .collect();
        assert!(!s.contains("secret thoughts"));
        assert!(s.contains("answer"));
    }
}
