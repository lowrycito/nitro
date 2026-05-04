//! Provider-management screens. Mirrors the TS files under
//! `src/screens/Provider*`.
//!
//! Five user-visible screens are merged into this module so they can share
//! the small form widgets without re-exposing them publicly:
//! - [`ProviderListScreen`]: read-only listing with the default marker
//! - [`ProviderAddScreen`] / [`ProviderEditScreen`]: wizard
//!   (name → baseURL → apiKey → apiType → model)
//! - [`ProviderRemoveScreen`] and [`ProviderDefaultScreen`]: pick-from-list

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::app_shell::{run_screen, Screen, Step};
use super::theme::{AQUA, BG_PRIMARY, FG_PRIMARY, FG_SECONDARY, RED, YELLOW};
use crate::logic::defaults::DEFAULT_PROVIDERS;
use crate::logic::provider::{
    get_default_provider, get_provider, list_providers, remove_provider, set_default_provider,
    set_provider, ApiType, ProviderInfo,
};

const API_TYPES: &[(ApiType, &str)] = &[
    (ApiType::OpenAiCompatible, "openai-compatible"),
    (ApiType::OpenAiResponses, "openai-responses"),
    (ApiType::Anthropic, "anthropic"),
];

// ----------------------------------------------------------------------------
// Provider list (read-only)
// ----------------------------------------------------------------------------

pub struct ProviderListScreen {
    items: Vec<(String, bool)>, // (name, is_default)
    list_state: ListState,
}

impl ProviderListScreen {
    pub fn new(data_dir: &std::path::Path) -> std::io::Result<Self> {
        let names = list_providers(data_dir)?;
        let default = get_default_provider(data_dir)
            .ok()
            .flatten()
            .map(|d| d.name);
        let items: Vec<_> = names
            .into_iter()
            .map(|n| (n.clone(), Some(&n) == default.as_ref()))
            .collect();
        let mut list_state = ListState::default();
        list_state.select(if items.is_empty() { None } else { Some(0) });
        Ok(Self { items, list_state })
    }
}

impl Screen for ProviderListScreen {
    type Output = ();
    fn render(&mut self, frame: &mut Frame<'_>) {
        background(frame);
        let area = frame.area();
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);
        title(frame, layout[0], "Nitro — Providers");

