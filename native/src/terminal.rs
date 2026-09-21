use crossterm::{
    cursor::Show,
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io::{self, Stdout};

pub struct Screen {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    active: bool,
}
pub fn restore() {
    let _ = execute!(
        io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
    let _ = disable_raw_mode();
}
impl Screen {
    pub fn new() -> io::Result<Self> {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            hook(info);
        }));
        let mut screen = Self {
            terminal: Terminal::new(CrosstermBackend::new(io::stdout()))?,
            active: false,
        };
        screen.enter()?;
        Ok(screen)
    }
    pub fn enter(&mut self) -> io::Result<()> {
        self.active = true;
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        self.terminal.clear()
    }
    pub fn suspend(&mut self) {
        if self.active {
            restore();
            self.active = false;
        }
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        self.suspend();
    }
}
