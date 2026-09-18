use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use zellij_launchpad_prototype::{
    app::{Action, App, Focus, Screen},
    view::{self, HitMap, Pointer},
};

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
fn history_click_selects_only_then_copy_or_replay_is_explicit() {
    for (w, h) in [(80, 24), (120, 36), (40, 12), (40, 10)] {
        let mut app = App::demo();
        app.recent = 9;
        app.update(Action::Escape); // dismiss completions before choosing a history path
        click_label(&mut app, w, h, "it's");
        assert_eq!(app.focus, Focus::History);
        assert_eq!(app.recent, 9);
        assert_eq!(app.screen, Screen::Dashboard);
        let event = app.history[9].clone();
        click_label(&mut app, w, h, "Tab copy");
        assert_eq!(app.focus, Focus::Path);
        assert_eq!(app.tool, event.tool);
        assert_eq!(app.screen, Screen::Dashboard);
        app.update(Action::Focus(Focus::History));
        click_label(&mut app, w, h, "Enter replay");
        assert!(
            matches!(&app.screen, Screen::Terminal(e) if e.path == event.path && e.tool == event.tool)
        );
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
    let mut app = App::demo();
    wheel(&mut app, 80, 24, 10, 5, true);
    assert_eq!(app.highlighted, Some(0));
    for _ in 0..30 {
        wheel(&mut app, 80, 24, 10, 5, true);
    }
    assert_eq!(app.highlighted, Some(app.suggestions.len() - 1));
    for _ in 0..30 {
        wheel(&mut app, 80, 24, 10, 5, false);
    }
    assert_eq!(app.highlighted, Some(0));
    wheel(&mut app, 80, 24, 10, 14, true);
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
    assert_eq!(
        app.help_scroll,
        zellij_launchpad_prototype::help::LINES.len() - 1
    );
    app.update(Action::Home);
    wheel(&mut app, 40, 10, 5, 3, false);
    assert_eq!(app.help_scroll, 0);
    assert_eq!(app.screen, Screen::Dashboard);
}
#[test]
fn explicit_launch_back_help_reset_and_quit_controls() {
    for (w, h) in [(80, 24), (120, 36), (40, 12), (40, 10)] {
        let mut app = App::demo();
        app.update(Action::Down); // highlighted completion must not hijack explicit launch
        click_label(&mut app, w, h, "Enter ↵");
        assert!(matches!(&app.screen, Screen::Terminal(e) if e.path == "/home/demo/Projects"));
        click_label(&mut app, w, h, "Esc back");
        assert_eq!(app.screen, Screen::Dashboard);
        click_label(&mut app, w, h, "F1");
        assert!(app.help);
        click_label(&mut app, w, h, "Esc back");
        assert!(!app.help);
        click_label(&mut app, w, h, "F5");
        assert!(!app.touched);
        click_label(&mut app, w, h, if w == 40 { "^Q" } else { "Ctrl+Q" });
        assert!(app.quit);
    }
}
#[test]
fn tool_click_selects_and_focuses_without_launch_even_when_wrapped() {
    use zellij_launchpad_prototype::app::Tool;
    for (w, h, x, y) in [(80, 24, 26, 9), (120, 36, 26, 12), (40, 12, 4, 6)] {
        let mut app = App::demo();
        click(&mut app, w, h, x, y);
        assert_eq!(app.focus, Focus::Tools, "{w}x{h}");
        assert_eq!(app.tool, if w == 40 { Tool::Copilot } else { Tool::Codex });
        assert_eq!(app.screen, Screen::Dashboard);
    }
}
#[test]
fn suggestion_click_accepts_without_launching() {
    let mut app = App::demo();
    app.update(Action::Clear);
    app.update(Action::Text("notes".into()));
    click(&mut app, 80, 24, 12, 6);
    assert_eq!(app.editor.text, "~/Projects/research/notes");
    assert_eq!(app.focus, Focus::Path);
    assert!(app.suggestions.is_empty());
    assert_eq!(app.screen, Screen::Dashboard);
    assert_ne!(app.history[0].age, "Just now");
}
#[test]
fn scrolled_input_maps_visible_origin_and_clipped_tail() {
    use zellij_launchpad_prototype::cells::{input_cursor, input_window};
    // Still wider than both viewports, with room under the input cap to edit.
    let text = format!("{}修理/e\u{301}👩🏽‍💻", "a".repeat(80));
    for (w, h, x, y, budget) in [(80, 24, 6, 3, 70), (40, 10, 4, 2, 34)] {
        let mut app = App::demo();
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
    let app = App::demo();
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
        map.action(Pointer::Click, 20, 10, Rect::new(0, 0, 80, 24)),
        None
    ); // separator
    let mut closed = App::demo();
    closed.update(Action::Escape);
    closed.update(Action::Escape);
    assert_eq!(closed.screen, Screen::Closed);
    click_label(&mut closed, 40, 10, "Esc reopen");
    assert_eq!(closed.screen, Screen::Dashboard);
}
#[test]
fn scrolled_suggestions_click_the_visible_result() {
    let mut app = App::demo();
    app.highlighted = Some(app.suggestions.len() - 1);
    let expected = app.path_label(&app.dirs[*app.suggestions.last().unwrap()].path);
    click(&mut app, 40, 10, 10, 3);
    assert_eq!(app.editor.text, expected);
    assert_eq!(app.screen, Screen::Dashboard);
}
#[test]
fn unavailable_history_and_empty_lists_do_not_launch_or_fall_back() {
    let mut app = App::demo();
    app.update(Action::ToggleCopilot);
    app.update(Action::SelectHistory(app.history[5].id));
    click_label(&mut app, 80, 24, "Enter replay");
    assert_eq!(app.screen, Screen::Dashboard);
    assert!(app.message.as_ref().unwrap().contains("unavailable"));
    click_label(&mut app, 80, 24, "Tab copy");
    click_label(&mut app, 80, 24, "Enter ↵");
    assert_eq!(app.screen, Screen::Dashboard);
    app.history.clear();
    app.suggestions.clear();
    app.focus = Focus::History;
    click_label(&mut app, 80, 24, "Enter replay");
    wheel(&mut app, 80, 24, 10, 14, true);
    assert_eq!(app.recent, 5); // no empty-list navigation or implicit launch
    app.update(Action::SelectHistory(u64::MAX));
    app.update(Action::AcceptSuggestion(usize::MAX));
    app.update(Action::PathCursor(usize::MAX));
    assert_eq!(app.screen, Screen::Dashboard);
}
#[test]
fn path_click_uses_cells_graphemes_padding_and_focus() {
    let mut app = App::demo();
    app.editor.set("修理/e\u{301}👩🏽‍💻");
    app.focus = Focus::Tools;
    click(&mut app, 80, 24, 9, 3); // second cell of 理: before the whole grapheme
    assert_eq!(app.focus, Focus::Path);
    assert_eq!(app.editor.cursor, "修".len());
    click(&mut app, 80, 24, 11, 3);
    assert_eq!(app.editor.cursor, "修理/".len());
    click(&mut app, 80, 24, 12, 3);
    assert_eq!(app.editor.cursor, "修理/e\u{301}".len());
    click(&mut app, 80, 24, 3, 3); // input left padding
    assert_eq!(app.editor.cursor, 0);
    click(&mut app, 80, 24, 74, 3);
    assert_eq!(app.editor.cursor, app.editor.text.len());
    assert_eq!(app.screen, Screen::Dashboard);
}