        if self.items.is_empty() {
            frame.render_widget(
                Paragraph::new("No providers configured. Press 'a' or run `nitro provider add`.")
                    .alignment(Alignment::Center),
                layout[1],
            );
        } else {
            let items: Vec<ListItem> = self
                .items
                .iter()
                .map(|(name, is_default)| {
                    let mut spans = vec![Span::raw(format!("  {name}"))];
                    if *is_default {
                        spans.push(Span::styled(
                            "  (default)",
                            Style::default().fg(AQUA).add_modifier(Modifier::BOLD),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let widget = List::new(items)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" providers ")
                        .style(Style::default().fg(FG_SECONDARY)),
                )
                .highlight_style(Style::default().fg(YELLOW).add_modifier(Modifier::REVERSED));
            frame.render_stateful_widget(widget, layout[1], &mut self.list_state);
        }

        footer(frame, layout[2], "↑/↓: select • Enter/q: close");
    }
    fn on_key(&mut self, key: KeyEvent) -> Step<Self::Output> {
        match key.code {
            KeyCode::Up => {
                if let Some(i) = self.list_state.selected() {
                    if i > 0 {
                        self.list_state.select(Some(i - 1));
                    }
                }
                Step::Continue
            }
            KeyCode::Down => {
                if let Some(i) = self.list_state.selected() {
                    if i + 1 < self.items.len() {
                        self.list_state.select(Some(i + 1));
                    }
                }
                Step::Continue
            }
            KeyCode::Enter | KeyCode::Char('q') | KeyCode::Esc => Step::Done(()),
            _ => Step::Continue,
        }
    }
}

pub fn run_provider_list_screen(data_dir: PathBuf) -> std::io::Result<()> {
    let screen = ProviderListScreen::new(&data_dir)?;
    run_screen(screen)
}

// ----------------------------------------------------------------------------
// Add / edit wizard
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WizardStep {
    Name,
    BaseUrl,
    ApiKey,
    ApiType,
    Model,
    Confirm,
}

pub struct ProviderWizardScreen {
    data_dir: PathBuf,
    /// `None` for add; `Some(name)` for edit.
    editing: Option<String>,
    step: WizardStep,
    // Form state.
    name: String,
    base_url: String,
    api_key: String,
    api_type_idx: usize,
    model: String,
    error: Option<String>,
    saved: bool,
}

impl ProviderWizardScreen {
    pub fn new_add(data_dir: PathBuf) -> Self {
        Self::base(
            data_dir,
            None,
            ProviderInfo {
                base_url: String::new(),
                api_key: String::new(),
                model: String::new(),
                api_type: ApiType::OpenAiCompatible,
            },
        )
    }

    pub fn new_edit(data_dir: PathBuf, name: String, info: ProviderInfo) -> Self {
        let mut s = Self::base(data_dir, Some(name.clone()), info);
        s.name = name;
        s
    }

    fn base(data_dir: PathBuf, editing: Option<String>, info: ProviderInfo) -> Self {
        Self {
            data_dir,
            editing,
            step: WizardStep::Name,
            name: String::new(),
            base_url: info.base_url,
            api_key: info.api_key,
            api_type_idx: API_TYPES
                .iter()
                .position(|(t, _)| *t == info.api_type)
                .unwrap_or(0),
            model: info.model,
            error: None,
            saved: false,
        }
    }

    fn current_buffer(&mut self) -> Option<&mut String> {
        match self.step {
            WizardStep::Name => Some(&mut self.name),
            WizardStep::BaseUrl => Some(&mut self.base_url),
            WizardStep::ApiKey => Some(&mut self.api_key),
            WizardStep::Model => Some(&mut self.model),
            WizardStep::ApiType | WizardStep::Confirm => None,
        }
    }

    fn next_step(&mut self) {
        self.error = None;
        self.step = match self.step {
            WizardStep::Name => {
                if self.name.is_empty() {
                    self.error = Some("Name is required.".into());
                    return;
                }
                // Apply default base URL when the chosen name matches a preset.
                if self.base_url.is_empty() {
                    if let Some(p) = DEFAULT_PROVIDERS.iter().find(|p| p.name == self.name) {
                        self.base_url = p.base_url.to_string();
                        self.api_type_idx = API_TYPES
                            .iter()
                            .position(|(t, _)| *t == p.api_type)
                            .unwrap_or(0);
                    }
                }
                WizardStep::BaseUrl
            }
            WizardStep::BaseUrl => {
                if self.base_url.is_empty() {
                    self.error = Some("Base URL is required.".into());
                    return;
                }
                WizardStep::ApiKey
            }
            WizardStep::ApiKey => {
                if self.api_key.is_empty() {
                    self.error = Some("API key is required.".into());
                    return;
                }
                WizardStep::ApiType
            }
            WizardStep::ApiType => WizardStep::Model,
            WizardStep::Model => {
                if self.model.is_empty() {
                    self.error = Some("Model is required.".into());
                    return;
                }
                WizardStep::Confirm
            }
            WizardStep::Confirm => WizardStep::Confirm,
        };
    }

    fn prev_step(&mut self) {
        self.error = None;
        self.step = match self.step {
            WizardStep::Name => WizardStep::Name,
            WizardStep::BaseUrl => WizardStep::Name,
            WizardStep::ApiKey => WizardStep::BaseUrl,
            WizardStep::ApiType => WizardStep::ApiKey,
            WizardStep::Model => WizardStep::ApiType,
            WizardStep::Confirm => WizardStep::Model,
        };
    }

    fn save(&mut self) -> Result<(), String> {
        let info = ProviderInfo {
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
            api_type: API_TYPES[self.api_type_idx].0,
        };
        // Rename support for edit: if the user changed `name`, remove the old
        // entry, then insert the new one with the new key.
        if let Some(old) = &self.editing {
            if *old != self.name {
                let _ = remove_provider(&self.data_dir, old);
            }
        }
        set_provider(&self.data_dir, &self.name, info).map_err(|e| format!("save failed: {e}"))?;
        self.saved = true;
        Ok(())
    }
}

impl Screen for ProviderWizardScreen {
    type Output = ();
    fn render(&mut self, frame: &mut Frame<'_>) {
        background(frame);
        let area = frame.area();
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(2),
                Constraint::Min(5),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(area);

        let title_text = if self.editing.is_some() {
            format!(
                "Nitro — Edit provider: {}",
                self.editing.as_deref().unwrap_or("")
            )
        } else {
            "Nitro — Add provider".to_string()
        };
        title(frame, layout[0], &title_text);

        let progress = format!(
            "Step {}/{}: {}",
            step_index(self.step) + 1,
            6,
            step_label(self.step),
        );
        frame.render_widget(
            Paragraph::new(progress).style(Style::default().fg(FG_SECONDARY)),
            layout[1],
        );

        self.render_step(frame, layout[2]);

        if let Some(err) = &self.error {
            frame.render_widget(
                Paragraph::new(Span::styled(err.clone(), Style::default().fg(RED))),
                layout[3],
            );
        }
        footer(
            frame,
            layout[4],
            "Enter: next • Esc: back • F2: save (on Confirm) • q: quit",
        );
    }

    fn on_key(&mut self, key: KeyEvent) -> Step<Self::Output> {
        match key.code {
            KeyCode::Esc => {
                if self.step == WizardStep::Name {
                    return Step::Done(());
                }
                self.prev_step();
                Step::Continue
            }
            KeyCode::Char('q') if !matches!(self.step, WizardStep::Confirm) => Step::Done(()),
            KeyCode::F(2) if self.step == WizardStep::Confirm => match self.save() {
                Ok(()) => Step::Done(()),
                Err(e) => {
                    self.error = Some(e);
                    Step::Continue
                }
            },
            KeyCode::Enter => {
                self.next_step();
                Step::Continue
            }
            KeyCode::Backspace => {
                if let Some(buf) = self.current_buffer() {
                    buf.pop();
                }
                Step::Continue
            }
            KeyCode::Char(c) => {
                if self.step == WizardStep::ApiType {
                    match c {
                        'j' | ' ' => {
                            self.api_type_idx = (self.api_type_idx + 1) % API_TYPES.len();
                        }
                        'k' => {
                            self.api_type_idx =
                                (self.api_type_idx + API_TYPES.len() - 1) % API_TYPES.len();
                        }
                        _ => {}
                    }
                } else if let Some(buf) = self.current_buffer() {
                    buf.push(c);
                }
                Step::Continue
            }
            KeyCode::Up if self.step == WizardStep::ApiType => {
                self.api_type_idx = (self.api_type_idx + API_TYPES.len() - 1) % API_TYPES.len();
                Step::Continue
            }
            KeyCode::Down if self.step == WizardStep::ApiType => {
                self.api_type_idx = (self.api_type_idx + 1) % API_TYPES.len();
                Step::Continue
            }
            _ => Step::Continue,
        }
    }
}

impl ProviderWizardScreen {
    fn render_step(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(FG_SECONDARY));
        match self.step {
            WizardStep::Name => render_text_field(frame, area, block, "Name", &self.name),
            WizardStep::BaseUrl => {
                render_text_field(frame, area, block, "Base URL", &self.base_url)
            }
            WizardStep::ApiKey => {
                let masked = "*".repeat(self.api_key.chars().count());
                render_text_field(frame, area, block, "API key", &masked)
            }
            WizardStep::Model => render_text_field(frame, area, block, "Model", &self.model),
            WizardStep::ApiType => render_select(frame, area, block, self.api_type_idx),
            WizardStep::Confirm => self.render_confirm(frame, area, block),
        }
    }

    fn render_confirm(&self, frame: &mut Frame<'_>, area: Rect, block: Block) {
        let masked = "*".repeat(self.api_key.chars().count());
        let lines = vec![
            Line::from(vec![
                Span::styled("name:     ", Style::default().fg(FG_SECONDARY)),
                Span::raw(self.name.clone()),
            ]),
            Line::from(vec![
                Span::styled("baseURL:  ", Style::default().fg(FG_SECONDARY)),
                Span::raw(self.base_url.clone()),
            ]),
            Line::from(vec![
                Span::styled("apiKey:   ", Style::default().fg(FG_SECONDARY)),
                Span::raw(masked),
            ]),
            Line::from(vec![
                Span::styled("apiType:  ", Style::default().fg(FG_SECONDARY)),
                Span::raw(API_TYPES[self.api_type_idx].1),
            ]),
            Line::from(vec![
                Span::styled("model:    ", Style::default().fg(FG_SECONDARY)),
                Span::raw(self.model.clone()),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Press F2 to save, Esc to go back.",
                Style::default().fg(YELLOW),
            )),
        ];
        frame.render_widget(Paragraph::new(lines).block(block.title(" confirm ")), area);
    }
}

fn render_text_field(frame: &mut Frame<'_>, area: Rect, block: Block, label: &str, value: &str) {
    let body = Paragraph::new(vec![
        Line::from(Span::styled(
            label.to_string(),
            Style::default().fg(FG_SECONDARY),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("> {value}"),
            Style::default().fg(FG_PRIMARY),
        )),
    ])
    .block(block);
    frame.render_widget(body, area);
}

fn render_select(frame: &mut Frame<'_>, area: Rect, block: Block, idx: usize) {
    let lines: Vec<Line> = API_TYPES
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            let (prefix, style) = if i == idx {
                (
                    "▸ ",
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                )
            } else {
                ("  ", Style::default().fg(FG_PRIMARY))
            };
            Line::from(vec![
                Span::raw(prefix),
                Span::styled((*label).to_string(), style),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(block.title(" apiType (↑/↓ to select) ")),
        area,
    );
}

fn step_index(s: WizardStep) -> usize {
    match s {
        WizardStep::Name => 0,
        WizardStep::BaseUrl => 1,
        WizardStep::ApiKey => 2,
        WizardStep::ApiType => 3,
        WizardStep::Model => 4,
        WizardStep::Confirm => 5,
    }
}

fn step_label(s: WizardStep) -> &'static str {
    match s {
        WizardStep::Name => "Name",
        WizardStep::BaseUrl => "Base URL",
        WizardStep::ApiKey => "API Key",
        WizardStep::ApiType => "API Type",
        WizardStep::Model => "Model",
        WizardStep::Confirm => "Confirm",
    }
}

pub fn run_provider_add_screen(data_dir: PathBuf) -> std::io::Result<()> {
    run_screen(ProviderWizardScreen::new_add(data_dir))
}

pub fn run_provider_edit_screen(data_dir: PathBuf) -> std::io::Result<()> {
    let names = list_providers(&data_dir)?;
    if names.is_empty() {
        // Nothing to edit; route through pick screen which will display the
        // empty-list message.
        return Ok(());
    }
    if let Some(name) = pick_provider(data_dir.clone(), "Pick a provider to edit")? {
        if let Some(info) = get_provider(&data_dir, &name)? {
            run_screen(ProviderWizardScreen::new_edit(data_dir, name, info))?;
        }
    }
    Ok(())
}

pub fn run_provider_remove_screen(data_dir: PathBuf) -> std::io::Result<()> {
    if let Some(name) = pick_provider(data_dir.clone(), "Pick a provider to remove")? {
        let _ = remove_provider(&data_dir, &name);
    }
    Ok(())
}

pub fn run_provider_default_screen(data_dir: PathBuf) -> std::io::Result<()> {
    if let Some(name) = pick_provider(data_dir.clone(), "Pick a default provider")? {
        let _ = set_default_provider(&data_dir, &name);
    }
    Ok(())
}

// ----------------------------------------------------------------------------
// Pick-from-list helper used by remove / default / edit
// ----------------------------------------------------------------------------

pub struct PickProviderScreen {
    title: String,
    items: Vec<String>,
    list_state: ListState,
    chosen: Option<String>,
}

impl PickProviderScreen {
    pub fn new(title: &str, items: Vec<String>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(if items.is_empty() { None } else { Some(0) });
        Self {
            title: title.to_string(),
            items,
            list_state,
            chosen: None,
        }
    }
}

impl Screen for PickProviderScreen {
    type Output = Option<String>;
    fn render(&mut self, frame: &mut Frame<'_>) {
        background(frame);
        let area = frame.area();
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);
        title(frame, layout[0], &self.title);
        if self.items.is_empty() {
            frame.render_widget(
                Paragraph::new("No providers configured.").alignment(Alignment::Center),
                layout[1],
            );
        } else {
            let items: Vec<ListItem> = self
                .items
                .iter()
                .map(|n| ListItem::new(format!("  {n}")))
                .collect();
            frame.render_stateful_widget(
                List::new(items)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .style(Style::default().fg(FG_SECONDARY)),
                    )
                    .highlight_style(Style::default().fg(YELLOW).add_modifier(Modifier::REVERSED)),
                layout[1],
                &mut self.list_state,
            );
        }
        footer(
            frame,
            layout[2],
            "↑/↓: select • Enter: confirm • Esc/q: cancel",
        );
    }
    fn on_key(&mut self, key: KeyEvent) -> Step<Self::Output> {
        match key.code {
            KeyCode::Up => {
                if let Some(i) = self.list_state.selected() {
                    if i > 0 {
                        self.list_state.select(Some(i - 1));
                    }
                }
                Step::Continue
            }
            KeyCode::Down => {
                if let Some(i) = self.list_state.selected() {
                    if i + 1 < self.items.len() {
                        self.list_state.select(Some(i + 1));
                    }
                }
                Step::Continue
            }
            KeyCode::Enter => {
                if let Some(i) = self.list_state.selected() {
                    self.chosen = self.items.get(i).cloned();
                }
                Step::Done(self.chosen.clone())
            }
            KeyCode::Esc | KeyCode::Char('q') => Step::Done(None),
            _ => Step::Continue,
        }
    }
}

