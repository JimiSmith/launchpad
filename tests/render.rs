mod common;

use common::app;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use zellij_launchpad_core::{
    app::{Action, App, Focus},
    theme, view,
};

fn draw(app: &App, w: u16, h: u16) -> Buffer {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| view::render(f, app)).unwrap();
    t.backend().buffer().clone()
}
fn text(b: &Buffer) -> String {
    b.content.iter().map(|c| c.symbol()).collect()
}
#[test]
fn remote_results_stay_visible_between_edit_and_latest_reply() {
    use zellij_launchpad_core::remote::RemoteRequest;
    let mut app = App::from_remote("/home/example".into());
    app.take_remote_request();
    assert!(app.apply_remote_results(0, vec!["/home/example/notes".into()]));
    let visible = |app: &App| text(&draw(app, 80, 24)).contains("~/notes/");
    assert!(visible(&app));
    let mut generations = Vec::new();
    for action in [
        Action::Text("n".into()),
        Action::Backspace,
        Action::Delete,
        Action::Clear,
    ] {
        app.update(Action::Down);
        app.update(action);
        assert_eq!(app.highlighted, None, "editing must reset active selection");
        assert!(
            visible(&app),
            "existing results must remain rendered while search is pending"
        );
        let Some(RemoteRequest::Query { generation, .. }) = app.take_remote_request() else {
            panic!("editing must still schedule a fresh search")
        };
        generations.push(generation);
    }
    let latest = generations.pop().unwrap();
    for generation in generations {
        assert!(!app.apply_remote_results(generation, Vec::new()));
        assert!(
            visible(&app),
            "stale replies must not clear the displayed results"
        );
    }
    assert!(app.apply_remote_results(latest, vec!["/home/example/projects".into()]));
    let rendered = text(&draw(&app, 80, 24));
    assert!(!rendered.contains("~/notes/"));
    assert!(rendered.contains("~/projects/"));
    app.update(Action::Text("no-match".into()));
    let generation = app.take_remote_request().unwrap().generation();
    assert!(app.apply_remote_results(generation, Vec::new()));
    assert!(
        app.suggestions.is_empty(),
        "a genuine empty reply must clear old results"
    );
    assert!(!text(&draw(&app, 80, 24)).contains("~/projects/"));
}

