mod common;

use common::app;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use zellij_launchpad_core::{
    app::{Action, App, Focus},
    remote::RemoteRequest,
    view::{self, HitMap, Pointer},
};

/// Take the outstanding validation request as (generation, raw path).
fn pending(app: &mut App) -> (u64, String) {
    match app.take_remote_request() {
        Some(RemoteRequest::Validate { generation, raw }) => (generation, raw),
        other => panic!("expected a pending validation, got {other:?}"),
    }
}
/// Take the outstanding validation and answer it with `result`.
fn settle(app: &mut App, result: Result<String, String>) -> String {
    let (generation, raw) = pending(app);
    assert!(app.finish_remote_validation(generation, result));
    raw
}

fn draw(app: &App, w: u16, h: u16) -> HitMap {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut map = HitMap::default();
    terminal
        .draw(|f| map = view::render_with_hits(f, app))
        .unwrap();
    map
}
fn click(app: &mut App, w: u16, h: u16, x: u16, y: u16) {
    if let Some(action) = draw(app, w, h).action(Pointer::Click, x, y, Rect::new(0, 0, w, h)) {
        app.update(action);
    }
}
fn click_label(app: &mut App, w: u16, h: u16, label: &str) {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let mut map = HitMap::default();
    terminal
        .draw(|f| map = view::render_with_hits(f, app))
        .unwrap();
    let b = terminal.backend().buffer();
    for y in 0..h {
        for x in 0..w {
            let tail: String = (x..w).map(|c| b[(c, y)].symbol()).collect();
            if tail.starts_with(label)
                && let Some(action) = map.action(Pointer::Click, x, y, b.area)
            {
                app.update(action);
                return;
            }
        }
    }
    panic!("missing visible control: {label} at {w}x{h}");
}
#[test]
fn history_click_selects_only_then_replay_is_explicit() {
    for (w, h) in [(80, 24), (120, 36), (40, 12), (40, 10)] {
        let mut app = app();
        app.history[9].path = "/home/example/it's literal; $HOME".into();
        app.recent = 9;
        app.update(Action::Escape); // dismiss completions before choosing a history path
        click_label(&mut app, w, h, "it's");
        assert_eq!(app.focus, Focus::History);
        assert_eq!(app.recent, 9);
        assert!(app.take_launch().is_none());
        let event = app.history[9].clone();
        app.update(Action::Tab);
        assert_eq!(app.focus, Focus::Path);
        assert_eq!(app.tool, event.tool, "recent selection fills the form");
        assert_eq!(app.editor.text, app.path_label(&event.path));
        assert!(app.take_launch().is_none());
        app.update(Action::Focus(Focus::History));
        app.update(Action::Enter);
        let raw = settle(&mut app, Ok(event.path.clone()));
        assert_eq!(raw, event.path, "replay revalidates the stored path first");
        let launch = app
            .take_launch()
            .expect("replay hands one launch to the host");
        assert_eq!((launch.path, launch.tool), (event.path, event.tool));
    }
}
fn wheel(app: &mut App, w: u16, h: u16, x: u16, y: u16, down: bool) {
    let pointer = if down {
        Pointer::ScrollDown
    } else {
        Pointer::ScrollUp
    };
    if let Some(action) = draw(app, w, h).action(pointer, x, y, Rect::new(0, 0, w, h)) {
        app.update(action);
    }
}
#[test]
fn wheel_is_section_local_and_clamped() {
    let mut app = app();
    wheel(&mut app, 80, 24, 10, 6, true);
    assert_eq!(app.highlighted, Some(0));
    for _ in 0..30 {
        wheel(&mut app, 80, 24, 10, 6, true);
    }
    assert_eq!(app.highlighted, Some(app.suggestions.len() - 1));
    for _ in 0..30 {
        wheel(&mut app, 80, 24, 10, 6, false);
    }
    assert_eq!(app.highlighted, Some(0));
    wheel(&mut app, 80, 24, 10, 16, true);
    assert_eq!(app.focus, Focus::History);
    assert_eq!(app.recent, 1);
    for _ in 0..20 {
        wheel(&mut app, 40, 10, 15, 8, true);
    }
    assert_eq!(app.recent, 9);
    wheel(&mut app, 40, 10, 0, 8, false);
    wheel(&mut app, 40, 10, 5, 2, false); // path isn't scrollable
    assert_eq!(app.focus, Focus::History);
    assert_eq!(app.recent, 9);
    for _ in 0..20 {
        wheel(&mut app, 40, 10, 15, 8, false);
    }
    assert_eq!(app.recent, 0);
    app.update(Action::Help);
    wheel(&mut app, 40, 10, 5, 3, true);
    assert_eq!(app.help_scroll, 1);
    app.update(Action::End);
    wheel(&mut app, 40, 10, 5, 3, true);
    let end = app.help_scroll;
    wheel(&mut app, 40, 10, 5, 3, true);
    assert_eq!(app.help_scroll, end, "wheel stops at the rendered end");
    wheel(&mut app, 40, 10, 5, 3, false);
    assert_eq!(app.help_scroll, end - 1, "wheel up moves one rendered row");
    app.update(Action::Home);
    wheel(&mut app, 40, 10, 5, 3, false);
    assert_eq!(app.help_scroll, 0);
    assert!(app.take_launch().is_none());
}
#[test]
fn explicit_launch_back_help_reset_and_quit_controls() {
    for (w, h) in [(80, 24), (120, 36), (40, 12), (40, 10)] {
        let mut app = app();
        app.update(Action::Down); // highlighted completion must not hijack explicit launch
        click_label(&mut app, w, h, "Launch ↵");
        assert_eq!(
            pending(&mut app).1,
            "notes",
            "explicit launch validates the typed path, not the highlight"
        );
        app.update(Action::Escape); // abandon the pending validation
        assert!(app.take_launch().is_none());
        click_label(&mut app, w, h, "F1");
        assert!(app.help);
        click_label(&mut app, w, h, "Esc back");
        assert!(!app.help);
        app.update(Action::Reset);
        assert!(!app.touched);
        app.update(Action::Quit);
        assert!(app.quit);
    }
}
#[test]
fn tool_click_selects_and_focuses_without_launch_even_when_wrapped() {
    use zellij_launchpad_core::app::Tool;
    for (w, h) in [(80, 24), (120, 36), (40, 12)] {
        let mut app = app();
        click_label(&mut app, w, h, "Codex");
        assert_eq!(app.focus, Focus::Tools, "{w}x{h}");
        assert_eq!(app.tool, Tool::new("codex").unwrap());
        assert!(app.take_launch().is_none());
    }
}
#[test]
fn suggestion_click_accepts_without_launching() {
    let mut app = app();
    click(&mut app, 80, 24, 12, 7);
    assert_eq!(app.focus, Focus::Path);
    let raw = settle(&mut app, Ok(common::DIRECTORIES[1].into()));
    assert_eq!(raw, common::DIRECTORIES[1], "the second visible row");
    assert_eq!(app.editor.text, "~/Projects/notes");
    assert!(app.suggestions.is_empty());
    assert!(app.take_launch().is_none(), "accepting never launches");
    assert_ne!(app.history[0].age, "Just now");
}
#[test]
fn scrolled_input_maps_visible_origin_and_clipped_tail() {
    use zellij_launchpad_core::cells::{input_cursor, input_window};
    // Still wider than both viewports, with room under the input cap to edit.
    let text = format!("{}修理/e\u{301}👩🏽‍💻", "a".repeat(80));
    for (w, h, x, y, budget) in [(80, 24, 4, 4, 74), (40, 10, 3, 2, 36)] {
        let mut app = app();
        app.editor.set(&text);
        let (visible, _) = input_window(&text, text.len(), budget);
        let start = text.len() - visible.len();
        click(&mut app, w, h, x, y);
        assert_eq!(app.editor.cursor, start);
        app.update(Action::Text("Z".into()));
        assert_eq!(&app.editor.text[start..start + 1], "Z");
    }
    assert_eq!(input_cursor("修理abc", 0, 4, 3), "修".len()); // ellipsis = first omitted
    assert_eq!(input_cursor("e\u{301}修👩🏽‍💻", 0, 6, 4), "e\u{301}修".len());
    assert_eq!(input_cursor("abc", 0, 0, 0), 0);
}
#[test]
fn stale_resize_blank_and_tiny_hit_maps_are_inert() {
    let app = app();
    let map = draw(&app, 80, 24);
    for pointer in [Pointer::Click, Pointer::ScrollDown, Pointer::ScrollUp] {
        for (x, y) in [(0, 0), (79, 23), (80, 24), (u16::MAX, u16::MAX)] {
            assert_eq!(map.action(pointer, x, y, Rect::new(0, 0, 80, 24)), None);
        }
        assert_eq!(map.action(pointer, 68, 9, Rect::new(0, 0, 120, 36)), None);
        for (w, h) in [(39, 24), (80, 9), (0, 0)] {
            let tiny = draw(&app, w, h);
            for y in 0..h {
                for x in 0..w {
                    assert_eq!(tiny.action(pointer, x, y, Rect::new(0, 0, w, h)), None);
                }
            }
        }
    }
    assert_eq!(
        map.action(Pointer::Click, 20, 12, Rect::new(0, 0, 80, 24)),
        None
    ); // separator
}
#[test]
fn scrolled_suggestions_click_the_visible_result() {
    let mut app = app();
    app.highlighted = Some(app.suggestions.len() - 1);
    let last = app.dirs[*app.suggestions.last().unwrap()].path.clone();
    let expected = app.path_label(&last);
    click(&mut app, 40, 10, 10, 3);
    let raw = settle(&mut app, Ok(last.clone()));
    assert_eq!(raw, last, "the scrolled-to row, not the first");
    assert_eq!(app.editor.text, expected);
    assert!(app.take_launch().is_none());
}
#[test]
fn unavailable_history_and_empty_lists_do_not_launch_or_fall_back() {
    let mut app = app();
    // This instance's configuration no longer defines the copilot command.
    app.configure(&std::collections::BTreeMap::from([
        ("commands".to_string(), "claude".to_string()),
        ("command_claude".to_string(), "claude".to_string()),
    ]));
    let row = app.history[5].clone();
    assert!(!app.available(row.tool));
    app.update(Action::SelectHistory(row.id));
    app.update(Action::Enter);
    settle(&mut app, Ok(row.path.clone()));
    assert!(app.take_launch().is_none(), "no silent substitution");
    assert!(app.message.as_ref().unwrap().contains("unavailable"));
    app.update(Action::Tab);
    assert_eq!(app.focus, Focus::Path);
    assert_eq!(
        app.tool, row.tool,
        "removed tools must not silently become Shell"
    );
    app.history.clear();
    app.suggestions.clear();
    app.focus = Focus::History;
    app.update(Action::Enter);
    wheel(&mut app, 80, 24, 10, 16, true);
    assert_eq!(app.recent, 5); // no empty-list navigation or implicit launch
    app.update(Action::SelectHistory(u64::MAX));
    app.update(Action::AcceptSuggestion(usize::MAX));
    app.update(Action::PathCursor(usize::MAX));
    assert!(app.take_launch().is_none());
}
#[test]
fn path_click_uses_cells_graphemes_padding_and_focus() {
    let mut app = app();
    app.editor.set("修理/e\u{301}👩🏽‍💻");
    app.focus = Focus::Tools;
    click(&mut app, 80, 24, 7, 4); // second cell of 理: before the whole grapheme
    assert_eq!(app.focus, Focus::Path);
    assert_eq!(app.editor.cursor, "修".len());
    click(&mut app, 80, 24, 9, 4);
    assert_eq!(app.editor.cursor, "修理/".len());
    click(&mut app, 80, 24, 10, 4);
    assert_eq!(app.editor.cursor, "修理/e\u{301}".len());
    click(&mut app, 80, 24, 2, 4); // input left padding
    assert_eq!(app.editor.cursor, 0);
    click(&mut app, 80, 24, 74, 4);
    assert_eq!(app.editor.cursor, app.editor.text.len());
    assert!(app.take_launch().is_none());
}

#[test]
fn recent_selection_keeps_launch_button_and_keyboard_in_agreement() {
    for selection in [
        Action::Focus(Focus::History),
        Action::BackTab,
        Action::SelectHistory(2),
    ] {
        for submit in [Action::Enter, Action::LaunchForm] {
            let mut a = app();
            a.update(selection.clone());
            a.update(Action::Down);
            let expected = a.history[a.recent].clone();
            assert_eq!(a.editor.text, a.path_label(&expected.path));
            assert_eq!(a.tool, expected.tool);
            assert!(a.suggestions.is_empty());
            assert!(a.take_launch().is_none());
            a.update(submit);
            assert_eq!(settle(&mut a, Ok(expected.path.clone())), expected.path);
            let launch = a.take_launch().unwrap();
            assert_eq!((launch.path, launch.tool), (expected.path, expected.tool));
        }
    }
    let mut a = app();
    a.update(Action::Focus(Focus::History));
    let mut history = a.history.clone();
    history.remove(0);
    a.replace_history(history);
    assert_eq!(a.editor.text, a.path_label(&a.history[0].path));
    assert_eq!(a.tool, a.history[0].tool);
}
