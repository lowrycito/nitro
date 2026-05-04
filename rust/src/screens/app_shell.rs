//! Terminal lifecycle and a tiny event-loop runner.
//!
//! Each screen is a struct that implements [`Screen`]. The runner owns the
//! terminal — enabling raw mode + alternate screen on entry, restoring on
//! exit, and installing a panic hook so an unwinding panic doesn't leave
//! the user's terminal in raw mode.

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

pub type AppTerminal = Terminal<CrosstermBackend<Stdout>>;

/// Result of a screen's `update` method. `Continue` re-renders; `Done`
/// returns the screen-specific output to the caller.
pub enum Step<T> {
    Continue,
    Done(T),
}

pub trait Screen {
    type Output;
    fn render(&mut self, frame: &mut ratatui::Frame<'_>);
    /// Handle a key event. Non-key events (resize, mouse, paste) are
    /// ignored unless overridden.
    fn on_key(&mut self, key: KeyEvent) -> Step<Self::Output>;
    fn on_event(&mut self, event: Event) -> Step<Self::Output> {
        match event {
            Event::Key(k) => self.on_key(k),
            _ => Step::Continue,
        }
    }
}

/// Set up the terminal, run a screen until it produces output, and tear
/// down regardless of how the screen finished. Errors mid-flight are
/// surfaced after the terminal is restored.
pub fn run_screen<S: Screen>(mut screen: S) -> io::Result<S::Output> {
    install_panic_hook();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let outcome = run_screen_inner(&mut screen, &mut terminal);

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    outcome
}

fn run_screen_inner<S: Screen>(
    screen: &mut S,
    terminal: &mut AppTerminal,
) -> io::Result<S::Output> {
    loop {
        terminal.draw(|frame| screen.render(frame))?;
        // Poll briefly so resize events refresh the UI even when the user
        // isn't typing. 100ms is fast enough that the UI feels live but
        // doesn't burn CPU.
        if event::poll(Duration::from_millis(100))? {
            let ev = event::read()?;
            if let Step::Done(out) = screen.on_event(ev) {
                return Ok(out);
            }
        }
    }
}

fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen);
        original(info);
    }));
}
