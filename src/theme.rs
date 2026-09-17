use ratatui::style::{Color, Style};
// Source: design/launchpad.html; deliberately no terminal chrome or tagline.
pub const BG: Color = Color::Rgb(0x10, 0x13, 0x10);
pub const SURFACE: Color = Color::Rgb(0x17, 0x1b, 0x17);
pub const RAISED: Color = Color::Rgb(0x20, 0x26, 0x1f);
pub const LINE: Color = Color::Rgb(0x39, 0x40, 0x37);
pub const INK: Color = Color::Rgb(0xe4, 0xe8, 0xdd);
pub const MUTED: Color = Color::Rgb(0xa1, 0xaa, 0x9b);
pub const ACCENT: Color = Color::Rgb(0xc6, 0xda, 0x83);
pub const ON_ACCENT: Color = Color::Rgb(0x19, 0x20, 0x0f);
pub const ERROR: Color = Color::Rgb(0xe2, 0xb5, 0x96);
pub fn base() -> Style {
    Style::default().fg(INK).bg(SURFACE)
}
pub fn muted() -> Style {
    base().fg(MUTED)
}
pub fn accent() -> Style {
    base().fg(ACCENT)
}
