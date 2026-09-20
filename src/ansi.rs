//! Full-frame ANSI serialization for plugin stdout, not a terminal session.
use ratatui::buffer::Buffer;

pub fn serialize(buffer: &Buffer) -> String {
    use std::fmt::Write;
    if buffer.area.is_empty() {
        return String::new();
    }
    let mut style = (
        ratatui::style::Color::Reset,
        ratatui::style::Color::Reset,
        ratatui::style::Modifier::empty(),
    );
    let mut out = String::with_capacity(buffer.content.len() + buffer.area.height as usize * 8);
    out.push_str("\x1b[0m");
    for y in buffer.area.y..buffer.area.bottom() {
        write!(out, "\x1b[{};1H", y - buffer.area.y + 1).unwrap();
        let mut x = buffer.area.x;
        while x < buffer.area.right() {
            let cell = &buffer[(x, y)];
            let ascii = matches!(cell.symbol().as_bytes(), [b' '..=b'~']);
            let w = if ascii {
                1
            } else {
                crate::cells::width(cell.symbol()).max(1)
            };
            if (cell.fg, cell.bg, cell.modifier) != style {
                style = (cell.fg, cell.bg, cell.modifier);
                out.push_str("\x1b[0m");
                color(&mut out, cell.fg, false);
                color(&mut out, cell.bg, true);
                use ratatui::style::Modifier as M;
                for (flag, code) in [
                    (M::BOLD, 1),
                    (M::DIM, 2),
                    (M::ITALIC, 3),
                    (M::UNDERLINED, 4),
                    (M::SLOW_BLINK, 5),
                    (M::RAPID_BLINK, 6),
                    (M::REVERSED, 7),
                    (M::HIDDEN, 8),
                    (M::CROSSED_OUT, 9),
                ] {
                    if cell.modifier.contains(flag) {
                        write!(out, "\x1b[{code}m").unwrap();
                    }
                }
            }
            // Most cells (including all background spaces) need neither Unicode
            // segmentation/normalization nor a temporary string allocation.
            if ascii {
                out.push_str(cell.symbol());
                x += 1;
                continue;
            }
            use unicode_normalization::UnicodeNormalization;
            use unicode_width::UnicodeWidthChar;
            let symbol: String = cell.symbol().nfc().collect();
            let host_width: usize = symbol.chars().map(|c| c.width().unwrap_or(0)).sum();
            if host_width != w || symbol.contains(char::is_control) {
                // Zellij's scalar grid expands some emoji clusters, corrupting
                // adjacent columns. Keep their allocated width, not their bytes.
                out.push('�');
                out.extend(std::iter::repeat_n(' ', w - 1));
            } else {
                out.push_str(&symbol);
            }
            x = x.saturating_add(w as u16);
        }
    }
    out.push_str("\x1b[0m");
    out
}

fn color(out: &mut String, color: ratatui::style::Color, background: bool) {
    use ratatui::style::Color::*;
    use std::fmt::Write;
    let base = if background { 48 } else { 38 };
    let index = match color {
        Reset => return,
        Rgb(r, g, b) => {
            write!(out, "\x1b[{base};2;{r};{g};{b}m").unwrap();
            return;
        }
        Indexed(i) => i,
        Black => 0,
        Red => 1,
        Green => 2,
        Yellow => 3,
        Blue => 4,
        Magenta => 5,
        Cyan => 6,
        Gray => 7,
        DarkGray => 8,
        LightRed => 9,
        LightGreen => 10,
        LightYellow => 11,
        LightBlue => 12,
        LightMagenta => 13,
        LightCyan => 14,
        White => 15,
    };
    write!(out, "\x1b[{base};5;{index}m").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{layout::Rect, style::Style};
    #[test]
    fn zellij_scalar_grid_gets_nfc_and_width_safe_emoji_fallback() {
        // Host 0.45.1 drops zero-width scalars and does not segment graphemes.
        // Preserve editor bytes/hit-map widths while making display safe.
        let mut b = Buffer::empty(Rect::new(0, 0, 7, 1));
        b.set_string(0, 0, "e\u{301}👩🏽‍💻修XY", Style::default());
        assert_eq!(serialize(&b), "\x1b[0m\x1b[1;1Hé� 修XY\x1b[0m");
        let mut narrow = Buffer::empty(Rect::new(0, 0, 1, 1));
        narrow.set_string(0, 0, "修", Style::default());
        assert_eq!(serialize(&narrow), "\x1b[0m\x1b[1;1H \x1b[0m");
    }
    #[test]
    fn styled_spaces_survive_but_covered_styles_never_leak() {
        use ratatui::style::{Color, Modifier};
        let mut b = Buffer::empty(Rect::new(0, 0, 6, 1));
        b.set_string(0, 0, "修 X  ", Style::default());
        b[(0, 0)].set_fg(Color::Rgb(1, 2, 3));
        b[(1, 0)].set_bg(Color::Red); // covered continuation: must be ignored
        b[(2, 0)]
            .set_bg(Color::Indexed(4))
            .set_style(Style::default().add_modifier(Modifier::REVERSED));
        assert_eq!(
            serialize(&b),
            "\x1b[0m\x1b[1;1H\x1b[0m\x1b[38;2;1;2;3m修\x1b[0m\x1b[48;5;4m\x1b[7m \x1b[0mX  \x1b[0m"
        );
    }
    #[test]
    fn wide_continuations_and_exact_fit_rows_do_not_add_columns() {
        let mut b = Buffer::empty(Rect::new(4, 8, 6, 2));
        b.set_string(4, 8, "修理XY", Style::default());
        b.set_string(4, 9, "abcd修", Style::default());
        assert_eq!(
            serialize(&b),
            "\x1b[0m\x1b[1;1H修理XY\x1b[2;1Habcd修\x1b[0m"
        );
        assert_eq!(serialize(&Buffer::empty(Rect::new(0, 0, 0, 0))), "");
    }
}
