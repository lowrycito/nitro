//! Interactive chat screen — the main UI for `nitro interactive`,
//! `nitro resume`, and (once Phase 8 lands) `nitro <request>` for users who
//! want the streaming TUI for one-off requests too.
//!
//! ## Design
//!
//! The screen runs an async event loop that merges three event sources via a
//! single `mpsc::Receiver`:
//! - terminal events (crossterm) collected by a blocking task
//! - LLM stream events when a turn is in flight
//! - tool-execution completion events
//!
//! Each `tick` of the loop the screen redraws, pulls the next event, and
//! mutates state. State is intentionally kept linear (one `Mode` field)
//! rather than spread across booleans so it's easy to reason about which
//! key bindings are valid at any moment.

mod ask_modal;
mod bash_modal;
mod state;
mod transcript;

use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use super::app_shell::AppTerminal;
use crate::logic::llm::messages::Message;
use crate::logic::llm::stream::StreamItem;
use crate::logic::settings::Settings;
use crate::logic::provider::NamedProvider;

pub use state::{ChatScreen, ChatScreenAction};

/// Configuration handed in by the dispatcher.
#[derive(Debug, Clone)]
pub struct ChatScreenConfig {
    pub data_dir: PathBuf,
    pub provider: NamedProvider,
    pub settings: Settings,
    pub system_prompt: String,
    pub strict: bool,
    pub initial_request: String,
    pub initial_filename: Option<String>,
    pub hide_previous_messages: bool,
}

/// Events delivered to the chat loop.
#[derive(Debug)]
pub enum ChatEvent {
    Term(Event),
    Stream(StreamItem),
    StreamEnd,
    ToolDone(state::ToolDoneEvent),
}

/// Run the chat screen until the user quits.
pub async fn run_chat_screen(config: ChatScreenConfig) -> std::io::Result<()> {
    install_terminal()?;
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_chat_screen_inner(&mut terminal, config).await;

    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = terminal.show_cursor();
    result
}

fn install_terminal() -> std::io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen
    )?;
    Ok(())
}

async fn run_chat_screen_inner(
    terminal: &mut AppTerminal,
    config: ChatScreenConfig,
) -> std::io::Result<()> {
    let (tx, mut rx) = mpsc::channel::<ChatEvent>(64);

    // Spawn the terminal-event collector. spawn_blocking is the right
    // primitive because crossterm's `event::poll` is sync.
    let term_tx = tx.clone();
    let term_handle = tokio::task::spawn_blocking(move || {
        while !term_tx.is_closed() {
            if let Ok(true) = event::poll(Duration::from_millis(75)) {
                let ev = match event::read() {
                    Ok(ev) => ev,
                    Err(_) => continue,
                };
                if term_tx.blocking_send(ChatEvent::Term(ev)).is_err() {
                    break;
                }
            }
        }
    });

    let mut screen = ChatScreen::new(config, tx.clone());
    if let Err(e) = screen.bootstrap().await {
        crate::app::console::error(&format!("Error: {e}"));
    }

    loop {
        terminal.draw(|f| screen.render(f))?;
        let Some(event) = rx.recv().await else {
            break;
        };
        match screen.handle(event).await {
            ChatScreenAction::Continue => {}
            ChatScreenAction::Quit => break,
        }
    }

    drop(tx);
    let _ = term_handle.await;
    Ok(())
}

// Re-export some types tests + dispatcher need.
pub use bash_modal::BashModal;
pub use ask_modal::AskModal;
pub use transcript::Transcript;

#[allow(dead_code)]
fn _types_used(_m: Message) {}
