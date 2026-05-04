//! BashPrompt modal. Mirrors `src/components/bash/BashPrompt.tsx`.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::screens::theme::{AQUA, BG_SECONDARY, FG_PRIMARY, FG_SECONDARY, RED, YELLOW};
use crate::tools::bash::{BashApproval, BashModelInput, RiskLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Approve,
    Reject,
    Cancel,
}

const ACTIONS: &[Action] = &[Action::Approve, Action::Reject, Action::Cancel];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Choosing,
    EditingMessage,
}

pub struct BashModal {
    pub model_input: BashModelInput,
    focus: usize,
    mode: Mode,
    rejection_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BashModalOutcome {
    Decided(BashApproval),
    Cancelled,
}

impl BashModal {
    pub fn new(model_input: BashModelInput) -> Self {
        Self {
            model_input,
            focus: 0,
            mode: Mode::Choosing,
            rejection_message: String::new(),
        }
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        // Centered overlay box.
        let modal_area = centered(area, 60, 60);
        frame.render_widget(Clear, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Run command? ")
            .style(Style::default().bg(BG_SECONDARY).fg(FG_PRIMARY));
        frame.render_widget(block.clone(), modal_area);

        let inner = block.inner(modal_area);
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(inner);

        let header = vec![
            Line::from(vec![
                Span::styled("$ ", Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)),
                Span::styled(
                    self.model_input.command.clone(),
                    Style::default().fg(FG_PRIMARY),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                self.model_input.explanation.clone(),
                Style::default().fg(FG_SECONDARY),
            )),
        ];
        frame.render_widget(Paragraph::new(header).wrap(Wrap { trim: false }), layout[0]);

        let risk_color = risk_color(self.model_input.risk_level);
        let mut tag_spans: Vec<Span> = vec![Span::styled(
            format!("risk: {}", risk_label(self.model_input.risk_level)),
            Style::default().fg(risk_color).add_modifier(Modifier::BOLD),
        )];
        if !self.model_input.behavior_tags.is_empty() {
            let tags: Vec<String> = self
                .model_input
                .behavior_tags
                .iter()
                .map(|t| {
                    serde_json::to_string(t)
                        .unwrap_or_default()
                        .trim_matches('"')
                        .to_string()
                })
                .collect();
            tag_spans.push(Span::raw("   "));
            tag_spans.push(Span::styled(
                format!("[{}]", tags.join(", ")),
                Style::default().fg(FG_SECONDARY),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(tag_spans)), layout[1]);

        // Edit-mode rejection message input.
        if self.mode == Mode::EditingMessage {
            let edit = Paragraph::new(Line::from(vec![
                Span::styled("Rejection reason: ", Style::default().fg(YELLOW)),
                Span::styled(
                    self.rejection_message.clone(),
                    Style::default().fg(FG_PRIMARY),
                ),
            ]));
            frame.render_widget(edit, layout[2]);
        } else {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "↑/↓: choose • Enter: confirm • Esc: cancel",
                    Style::default().fg(FG_SECONDARY),
                )),
                layout[2],
            );
        }

        self.render_actions(frame, layout[3]);
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Option<BashModalOutcome> {
        match self.mode {
            Mode::Choosing => self.on_key_choosing(key),
            Mode::EditingMessage => self.on_key_editing(key),
        }
    }

    fn on_key_choosing(&mut self, key: KeyEvent) -> Option<BashModalOutcome> {
        match key.code {
            KeyCode::Up => {
                self.focus = (self.focus + ACTIONS.len() - 1) % ACTIONS.len();
                None
            }
            KeyCode::Down => {
                self.focus = (self.focus + 1) % ACTIONS.len();
                None
            }
            KeyCode::Esc => Some(BashModalOutcome::Cancelled),
            KeyCode::Enter => match ACTIONS[self.focus] {
                Action::Approve => Some(BashModalOutcome::Decided(BashApproval::Approved)),
                Action::Cancel => Some(BashModalOutcome::Cancelled),
                Action::Reject => {
                    self.mode = Mode::EditingMessage;
                    None
                }
            },
            _ => None,
        }
    }

    fn on_key_editing(&mut self, key: KeyEvent) -> Option<BashModalOutcome> {
        match key.code {
            KeyCode::Esc => {
                self.rejection_message.clear();
                self.mode = Mode::Choosing;
                None
            }
            KeyCode::Enter => {
                let msg = if self.rejection_message.is_empty() {
                    None
                } else {
                    Some(std::mem::take(&mut self.rejection_message))
                };
                Some(BashModalOutcome::Decided(BashApproval::Rejected { message: msg }))
            }
            KeyCode::Backspace => {
                self.rejection_message.pop();
                None
            }
            KeyCode::Char(c) => {
                self.rejection_message.push(c);
                None
            }
            _ => None,
        }
    }

