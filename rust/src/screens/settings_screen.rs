//! Settings menu. Mirrors `src/screens/SettingsScreen.tsx`.
//!
//! Field metadata is the same set of toggles/numbers/selects exposed in the
//! TS app. Persistence runs through `logic::settings::save_settings` so
//! every change is written to the same `~/.nitro/settings.json` either
//! binary uses.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::app_shell::{run_screen, Screen, Step};
use super::theme::{AQUA, BG_PRIMARY, FG_PRIMARY, FG_SECONDARY, YELLOW};
use crate::logic::settings::{load_settings, save_settings, ReasoningEffort, Settings};

pub fn run_settings_screen(data_dir: PathBuf) -> std::io::Result<()> {
    let initial = load_settings(&data_dir)?;
    let mut screen = SettingsScreen::new(data_dir, initial);
    run_screen(&mut screen as &mut SettingsScreen)?;
    Ok(())
}

// `&mut S` doesn't impl `Screen`; expose a wrapper.
impl Screen for &mut SettingsScreen {
    type Output = ();
    fn render(&mut self, frame: &mut Frame<'_>) {
        SettingsScreen::render(self, frame);
    }
    fn on_key(&mut self, key: KeyEvent) -> Step<Self::Output> {
        SettingsScreen::on_key(self, key)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    Bool,
    Int,
    ReasoningEffort,
}

struct FieldMeta {
    label: &'static str,
    description: &'static str,
    kind: FieldKind,
}

const FIELDS: &[FieldMeta] = &[
    FieldMeta {
        label: "Always Confirm",
        description: "Prompt for confirmation before all commands",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        label: "Show Thinking",
        description: "Show AI thinking/summary for supported models",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        label: "Show Token Summary",
        description: "Display token usage summary on session exit",
        kind: FieldKind::Bool,
    },
    FieldMeta {
        label: "Max Output Tokens",
        description: "Maximum output tokens for model responses",
        kind: FieldKind::Int,
    },
    FieldMeta {
        label: "Reasoning Effort",
        description: "How much the model reasons before responding",
        kind: FieldKind::ReasoningEffort,
    },
];

#[derive(Debug)]
enum Mode {
    Browsing,
    EditingInt { buffer: String, original: u32 },
    Status(String),
}

pub struct SettingsScreen {
    data_dir: PathBuf,
    settings: Settings,
    cursor: usize,
    list_state: ListState,
    mode: Mode,
}

impl SettingsScreen {
    fn new(data_dir: PathBuf, settings: Settings) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            data_dir,
            settings,
            cursor: 0,
            list_state,
            mode: Mode::Browsing,
        }
    }

    fn current_value(&self, idx: usize) -> String {
        let field = &FIELDS[idx];
        match field.kind {
            FieldKind::Bool => match field.label {
                "Always Confirm" => bool_str(self.settings.always_confirm),
                "Show Thinking" => bool_str(self.settings.show_thinking),
                "Show Token Summary" => bool_str(self.settings.show_token_summary),
                _ => bool_str(false),
            },
            FieldKind::Int => self.settings.max_output_tokens.to_string(),
            FieldKind::ReasoningEffort => effort_str(self.settings.reasoning_effort).to_string(),
        }
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
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

        frame.render_widget(
            Paragraph::new("Nitro — Settings")
                .style(Style::default().add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center),
            layout[0],
        );

        let items: Vec<ListItem> = FIELDS
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let value = self.current_value(i);
                let line = Line::from(vec![
                    Span::styled(
                        format!("  {:<22}", f.label),
                        Style::default().fg(FG_PRIMARY),
                    ),
                    Span::styled(value, Style::default().fg(YELLOW)),
                ]);
                ListItem::new(line)
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" settings ")
                    .style(Style::default().fg(FG_SECONDARY)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED).fg(AQUA));
        frame.render_stateful_widget(list, layout[1], &mut self.list_state);

        self.render_helper(frame, layout[2]);

        frame.render_widget(
            Paragraph::new("↑/↓: select • Enter: edit • s: save & quit • q: quit (no save)")
                .alignment(Alignment::Center)
                .style(Style::default().fg(FG_SECONDARY)),
            layout[3],
        );
    }

    fn render_helper(&self, frame: &mut Frame<'_>, area: Rect) {
        let field = &FIELDS[self.cursor];
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", field.label));
        let body = match &self.mode {
            Mode::Browsing => Line::from(field.description),
            Mode::EditingInt { buffer, .. } => Line::from(vec![
                Span::raw("New value: "),
                Span::styled(buffer.clone(), Style::default().fg(YELLOW)),
                Span::raw(" (Enter to save, Esc to cancel)"),
            ]),
            Mode::Status(msg) => Line::from(Span::styled(msg.clone(), Style::default().fg(AQUA))),
        };
        frame.render_widget(Paragraph::new(body).block(block), area);
    }

    fn on_key(&mut self, key: KeyEvent) -> Step<()> {
        match &mut self.mode {
            Mode::EditingInt { buffer, original } => match key.code {
                KeyCode::Esc => {
                    self.mode = Mode::Browsing;
                    Step::Continue
                }
                KeyCode::Enter => {
                    if let Ok(parsed) = buffer.parse::<u32>() {
                        if parsed > 0 {
                            self.settings.max_output_tokens = parsed;
                            self.mode = Mode::Status("updated (unsaved)".into());
                            return Step::Continue;
                        }
                    }
                    let original = *original;
                    self.settings.max_output_tokens = original;
                    self.mode = Mode::Status("invalid value, reverted".into());
                    Step::Continue
                }
                KeyCode::Backspace => {
                    buffer.pop();
                    Step::Continue
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    buffer.push(c);
                    Step::Continue
                }
                _ => Step::Continue,
            },
            Mode::Browsing | Mode::Status(_) => match key.code {
                KeyCode::Up => {
                    if self.cursor > 0 {
                        self.cursor -= 1;
                        self.list_state.select(Some(self.cursor));
                    }
                    self.mode = Mode::Browsing;
                    Step::Continue
                }
                KeyCode::Down => {
                    if self.cursor + 1 < FIELDS.len() {
                        self.cursor += 1;
                        self.list_state.select(Some(self.cursor));
                    }
                    self.mode = Mode::Browsing;
                    Step::Continue
                }
                KeyCode::Enter => {
                    self.activate_current();
                    Step::Continue
                }
                KeyCode::Char(' ') => {
                    if FIELDS[self.cursor].kind == FieldKind::Bool {
                        self.toggle_bool();
                    } else {
                        self.activate_current();
                    }
                    Step::Continue
                }
                KeyCode::Char('s') => {
                    if let Err(e) = save_settings(&self.data_dir, &self.settings) {
                        self.mode = Mode::Status(format!("save failed: {e}"));
                        Step::Continue
                    } else {
                        Step::Done(())
                    }
                }
                KeyCode::Char('q') | KeyCode::Esc => Step::Done(()),
                _ => Step::Continue,
            },
        }
    }

    fn activate_current(&mut self) {
        let field = &FIELDS[self.cursor];
        match field.kind {
            FieldKind::Bool => self.toggle_bool(),
            FieldKind::ReasoningEffort => self.cycle_effort(),
            FieldKind::Int => {
                self.mode = Mode::EditingInt {
                    buffer: self.settings.max_output_tokens.to_string(),
                    original: self.settings.max_output_tokens,
                };
            }
        }
    }

    fn toggle_bool(&mut self) {
        match FIELDS[self.cursor].label {
            "Always Confirm" => self.settings.always_confirm = !self.settings.always_confirm,
            "Show Thinking" => self.settings.show_thinking = !self.settings.show_thinking,
            "Show Token Summary" => {
                self.settings.show_token_summary = !self.settings.show_token_summary
            }
            _ => {}
        }
        self.mode = Mode::Status("updated (unsaved)".into());
    }

    fn cycle_effort(&mut self) {
        self.settings.reasoning_effort = match self.settings.reasoning_effort {
            ReasoningEffort::Low => ReasoningEffort::Med,
            ReasoningEffort::Med => ReasoningEffort::High,
            ReasoningEffort::High => ReasoningEffort::Low,
        };
        self.mode = Mode::Status("updated (unsaved)".into());
    }
}

