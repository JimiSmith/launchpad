mod common;

use std::collections::BTreeMap;

use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use zellij_launchpad_core::{
    app::{Action, App, Focus},
    theme::Theme,
    view,
};

fn config(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn partial_overrides_accept_hex_and_keep_defaults_for_invalid_values() {
    for invalid in [
        "",
        "red",
        "123456",
        "#abc",
        "#12345678",
        "#gg0000",
        "#é1234",
        "#12\n345",
        "\x1b[31m",
    ] {
        let (theme, errors) = Theme::parse(&config(&[
            ("theme_accent", "  #aB12Ef  "),
            ("theme_background", invalid),
        ]));
        assert_eq!(
            theme,
            Theme {
                accent: Color::Rgb(0xab, 0x12, 0xef),
                ..Theme::default()
            }
        );
        assert_eq!(
            errors,
            ["theme_background: expected #RRGGBB or default; using default"]
        );
    }
    assert_eq!(Theme::parse(&BTreeMap::new()), (Theme::default(), vec![]));
}

#[test]
fn terminal_background_is_the_default_and_can_be_explicitly_selected() {
    assert_eq!(Theme::default().background, Color::Reset);
    let (theme, errors) = Theme::parse(&config(&[
        ("theme_background", " Default "),
        ("theme_surface", "default"),
    ]));
    assert!(errors.is_empty());
    assert_eq!(theme.background, Color::Reset);
    assert_eq!(theme.surface, Color::Reset);

    let mut app = App::default();
    for (help, width, height) in [(false, 80, 24), (true, 80, 24), (false, 30, 8)] {
        app.help = help;
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
        view::render_buffer(&mut buffer, &app);
        assert_eq!(buffer[(0, 0)].bg, Color::Reset);
        assert!(
            buffer
                .content
                .iter()
                .all(|cell| cell.bg != Color::Rgb(36, 39, 58)),
            "must not paint Macchiato Base"
        );
    }
}

#[test]
fn all_rendered_colours_follow_each_instances_configuration() {
    let keys = [
        "background",
        "surface",
        "raised",
        "border",
        "text",
        "muted",
        "accent",
        "on_accent",
        "error",
    ];
    let configuration = keys
        .iter()
        .enumerate()
        .map(|(i, key)| (format!("theme_{key}"), format!("#{:02x}0203", i + 1)))
        .collect();
    let defaults = Theme::default();
    let default_colours = [
        defaults.background,
        defaults.surface,
        defaults.raised,
        defaults.border,
        defaults.text,
        defaults.muted,
        defaults.accent,
        defaults.on_accent,
        defaults.error,
    ];
    let mut seen = [false; 9];
    let mut app = common::app();
    app.configure(&configuration);
    assert!(app.theme_errors.is_empty());

    for focus in [Focus::Path, Focus::Tools, Focus::History] {
        app.help = false;
        app.update(Action::Focus(focus));
        app.highlighted = Some(0);
        for help in [false, true] {
            app.help = help;
            for size in [(80, 24), (40, 10), (30, 8), (180, 36)] {
                app.message = Some("Example error".into());
                let mut actual = Buffer::empty(Rect::new(0, 0, size.0, size.1));
                let (_, cursor) = view::render_buffer(&mut actual, &app);
                let mut baseline_app = common::app();
                baseline_app.configure(&BTreeMap::new());
                baseline_app.update(Action::Focus(app.focus));
                baseline_app.highlighted = app.highlighted;
                baseline_app.help = help;
                baseline_app.message = app.message.clone();
                let mut baseline = Buffer::empty(actual.area);
                let (_, baseline_cursor) = view::render_buffer(&mut baseline, &baseline_app);
                assert_eq!(cursor, baseline_cursor);
                for (actual, expected) in actual.content.iter().zip(&baseline.content) {
                    assert_eq!(actual.symbol(), expected.symbol());
                    assert_eq!(actual.modifier, expected.modifier);
                    for (colour, default) in [(actual.fg, expected.fg), (actual.bg, expected.bg)] {
                        let (index, inactive) = default_colours
                            .iter()
                            .position(|&c| c == default)
                            .map(|i| (i, false))
                            .or_else(|| {
                                default_colours
                                    .iter()
                                    .enumerate()
                                    .skip(1)
                                    .find(|(_, c)| defaults.inactive(**c) == default)
                                    .map(|(i, _)| (i, true))
                            })
                            .expect("every cell uses a theme role or its inactive colour");
                        seen[index] = true;
                        let configured = Color::Rgb(index as u8 + 1, 2, 3);
                        assert_eq!(
                            colour,
                            if inactive {
                                app.theme.inactive(configured)
                            } else {
                                configured
                            },
                            "role {}",
                            keys[index]
                        );
                    }
                }
            }
        }
    }
    assert!(
        [0, 2, 3, 4, 5, 6, 8].into_iter().all(|role| seen[role]),
        "all roles used by the minimal design must be exercised"
    );
}

#[test]
fn theme_errors_join_command_errors_and_reconfiguration_clears_them() {
    let mut app = App::default();
    app.configure(&config(&[
        ("commands", "missing"),
        ("theme_text", "invalid"),
        ("theme_accent", "#123456"),
    ]));
    assert_eq!(app.config_errors().count(), 2);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 24));
    view::render_buffer(&mut buffer, &app);
    let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
    assert!(text.contains("all 2 errors"));
    let help = zellij_launchpad_core::help::lines(&app, 92).join("\n");
    assert!(help.contains("command_missing"));
    assert!(help.contains("theme_text: expected #RRGGBB"));
    app.configure(&BTreeMap::new());
    assert_eq!(app.config_errors().count(), 0);
    assert_eq!(app.theme, Theme::default());
}

#[test]
fn inactive_colours_preserve_hues_and_respect_explicit_backgrounds() {
    let theme = Theme::default();
    assert_eq!(theme.inactive(theme.text), Color::Rgb(131, 137, 159));
    assert_eq!(theme.inactive(theme.accent), Color::Rgb(128, 104, 159));
    assert_eq!(theme.inactive(Color::Reset), Color::Rgb(131, 137, 159));
    let theme = Theme {
        background: Color::Rgb(255, 255, 255),
        text: Color::Rgb(0, 0, 0),
        ..theme
    };
    assert_eq!(theme.inactive(theme.text), Color::Rgb(89, 89, 89));
}
