mod common;

use common::app;
use launchpad_core::{
    app::{Action, App, Focus},
    theme, view,
};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

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
    use launchpad_core::remote::RemoteRequest;
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
        text(&b).contains("Search"),
        "End reaches the status group at the end of the keys screen"
    );
    a.update(Action::Escape);
    a.update(Action::Clear);
    a.update(Action::Text("~/Projects/修理".into()));
    let b = draw(&a, 80, 24);
    let (x, y) = label_position(&b, "› ~/Projects/");
    assert_eq!(b[(x + 13, y)].symbol(), "修");
    assert_eq!(b[(x + 14, y)].symbol(), " ");
    assert_eq!(b[(x + 15, y)].symbol(), "理");
    assert_eq!(b[(77, y)].symbol(), "│");
    assert_eq!(b[(77, y)].fg, theme::Theme::default().accent);
    for (w, h) in [(80, 24), (120, 36), (40, 12), (40, 10)] {
        a.update(Action::Clear);
        a.update(Action::Text("👩🏽‍💻e\u{301}修理/".repeat(50)));
        let b = draw(&a, w, h);
        assert_eq!(b.area.width, w);
    }
}
#[test]
fn wide_terminals_center_a_maximum_96_column_ui_and_mouse_targets() {
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
            let mut baseline = Terminal::new(TestBackend::new(96, height)).unwrap();
            let mut base_hits = view::HitMap::default();
            baseline
                .draw(|f| base_hits = view::render_with_hits(f, &app))
                .unwrap();
            for width in [97, 160, 241] {
                let offset = (width - 96) / 2;
                let mut wide = Terminal::new(TestBackend::new(width, height)).unwrap();
                let mut hits = view::HitMap::default();
                wide.draw(|f| hits = view::render_with_hits(f, &app))
                    .unwrap();
                for y in 0..height {
                    for x in 0..width {
                        if x >= offset && x < offset + 96 {
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
                            let expected = if x >= offset && x < offset + 96 {
                                base_hits.action(
                                    pointer,
                                    x - offset,
                                    y,
                                    Rect::new(0, 0, 96, height),
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
fn recent_rows_show_only_the_directory() {
    let mut a = app();
    a.history.truncate(1);
    a.history[0].path = "/home/example/recent".into();
    for (w, h) in [(40, 10), (40, 12), (80, 24), (120, 36)] {
        let b = draw(&a, w, h);
        let y = (0..h)
            .find(|&y| {
                (0..w)
                    .map(|x| b[(x, y)].symbol())
                    .collect::<String>()
                    .contains("~/recent")
            })
            .unwrap();
        let row: String = (0..w).map(|x| b[(x, y)].symbol()).collect();
        assert_eq!(row.trim().trim_matches('│').trim(), "~/recent");
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
fn dashboard_groups_tool_above_directories_and_dims_inactive_content() {
    use ratatui::style::Modifier;
    let theme = theme::Theme::default();
    for focus in [Focus::Path, Focus::Tools, Focus::History] {
        let mut a = app();
        a.focus = focus;
        for (w, h) in [(40, 10), (40, 12), (80, 24), (120, 36)] {
            let b = draw(&a, w, h);
            let (tool_x, tool_y) = label_position(&b, "Tool");
            let (dir_x, dir_y) = label_position(&b, "Directory");
            let (list_x, list_y) = label_position(&b, "Saved / Recent");
            assert!(tool_y < dir_y && dir_y < list_y);
            assert_eq!(tool_x, dir_x);
            for (x, y, active) in [
                (tool_x, tool_y, focus == Focus::Tools),
                (dir_x, dir_y, focus != Focus::Tools),
            ] {
                assert_eq!(b[(x - 2, y)].symbol(), "╭");
                assert_eq!(
                    b[(x - 2, y)].fg,
                    if active { theme.accent } else { theme.border }
                );
                assert_eq!(b[(x - 2, y + 1)].symbol(), "│");
            }
            let (x, y) = label_position(&b, "Shell");
            assert!(b[(x, y)].modifier.contains(Modifier::UNDERLINED));
            assert_eq!(
                b[(x, y)].fg,
                if focus == Focus::Tools {
                    theme.accent
                } else {
                    theme.inactive(theme.accent)
                }
            );
            assert_eq!(b[(list_x, list_y + 1)].symbol(), "~");
            assert_eq!(
                b[(list_x, list_y + 1)]
                    .modifier
                    .contains(Modifier::UNDERLINED),
                focus == Focus::History
            );
            let (x, y) = label_position(&b, "› notes");
            assert_eq!(
                b[(x + 2, y)].fg,
                if focus == Focus::Path {
                    theme.text
                } else {
                    theme.inactive(theme.text)
                }
            );
            assert!(
                b.content
                    .iter()
                    .all(|c| c.bg == ratatui::style::Color::Reset)
            );
        }
    }
}
fn label_position(buffer: &Buffer, label: &str) -> (u16, u16) {
    (0..buffer.area.height)
        .find_map(|y| {
            (0..buffer.area.width)
                .find(|&x| {
                    (x..buffer.area.width)
                        .map(|c| buffer[(c, y)].symbol())
                        .collect::<String>()
                        .starts_with(label)
                })
                .map(|x| (x, y))
        })
        .unwrap_or_else(|| panic!("missing {label}"))
}

#[test]
fn narrow_view_scrolls_history_and_tiny_view_has_no_hidden_launch_controls() {
    let mut a = app();
    a.history.last_mut().unwrap().path = "/home/example/literal".into();
    a.update(Action::Focus(Focus::History));
    a.update(Action::End);
    let s = text(&draw(&a, 40, 12));
    assert!(s.contains("Directory"));
    assert!(s.contains("literal"));
    for (w, h) in [(39, 12), (40, 9), (10, 4), (1, 1), (0, 0)] {
        let s = text(&draw(&a, w, h));
        assert!(!s.contains("Launch with"));
        if w >= 10 && h >= 4 {
            assert!(s.contains("Resize"));
        }
    }
}

#[test]
fn resized_layout_keeps_launch_history_and_errors_reachable() {
    use ratatui::layout::Rect;
    use view::Pointer;
    for w in [40, 48, 60, 80, 96, 160] {
        for h in 10..=36 {
            let mut a = app();
            a.commands.entries[1].label = "A long custom tool label with 修理".into();
            a.update(Action::Focus(Focus::History));
            a.update(Action::End);
            for message in [
                None,
                Some("Directory unavailable. Choose another directory and retry.".into()),
            ] {
                a.message = message;
                let area = Rect::new(0, 0, w, h);
                let mut buffer = Buffer::empty(area);
                let (hits, _) = view::render_buffer(&mut buffer, &a);
                for action in [
                    Action::LaunchForm,
                    Action::SelectHistory(a.history[a.recent].id),
                    Action::Help,
                ] {
                    assert!(
                        (0..h).any(|y| (0..w).any(
                            |x| hits.action(Pointer::Click, x, y, area) == Some(action.clone())
                        )),
                        "{w}x{h}: {action:?}"
                    );
                }
                if a.message.is_some() {
                    assert!(text(&buffer).contains("Directory unavailable"), "{w}x{h}");
                }
            }
        }
    }
}

#[test]
fn medium_height_with_suggestions_and_message_still_shows_a_recent_row() {
    let mut a = app();
    a.suggestions.truncate(3);
    a.history[0].path = "/home/example/only-in-recent".into();
    a.message = Some("Worker timed out".into());
    let s = text(&draw(&a, 80, 18));
    assert!(s.contains("only-in-recent") && s.contains("1–"), "{s}");
}

#[test]
fn saved_rows_use_full_width_and_scroll_at_small_sizes() {
    let mut a = app();
    a.replace_saved(
        (0..20)
            .map(|i| format!("/home/example/saved-{i:02}"))
            .collect(),
    );
    let tool = a.tool;
    a.update(Action::Focus(Focus::History));
    a.update(Action::End);
    for (w, h) in [(40, 10), (80, 24)] {
        let buffer = draw(&a, w, h);
        let rendered = text(&buffer);
        assert_list_selection(&buffer, true);
        assert!(rendered.contains("~/saved-19"));
        assert!(!rendered.contains("~/saved-00"));
    }
    assert_eq!(a.tool, tool);
    a.update(Action::Right);
    assert_list_selection(&draw(&a, 80, 24), false);
    a.update(Action::Focus(Focus::Path));
    assert_list_selection(&draw(&a, 80, 24), false);
    a.update(Action::Focus(Focus::History));
    a.update(Action::Left);
    a.replace_saved(Vec::new());
    assert!(text(&draw(&a, 80, 24)).contains("No saved directories"));
}

fn assert_list_selection(buffer: &Buffer, saved: bool) {
    use ratatui::style::Modifier;
    let (x, y) = label_position(buffer, "Saved / Recent");
    for offset in 0..14 {
        let selected = if offset < 5 {
            saved
        } else if offset >= 8 {
            !saved
        } else {
            false
        };
        assert_eq!(
            buffer[(x + offset, y)]
                .modifier
                .contains(Modifier::UNDERLINED),
            selected
        );
    }
}