fn bool_str(b: bool) -> String {
    if b {
        "on".to_string()
    } else {
        "off".to_string()
    }
}

fn effort_str(e: ReasoningEffort) -> &'static str {
    match e {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Med => "med",
        ReasoningEffort::High => "high",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use tempfile::TempDir;

    fn rendered(screen: &mut SettingsScreen) -> String {
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
    fn renders_all_field_labels() {
        let tmp = TempDir::new().unwrap();
        let mut s = SettingsScreen::new(tmp.path().to_path_buf(), Settings::default());
        let txt = rendered(&mut s);
        for f in FIELDS {
            assert!(txt.contains(f.label), "missing field label: {}", f.label);
        }
    }

    #[test]
    fn space_toggles_boolean_field() {
        let tmp = TempDir::new().unwrap();
        let mut s = SettingsScreen::new(tmp.path().to_path_buf(), Settings::default());
        // Cursor starts at "Always Confirm".
        assert!(!s.settings.always_confirm);
        s.on_key(KeyEvent::from(KeyCode::Char(' ')));
        assert!(s.settings.always_confirm);
        s.on_key(KeyEvent::from(KeyCode::Char(' ')));
        assert!(!s.settings.always_confirm);
    }

    #[test]
    fn cycle_through_reasoning_effort() {
        let tmp = TempDir::new().unwrap();
        let mut s = SettingsScreen::new(tmp.path().to_path_buf(), Settings::default());
        for _ in 0..(FIELDS.len() - 1) {
            s.on_key(KeyEvent::from(KeyCode::Down));
        }
        assert_eq!(s.cursor, FIELDS.len() - 1);
        assert_eq!(s.settings.reasoning_effort, ReasoningEffort::Med);
        s.on_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(s.settings.reasoning_effort, ReasoningEffort::High);
        s.on_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(s.settings.reasoning_effort, ReasoningEffort::Low);
    }

    #[test]
    fn editing_int_buffer_accepts_digits_only() {
        let tmp = TempDir::new().unwrap();
        let mut s = SettingsScreen::new(tmp.path().to_path_buf(), Settings::default());
        // Move to "Max Output Tokens" (index 3 in FIELDS).
        for _ in 0..3 {
            s.on_key(KeyEvent::from(KeyCode::Down));
        }
        s.on_key(KeyEvent::from(KeyCode::Enter));
        // Buffer initialised to "16000"; remove digits and type new value.
        for _ in 0..5 {
            s.on_key(KeyEvent::from(KeyCode::Backspace));
        }
        for c in "1234".chars() {
            s.on_key(KeyEvent::from(KeyCode::Char(c)));
        }
        // Letter a: ignored.
        s.on_key(KeyEvent::from(KeyCode::Char('a')));
        s.on_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(s.settings.max_output_tokens, 1234);
    }

    #[test]
    fn s_persists_settings_to_disk() {
        let tmp = TempDir::new().unwrap();
        let mut s = SettingsScreen::new(tmp.path().to_path_buf(), Settings::default());
        s.on_key(KeyEvent::from(KeyCode::Char(' ')));
        let res = s.on_key(KeyEvent::from(KeyCode::Char('s')));
        assert!(matches!(res, Step::Done(())));
        let reloaded = load_settings(tmp.path()).unwrap();
        assert!(reloaded.always_confirm);
    }

    #[test]
    fn q_quits_without_saving() {
        let tmp = TempDir::new().unwrap();
        let initial = Settings::default();
        save_settings(tmp.path(), &initial).unwrap();
        let mut s = SettingsScreen::new(tmp.path().to_path_buf(), initial);
        s.on_key(KeyEvent::from(KeyCode::Char(' ')));
        let res = s.on_key(KeyEvent::from(KeyCode::Char('q')));
        assert!(matches!(res, Step::Done(())));
        let reloaded = load_settings(tmp.path()).unwrap();
        assert!(!reloaded.always_confirm, "in-memory toggle leaked to disk");
    }
}
