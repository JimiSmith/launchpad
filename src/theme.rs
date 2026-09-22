use std::collections::BTreeMap;

use ratatui::style::{Color, Style};

/// Per-instance UI colours: Catppuccin Macchiato over the terminal background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub background: Color,
    pub surface: Color,
    pub raised: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub error: Color,
}

impl Default for Theme {
    fn default() -> Self {
        // https://github.com/catppuccin/palette — Macchiato
        Self {
            background: Color::Reset,                // Terminal background
            surface: Color::Rgb(0x1e, 0x20, 0x30),   // Mantle
            raised: Color::Rgb(0x36, 0x3a, 0x4f),    // Surface 0
            border: Color::Rgb(0x49, 0x4d, 0x64),    // Surface 1
            text: Color::Rgb(0xca, 0xd3, 0xf5),      // Text
            muted: Color::Rgb(0xa5, 0xad, 0xcb),     // Subtext 0
            accent: Color::Rgb(0xc6, 0xa0, 0xf6),    // Mauve
            on_accent: Color::Rgb(0x18, 0x19, 0x26), // Crust
            error: Color::Rgb(0xed, 0x87, 0x96),     // Red
        }
    }
}

impl Theme {
    /// Apply #RRGGBB or terminal `default` overrides, retaining defaults on error.
    pub fn parse(configuration: &BTreeMap<String, String>) -> (Self, Vec<String>) {
        let mut theme = Self::default();
        let mut errors = Vec::new();
        for (key, colour) in [
            ("theme_background", &mut theme.background),
            ("theme_surface", &mut theme.surface),
            ("theme_raised", &mut theme.raised),
            ("theme_border", &mut theme.border),
            ("theme_text", &mut theme.text),
            ("theme_muted", &mut theme.muted),
            ("theme_accent", &mut theme.accent),
            ("theme_on_accent", &mut theme.on_accent),
            ("theme_error", &mut theme.error),
        ] {
            if let Some(value) = configuration.get(key) {
                if let Some(parsed) = parse_colour(value) {
                    *colour = parsed;
                } else {
                    errors.push(format!("{key}: expected #RRGGBB or default; using default"));
                }
            }
        }
        (theme, errors)
    }

    pub fn base(self) -> Style {
        Style::default().fg(self.text).bg(self.background)
    }

    pub fn muted(self) -> Style {
        self.base().fg(self.muted)
    }

    pub fn accent(self) -> Style {
        self.base().fg(self.accent)
    }

    /// Match the mockup's 65% opacity without relying on terminal SGR dim support.
    /// An unknown shell background uses black for this calculation only; the
    /// actual background remains the terminal default.
    pub fn inactive(self, foreground: Color) -> Color {
        let (r, g, b) = match foreground {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => match self.text {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => (0xca, 0xd3, 0xf5),
            },
        };
        let (br, bg, bb) = match self.background {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => (0, 0, 0),
        };
        let blend = |fg: u8, bg: u8| ((u16::from(fg) * 65 + u16::from(bg) * 35) / 100) as u8;
        Color::Rgb(blend(r, br), blend(g, bg), blend(b, bb))
    }
}

fn parse_colour(value: &str) -> Option<Color> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("default") {
        return Some(Color::Reset);
    }
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
}
