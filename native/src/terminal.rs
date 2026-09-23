use crossterm::{
    cursor::Show,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{self, Stdout},
    time::Duration,
};

pub struct Screen {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    active: bool,
}
pub fn restore() {
    // Pop before leaving: Kitty keeps separate stacks per screen, and a
    // launched tool must not inherit the encoding. Unsupported on Windows.
    let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
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
        // Zellij forwards Kitty's disambiguated keys, so Ctrl+I differs from
        // Tab. Windows console input distinguishes them without this.
        let _ = execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
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

/// Wait up to `timeout` for terminal input; true once the terminal hung up.
/// Crossterm 0.29 loops forever reading EOF from a closed terminal, ignoring
/// its timeout and our stop flag, so it must never be polled after a hang-up.
#[cfg(unix)]
pub fn hung_up(timeout: Duration) -> io::Result<bool> {
    let mut fd = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    // SAFETY: one valid pollfd for the duration of the call.
    if unsafe { libc::poll(&mut fd, 1, ms) } < 0 {
        let error = io::Error::last_os_error();
        return match error.kind() {
            io::ErrorKind::Interrupted => Ok(false),
            _ => Err(error),
        };
    }
    Ok(fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0)
}