fn pick_provider(data_dir: PathBuf, title: &str) -> std::io::Result<Option<String>> {
    let names = list_providers(&data_dir)?;
    let screen = PickProviderScreen::new(title, names);
    run_screen(screen)
}

// ----------------------------------------------------------------------------
// Shared rendering helpers
// ----------------------------------------------------------------------------

fn background(frame: &mut Frame<'_>) {
    let area = frame.area();
    let outer = Block::default().style(Style::default().bg(BG_PRIMARY).fg(FG_PRIMARY));
    frame.render_widget(outer, area);
}

fn title(frame: &mut Frame<'_>, area: Rect, text: &str) {
    let p = Paragraph::new(text.to_string())
        .style(Style::default().add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center);
    frame.render_widget(p, area);
}

fn footer(frame: &mut Frame<'_>, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(FG_SECONDARY)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use tempfile::TempDir;

    fn dir_with(items: &[(&str, ApiType)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        for (name, api) in items {
            set_provider(
                tmp.path(),
                name,
                ProviderInfo {
                    base_url: "https://example.com".into(),
                    api_key: "k".into(),
                    model: "m".into(),
                    api_type: *api,
                },
            )
            .unwrap();
        }
        tmp
    }

    fn render(screen: &mut impl Screen) -> String {
        let backend = TestBackend::new(80, 24);
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
    fn list_screen_marks_default() {
        let tmp = dir_with(&[
            ("openai", ApiType::OpenAiCompatible),
            ("anthropic", ApiType::Anthropic),
        ]);
        set_default_provider(tmp.path(), "anthropic").unwrap();
        let mut screen = ProviderListScreen::new(tmp.path()).unwrap();
        let s = render(&mut screen);
        assert!(s.contains("openai"));
        assert!(s.contains("anthropic"));
        assert!(s.contains("(default)"));
    }

    #[test]
    fn add_wizard_advances_through_all_steps_and_saves() {
        let tmp = TempDir::new().unwrap();
        let mut s = ProviderWizardScreen::new_add(tmp.path().to_path_buf());

        // Type name.
        for c in "anthropic".chars() {
            s.on_key(KeyEvent::from(KeyCode::Char(c)));
        }
        s.on_key(KeyEvent::from(KeyCode::Enter));
        // The 'anthropic' preset auto-fills baseURL + apiType.
        assert_eq!(s.base_url, "https://api.anthropic.com/v1");
        assert_eq!(API_TYPES[s.api_type_idx].0, ApiType::Anthropic);
        assert_eq!(s.step, WizardStep::BaseUrl);

        s.on_key(KeyEvent::from(KeyCode::Enter)); // accept base url
        for c in "sk".chars() {
            s.on_key(KeyEvent::from(KeyCode::Char(c)));
        }
        s.on_key(KeyEvent::from(KeyCode::Enter)); // accept apiKey
        s.on_key(KeyEvent::from(KeyCode::Enter)); // skip apiType (still Anthropic)
        for c in "claude".chars() {
            s.on_key(KeyEvent::from(KeyCode::Char(c)));
        }
        s.on_key(KeyEvent::from(KeyCode::Enter)); // accept model -> confirm
        assert_eq!(s.step, WizardStep::Confirm);

        let res = s.on_key(KeyEvent::from(KeyCode::F(2)));
        assert!(matches!(res, Step::Done(())));
        // Persisted on disk in TS-compatible shape.
        let info = get_provider(tmp.path(), "anthropic").unwrap().unwrap();
        assert_eq!(info.base_url, "https://api.anthropic.com/v1");
        assert_eq!(info.api_key, "sk");
        assert_eq!(info.model, "claude");
        assert_eq!(info.api_type, ApiType::Anthropic);
    }

    #[test]
    fn add_wizard_blocks_empty_required_fields() {
        let tmp = TempDir::new().unwrap();
        let mut s = ProviderWizardScreen::new_add(tmp.path().to_path_buf());
        s.on_key(KeyEvent::from(KeyCode::Enter)); // Empty name -> error.
        assert_eq!(s.step, WizardStep::Name);
        assert!(s.error.is_some());
    }

    #[test]
    fn esc_steps_back_through_wizard() {
        let tmp = TempDir::new().unwrap();
        let mut s = ProviderWizardScreen::new_add(tmp.path().to_path_buf());
        for c in "x".chars() {
            s.on_key(KeyEvent::from(KeyCode::Char(c)));
        }
        s.on_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(s.step, WizardStep::BaseUrl);
        s.on_key(KeyEvent::from(KeyCode::Esc));
        assert_eq!(s.step, WizardStep::Name);
    }

    #[test]
    fn pick_screen_returns_selected_name() {
        let mut s = PickProviderScreen::new(
            "Pick",
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        s.on_key(KeyEvent::from(KeyCode::Down));
        let res = s.on_key(KeyEvent::from(KeyCode::Enter));
        match res {
            Step::Done(Some(name)) => assert_eq!(name, "b"),
            _ => panic!("expected b"),
        }
    }

    #[test]
    fn pick_screen_esc_returns_none() {
        let mut s = PickProviderScreen::new("Pick", vec!["a".to_string()]);
        let res = s.on_key(KeyEvent::from(KeyCode::Esc));
        assert!(matches!(res, Step::Done(None)));
    }

    #[test]
    fn pick_screen_with_empty_list_renders_message() {
        let mut s = PickProviderScreen::new("Pick", vec![]);
        let txt = render(&mut s);
        assert!(txt.contains("No providers configured."));
    }
}
