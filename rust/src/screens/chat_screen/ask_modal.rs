//! AskUser modal. One question at a time; the user can pick a numbered
//! choice or type a free-text answer. Mirrors `AskPrompt.tsx` from the TS app.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::screens::theme::{AQUA, BG_SECONDARY, FG_PRIMARY, FG_SECONDARY, YELLOW};
use crate::tools::ask::{Question, QuestionResponse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Choosing,
    Typing,
}

pub struct AskModal {
    questions: Vec<Question>,
    answers: Vec<QuestionResponse>,
    /// Current question index.
    cursor: usize,
    /// Currently focused choice index (0..choices.len() means a preset
    /// choice; len == "Type your own answer").
    focus: usize,
    mode: Mode,
    typed: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskModalOutcome {
    Done(Vec<QuestionResponse>),
    Cancelled,
}

impl AskModal {
    pub fn new(questions: Vec<Question>) -> Self {
        Self {
            answers: Vec::with_capacity(questions.len()),
            cursor: 0,
            focus: 0,
            mode: Mode::Choosing,
            typed: String::new(),
            questions,
        }
    }

    fn choices_count(&self) -> usize {
        self.questions[self.cursor].choices.len() + 1 // +1 for "Type your own answer"
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let area = centered(area, 70, 70);
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " AskUser ({}/{}): {} ",
                self.cursor + 1,
                self.questions.len(),
                self.questions[self.cursor].title
            ))
            .style(Style::default().bg(BG_SECONDARY).fg(FG_PRIMARY));
        frame.render_widget(block.clone(), area);
        let inner = block.inner(area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(inner);

        let q = &self.questions[self.cursor];
        let body = Paragraph::new(Line::from(Span::styled(
            q.question.clone(),
            Style::default().fg(FG_PRIMARY),
        )));
        frame.render_widget(body, layout[0]);

        // Choice list.
        let mut lines = Vec::new();
        for (i, c) in q.choices.iter().enumerate() {
            let prefix = if self.focus == i && self.mode == Mode::Choosing {
                "▸ "
            } else {
                "  "
            };
            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(YELLOW)),
                Span::styled(format!("{}) ", i + 1), Style::default().fg(FG_SECONDARY)),
                Span::raw(c.label.clone()),
            ];
            if let Some(desc) = &c.description {
                spans.push(Span::styled(
                    format!("  — {desc}"),
                    Style::default().fg(FG_SECONDARY),
                ));
            }
            lines.push(Line::from(spans));
        }
        let typing_focus = self.focus == q.choices.len();
        let prefix = if typing_focus { "▸ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(YELLOW)),
            Span::styled(
                "Type your own answer",
                Style::default().fg(if typing_focus { AQUA } else { FG_PRIMARY }),
            ),
        ]));
        frame.render_widget(Paragraph::new(lines), layout[1]);

        // Active typing line.
        if self.mode == Mode::Typing {
            let edit = Line::from(vec![
                Span::styled("  > ", Style::default().fg(YELLOW)),
                Span::raw(self.typed.clone()),
            ]);
            frame.render_widget(Paragraph::new(edit), layout[2]);
        }

        let footer_text = match self.mode {
            Mode::Choosing => "↑/↓: choose • Enter: confirm • Esc: cancel",
            Mode::Typing => "Enter: submit • Esc: back",
        };
        frame.render_widget(
            Paragraph::new(footer_text)
                .alignment(Alignment::Center)
                .style(Style::default().fg(FG_SECONDARY)),
            layout[3],
        );
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Option<AskModalOutcome> {
        match self.mode {
            Mode::Choosing => self.on_choose_key(key),
            Mode::Typing => self.on_type_key(key),
        }
    }

    fn on_choose_key(&mut self, key: KeyEvent) -> Option<AskModalOutcome> {
        let n = self.choices_count();
        match key.code {
            KeyCode::Up => {
                self.focus = (self.focus + n - 1) % n;
                None
            }
            KeyCode::Down => {
                self.focus = (self.focus + 1) % n;
                None
            }
            KeyCode::Esc => Some(AskModalOutcome::Cancelled),
            KeyCode::Enter => {
                let q = &self.questions[self.cursor];
                if self.focus < q.choices.len() {
                    let answer = q.choices[self.focus].label.clone();
                    self.commit(answer)
                } else {
                    self.mode = Mode::Typing;
                    self.typed.clear();
                    None
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let idx = (c as u8 - b'1') as usize;
                let q = &self.questions[self.cursor];
                if idx < q.choices.len() {
                    let answer = q.choices[idx].label.clone();
                    self.commit(answer)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn on_type_key(&mut self, key: KeyEvent) -> Option<AskModalOutcome> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Choosing;
                self.typed.clear();
                None
            }
            KeyCode::Enter => {
                if !self.typed.is_empty() {
                    let answer = std::mem::take(&mut self.typed);
                    self.mode = Mode::Choosing;
                    self.commit(answer)
                } else {
                    None
                }
            }
            KeyCode::Backspace => {
                self.typed.pop();
                None
            }
            KeyCode::Char(c) => {
                self.typed.push(c);
                None
            }
            _ => None,
        }
    }

    fn commit(&mut self, answer: String) -> Option<AskModalOutcome> {
        self.answers.push(QuestionResponse {
            question: self.questions[self.cursor].question.clone(),
            answer,
        });
        self.cursor += 1;
        self.focus = 0;
        if self.cursor >= self.questions.len() {
            Some(AskModalOutcome::Done(std::mem::take(&mut self.answers)))
        } else {
            None
        }
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
    use crate::tools::ask::QuestionChoice;

    fn questions() -> Vec<Question> {
        vec![
            Question {
                title: "Old".into(),
                question: "How old?".into(),
                choices: vec![
                    QuestionChoice {
                        label: "3 months".into(),
                        description: None,
                    },
                    QuestionChoice {
                        label: "1 year".into(),
                        description: Some("Year+".into()),
                    },
                ],
            },
            Question {
                title: "Confirm".into(),
                question: "Proceed?".into(),
                choices: vec![QuestionChoice {
                    label: "yes".into(),
                    description: None,
                }],
            },
        ]
    }

    #[test]
    fn enter_with_choice_selects_label() {
        let mut m = AskModal::new(questions());
        let out = m.on_key(KeyEvent::from(KeyCode::Enter));
        assert!(out.is_none());
        // Now on Q2.
        let out = m.on_key(KeyEvent::from(KeyCode::Enter)).unwrap();
        match out {
            AskModalOutcome::Done(answers) => {
                assert_eq!(answers[0].answer, "3 months");
                assert_eq!(answers[1].answer, "yes");
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn typing_path_records_answer() {
        let mut m = AskModal::new(questions());
        // Move focus to "Type your own answer" (index 2).
        m.on_key(KeyEvent::from(KeyCode::Down));
        m.on_key(KeyEvent::from(KeyCode::Down));
        m.on_key(KeyEvent::from(KeyCode::Enter));
        for c in "blue".chars() {
            m.on_key(KeyEvent::from(KeyCode::Char(c)));
        }
        let out = m.on_key(KeyEvent::from(KeyCode::Enter));
        assert!(out.is_none());
        let out = m.on_key(KeyEvent::from(KeyCode::Enter)).unwrap();
        match out {
            AskModalOutcome::Done(answers) => {
                assert_eq!(answers[0].answer, "blue");
                assert_eq!(answers[1].answer, "yes");
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn esc_cancels() {
        let mut m = AskModal::new(questions());
        let out = m.on_key(KeyEvent::from(KeyCode::Esc));
        assert_eq!(out, Some(AskModalOutcome::Cancelled));
    }

    #[test]
    fn digit_shortcut_picks_choice() {
        let mut m = AskModal::new(questions());
        let out = m.on_key(KeyEvent::from(KeyCode::Char('2')));
        assert!(out.is_none());
        let out = m.on_key(KeyEvent::from(KeyCode::Char('1'))).unwrap();
        match out {
            AskModalOutcome::Done(answers) => {
                assert_eq!(answers[0].answer, "1 year");
                assert_eq!(answers[1].answer, "yes");
            }
            _ => panic!("expected Done"),
        }
    }
}