    fn render_actions(&self, frame: &mut Frame<'_>, area: Rect) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(34),
                Constraint::Percentage(33),
            ])
            .split(area);
        let labels = [
            ("Approve and Run", AQUA),
            ("Reject with Message", YELLOW),
            ("Cancel and Exit", RED),
        ];
        for (i, (label, color)) in labels.iter().enumerate() {
            let focused = i == self.focus;
            let style = if focused {
                Style::default()
                    .fg(*color)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(*color)
            };
            let text = if focused {
                format!("> {label} <")
            } else {
                format!("  {label}  ")
            };
            frame.render_widget(
                Paragraph::new(text).alignment(Alignment::Center).style(style),
                cols[i],
            );
        }
    }
}

fn risk_label(r: RiskLevel) -> &'static str {
    match r {
        RiskLevel::ReadOnly => "Read Only",
        RiskLevel::Normal => "Normal",
        RiskLevel::Dangerous => "Dangerous",
        RiskLevel::ExtremelyDangerous => "Extremely Dangerous",
    }
}

fn risk_color(r: RiskLevel) -> ratatui::style::Color {
    match r {
        RiskLevel::ReadOnly => AQUA,
        RiskLevel::Normal => YELLOW,
        RiskLevel::Dangerous => crate::screens::theme::ORANGE,
        RiskLevel::ExtremelyDangerous => RED,
    }
}

fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1]);
    h[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::bash::BehaviorTag;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn input() -> BashModelInput {
        BashModelInput {
            command: "rm -rf folder".into(),
            explanation: "Delete a directory".into(),
            risk_level: RiskLevel::ExtremelyDangerous,
            behavior_tags: vec![BehaviorTag::Delete],
            timeout: 30_000,
        }
    }

    fn render_string(modal: &BashModal) -> String {
        // Wider than 80 so the action labels render in full; 80×24 truncates
        // "Approve and Run" to ~15 chars, which the snapshot expects intact.
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| modal.render(f, f.area())).unwrap();
        let buf = terminal.backend().buffer();
        (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| buf[(x, y)].symbol().to_string()))
            .collect::<Vec<_>>()
            .chunks(buf.area.width as usize)
            .map(|c| c.join(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn shows_command_explanation_risk_and_actions() {
        let m = BashModal::new(input());
        let s = render_string(&m);
        assert!(s.contains("rm -rf folder"));
        assert!(s.contains("Delete a directory"));
        assert!(s.contains("Extremely Dangerous"));
        assert!(s.contains("Approve and Run"));
        assert!(s.contains("Reject with Message"));
        assert!(s.contains("Cancel and Exit"));
        assert!(s.contains("Delete"));
    }

    #[test]
    fn approve_returns_approved() {
        let mut m = BashModal::new(input());
        let out = m.on_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(out, Some(BashModalOutcome::Decided(BashApproval::Approved)));
    }

    #[test]
    fn cancel_via_esc_returns_cancelled() {
        let mut m = BashModal::new(input());
        let out = m.on_key(KeyEvent::from(KeyCode::Esc));
        assert_eq!(out, Some(BashModalOutcome::Cancelled));
    }

    #[test]
    fn reject_path_collects_message() {
        let mut m = BashModal::new(input());
        m.on_key(KeyEvent::from(KeyCode::Down));
        let out = m.on_key(KeyEvent::from(KeyCode::Enter));
        assert!(out.is_none()); // entered editing mode
        for c in "no thanks".chars() {
            m.on_key(KeyEvent::from(KeyCode::Char(c)));
        }
        let out = m.on_key(KeyEvent::from(KeyCode::Enter)).unwrap();
        match out {
            BashModalOutcome::Decided(BashApproval::Rejected { message }) => {
                assert_eq!(message.as_deref(), Some("no thanks"));
            }
            _ => panic!("expected Rejected"),
        }
    }

    #[test]
    fn reject_with_empty_message_returns_none() {
        let mut m = BashModal::new(input());
        m.on_key(KeyEvent::from(KeyCode::Down));
        m.on_key(KeyEvent::from(KeyCode::Enter)); // enter edit mode
        let out = m.on_key(KeyEvent::from(KeyCode::Enter)).unwrap();
        match out {
            BashModalOutcome::Decided(BashApproval::Rejected { message }) => {
                assert_eq!(message, None);
            }
            _ => panic!("expected Rejected"),
        }
    }

    #[test]
    fn esc_in_edit_returns_to_choose_mode() {
        let mut m = BashModal::new(input());
        m.on_key(KeyEvent::from(KeyCode::Down));
        m.on_key(KeyEvent::from(KeyCode::Enter));
        for c in "abc".chars() {
            m.on_key(KeyEvent::from(KeyCode::Char(c)));
        }
        let out = m.on_key(KeyEvent::from(KeyCode::Esc));
        assert_eq!(out, None);
        assert!(m.rejection_message.is_empty());
    }

    #[test]
    fn arrow_keys_wrap_focus() {
        let mut m = BashModal::new(input());
        assert_eq!(m.focus, 0);
        m.on_key(KeyEvent::from(KeyCode::Up));
        assert_eq!(m.focus, ACTIONS.len() - 1);
        m.on_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(m.focus, 0);
    }
}
