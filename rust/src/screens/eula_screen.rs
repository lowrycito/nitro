//! EULA agreement screen.
//!
//! Mirrors `src/screens/EulaScreen.tsx`. Displays the EULA text in a
//! scrollable block plus two buttons (Accept / Decline). Acceptance is
//! persisted by the caller — this screen returns the user's choice only.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::app_shell::{run_screen, Screen, Step};
use super::theme::{AQUA, BG_PRIMARY, FG_PRIMARY, FG_SECONDARY, RED};
use crate::logic::eula::EULA_TEXT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EulaOutcome {
    Accepted,
    Declined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Accept,
    Decline,
}

pub struct EulaScreen {
    focus: Focus,
    scroll: u16,
}

impl Default for EulaScreen {
    fn default() -> Self {
        Self {
            focus: Focus::Decline,
            scroll: 0,
        }
    }
}

impl Screen for EulaScreen {
    type Output = EulaOutcome;

    fn render(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let outer = Block::default().style(Style::default().bg(BG_PRIMARY).fg(FG_PRIMARY));
        frame.render_widget(outer, area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            "Nitro — End User License Agreement",
            Style::default().add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center);
        frame.render_widget(title, layout[0]);

        let body = Paragraph::new(EULA_TEXT)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::default().fg(FG_SECONDARY)),
            );
        frame.render_widget(body, layout[1]);

        self.render_buttons(frame, layout[2]);

        let footer = Paragraph::new("←/→: switch • Enter: confirm • ↑/↓: scroll • q: quit")
            .alignment(Alignment::Center)
            .style(Style::default().fg(FG_SECONDARY));
        frame.render_widget(footer, layout[3]);
    }

    fn on_key(&mut self, key: KeyEvent) -> Step<Self::Output> {
        match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Accept => Focus::Decline,
                    Focus::Decline => Focus::Accept,
                };
                Step::Continue
            }
            KeyCode::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                Step::Continue
            }
            KeyCode::Down => {
                self.scroll = self.scroll.saturating_add(1);
                Step::Continue
            }
            KeyCode::Char('q') | KeyCode::Esc => Step::Done(EulaOutcome::Declined),
            KeyCode::Enter => Step::Done(match self.focus {
                Focus::Accept => EulaOutcome::Accepted,
                Focus::Decline => EulaOutcome::Declined,
            }),
            _ => Step::Continue,
        }
    }
}

impl EulaScreen {
    fn render_buttons(&self, frame: &mut Frame<'_>, area: Rect) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        frame.render_widget(self.button("Decline", Focus::Decline, RED), cols[0]);
        frame.render_widget(self.button("Accept", Focus::Accept, AQUA), cols[1]);
    }

    fn button(
        &self,
        label: &'static str,
        target: Focus,
        color: ratatui::style::Color,
    ) -> Paragraph<'_> {
        let focused = self.focus == target;
        let style = if focused {
            Style::default()
                .fg(color)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(color)
        };
        let label = if focused {
            format!("> {label} <")
        } else {
            format!("  {label}  ")
        };
        Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(style)
    }
}

/// Public entry point — sets up the terminal and runs the screen until the
/// user picks an outcome.
pub fn run_eula_screen() -> std::io::Result<EulaOutcome> {
    run_screen(EulaScreen::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_to_string(mut screen: EulaScreen) -> String {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| screen.render(f)).unwrap();
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
    fn renders_eula_text_and_buttons() {
        let s = render_to_string(EulaScreen::default());
        assert!(s.contains("End User License Agreement"));
        assert!(s.contains("Accept"));
        assert!(s.contains("Decline"));
    }

    #[test]
    fn left_right_switches_focus() {
        let mut s = EulaScreen::default();
        assert_eq!(s.focus, Focus::Decline);
        s.on_key(KeyEvent::from(KeyCode::Right));
        assert_eq!(s.focus, Focus::Accept);
        s.on_key(KeyEvent::from(KeyCode::Left));
        assert_eq!(s.focus, Focus::Decline);
    }

    #[test]
    fn enter_returns_focused_choice() {
        let mut s = EulaScreen::default();
        s.on_key(KeyEvent::from(KeyCode::Right));
        let res = s.on_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(res, Step::Done(EulaOutcome::Accepted)));

        let mut s = EulaScreen::default();
        let res = s.on_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(res, Step::Done(EulaOutcome::Declined)));
    }

    #[test]
    fn esc_declines() {
        let mut s = EulaScreen::default();
        s.on_key(KeyEvent::from(KeyCode::Right));
        let res = s.on_key(KeyEvent::from(KeyCode::Esc));
        assert!(matches!(res, Step::Done(EulaOutcome::Declined)));
    }

    #[test]
    fn up_and_down_scroll_body() {
        let mut s = EulaScreen::default();
        assert_eq!(s.scroll, 0);
        s.on_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(s.scroll, 1);
        s.on_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(s.scroll, 2);
        s.on_key(KeyEvent::from(KeyCode::Up));
        assert_eq!(s.scroll, 1);
    }
}