#[test]
fn help_scrolls_at_small_sizes_and_unicode_cells_do_not_shift_neighbors() {
    let mut a = app();
    a.update(Action::Help);
    a.update(Action::End);
    let b = draw(&a, 40, 12);
    assert!(
        text(&b).contains("hermes: Hermes"),
        "End reaches the configured command list at the end of help"
    );
    a.update(Action::Escape);
    a.update(Action::Clear);
    a.update(Action::Text("~/Projects/修理".into()));
    let b = draw(&a, 80, 24);
    // Path begins at x=6, y=3; ASCII prefix takes eleven cells.
    assert_eq!(b[(17, 3)].symbol(), "修");
    assert_eq!(b[(18, 3)].symbol(), " ");
    assert_eq!(b[(19, 3)].symbol(), "理");
    assert_eq!(b[(77, 3)].symbol(), "│");
    assert_eq!(b[(77, 3)].fg, theme::Theme::default().accent);
    for (w, h) in [(80, 24), (120, 36), (40, 12), (40, 10)] {
        a.update(Action::Clear);
        a.update(Action::Text("👩🏽‍💻e\u{301}修理/".repeat(50)));
        let b = draw(&a, w, h);
        assert_eq!(b.area.width, w);
    }
}
#[test]
fn wide_terminals_center_a_maximum_160_column_ui_and_mouse_targets() {
    use ratatui::layout::Rect;
    use view::Pointer;

    let mut states = vec![app()];
    let mut help = app();
    help.update(Action::Help);
    states.push(help);
    // Validating: the status paragraph replaces the suggestion list.
    let mut validating = app();
    validating.update(Action::Enter);
    states.push(validating);
    // Empty history: the dashboard renders its empty state instead of rows.
    let mut empty = app();
    empty.history.clear();
    states.push(empty);

    for app in states {
        for height in [24, 36] {
            let mut baseline = Terminal::new(TestBackend::new(160, height)).unwrap();
            let mut base_hits = view::HitMap::default();
            baseline
                .draw(|f| base_hits = view::render_with_hits(f, &app))
                .unwrap();
            for width in [161, 200, 241] {
                let offset = (width - 160) / 2;
                let mut wide = Terminal::new(TestBackend::new(width, height)).unwrap();
                let mut hits = view::HitMap::default();
                wide.draw(|f| hits = view::render_with_hits(f, &app))
                    .unwrap();
                for y in 0..height {
                    for x in 0..width {
                        if x >= offset && x < offset + 160 {
                            assert_eq!(
                                wide.backend().buffer()[(x, y)],
                                baseline.backend().buffer()[(x - offset, y)],
                                "{width}x{height} at {x},{y}"
                            );
                        } else {
                            assert_eq!(wide.backend().buffer()[(x, y)].symbol(), " ");
                        }
                        for pointer in [Pointer::Click, Pointer::ScrollUp, Pointer::ScrollDown] {
                            let actual = hits.action(pointer, x, y, Rect::new(0, 0, width, height));
                            let expected = if x >= offset && x < offset + 160 {
                                base_hits.action(
                                    pointer,
                                    x - offset,
                                    y,
                                    Rect::new(0, 0, 160, height),
                                )
                            } else {
                                None
                            };
                            assert_eq!(
                                format!("{actual:?}"),
                                format!("{expected:?}"),
                                "mouse {width}x{height} at {x},{y}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn recent_tool_column_fits_twenty_characters_before_the_directory() {
    let mut a = app();
    a.history.truncate(1);
    a.history[0].path = "/home/example/recent".into();
    let tool = a.history[0].tool;
    a.commands
        .entries
        .iter_mut()
        .find(|c| c.id == tool)
        .unwrap()
        .label = "12345678901234567890".into();
    for (w, h) in [(40, 10), (40, 12), (80, 24), (120, 36)] {
        let b = draw(&a, w, h);
        let rendered = text(&b);
        assert!(
            rendered.contains("12345678901234567890 ~/recent"),
            "{w}x{h}: {rendered}"
        );
    }
}

#[test]
fn minimum_usable_view_keeps_selected_history_visible() {
    let mut a = app();
    a.history.last_mut().unwrap().path = "/home/example/literal".into();
    a.update(Action::Focus(Focus::History));
    a.update(Action::End);
    let s = text(&draw(&a, 40, 10));
    assert!(s.contains("literal"), "{s}");
}
#[test]
fn roomy_layout_uses_spare_rows_for_history_rhythm() {
    let b = draw(&app(), 120, 36);
    assert_eq!(b[(2, 34)].symbol(), "1");
    assert_eq!(b[(3, 34)].symbol(), "0");
}
#[test]
fn dashboard_fits_ten_events_and_fixed_tool_order_at_real_terminal_sizes() {
    for (w, h) in [(80, 24), (120, 36)] {
        let b = draw(&app(), w, h);
        let s = text(&b);
        for label in [
            "Launchpad",
            "Directory",
            "Launch with",
            "Recent launches",
            "10 events",
            "Ctrl+Q",
            "2d ago",
        ] {
            assert!(s.contains(label), "{w}x{h}: {label}");
        }
        let tools = ["Shell", "Claude", "Codex", "Copilot", "Hermes"].map(|s1| s.find(s1).unwrap());
        assert!(tools.windows(2).all(|p| p[0] < p[1]));
        assert!(
            b.content
                .iter()
                .any(|c| c.fg == theme::Theme::default().accent)
        );
        assert!(!s.contains("one directory"));
    }
}
#[test]
fn narrow_view_scrolls_history_and_tiny_view_has_no_hidden_launch_controls() {
    let mut a = app();
    a.history.last_mut().unwrap().path = "/home/example/literal".into();
    a.update(Action::Focus(Focus::History));
    a.update(Action::End);
    let s = text(&draw(&a, 40, 12));
    assert!(s.contains("Directory"));
    assert!(s.contains("Hermes"));
    assert!(s.contains("literal"));
    for (w, h) in [(39, 12), (40, 9), (10, 4), (1, 1), (0, 0)] {
        let s = text(&draw(&a, w, h));
        assert!(!s.contains("Launch with"));
        if w >= 10 && h >= 4 {
            assert!(s.contains("Resize"));
        }
    }
}
