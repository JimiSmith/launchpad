use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use zellij_launchpad_prototype::{
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
fn help_scrolls_at_small_sizes_and_unicode_cells_do_not_shift_neighbors() {
    let mut a = App::default();
    a.update(Action::Help);
    a.update(Action::End);
    let b = draw(&a, 40, 12);
    assert!(text(&b).contains("memory only"));
    a.update(Action::Escape);
    a.update(Action::Clear);
    a.update(Action::Text("~/Projects/修理".into()));
    let b = draw(&a, 80, 24);
    // Path begins at x=6, y=3; ASCII prefix takes eleven cells.
    assert_eq!(b[(17, 3)].symbol(), "修");
    assert_eq!(b[(18, 3)].symbol(), " ");
    assert_eq!(b[(19, 3)].symbol(), "理");
    assert_eq!(b[(77, 3)].symbol(), "│");
    assert_eq!(b[(77, 3)].fg, theme::ACCENT);
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

    let mut states = vec![App::default()];
    let mut help = App::default();
    help.update(Action::Help);
    states.push(help);
    let mut terminal = App::default();
    terminal.update(Action::Enter);
    states.push(terminal);
    let mut closed = App::default();
    closed.update(Action::Escape);
    closed.update(Action::Escape);
    states.push(closed);

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
fn minimum_usable_view_keeps_selected_history_visible() {
    let mut a = App::default();
    a.update(Action::Focus(Focus::History));
    a.update(Action::End);
    let s = text(&draw(&a, 40, 10));
    assert!(s.contains("literal"), "{s}");
}
#[test]
fn roomy_layout_uses_spare_rows_for_history_rhythm() {
    let b = draw(&App::default(), 120, 36);
    assert_eq!(b[(2, 34)].symbol(), "1");
    assert_eq!(b[(3, 34)].symbol(), "0");
}
#[test]
fn dashboard_fits_ten_events_and_fixed_tool_order_at_real_terminal_sizes() {
    for (w, h) in [(80, 24), (120, 36)] {
        let b = draw(&App::default(), w, h);
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
        assert!(b.content.iter().any(|c| c.fg == theme::ACCENT));
        assert!(!s.contains("one directory"));
    }
}
#[test]
fn narrow_view_scrolls_history_and_tiny_view_has_no_hidden_launch_controls() {
    let mut a = App::default();
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
