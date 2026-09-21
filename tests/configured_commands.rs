use std::collections::BTreeMap;
use zellij_launchpad_core::app::{Action, App, Tool};

fn config(items: &[(&str, &str)]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn configured_ids_select_real_structured_launch_after_validation() {
    let mut app = App::from_remote("/home/fixture".into());
    app.configure(&config(&[
        ("commands", " worktree, hermes,worktree,, other "),
        ("command_worktree", "claude"),
        ("arguments_worktree", "-w"),
        ("arguments_other", "--model 'other model'"),
        ("label_worktree", "Claude in Worktree"),
        ("command_hermes", "hermes"),
        ("command_other", "claude"),
        ("command_unlisted", "ignored"),
    ]));
    assert_eq!(
        app.visible_tools()
            .iter()
            .map(|t| app.tool_label(*t))
            .collect::<Vec<_>>(),
        vec!["Shell", "Claude in Worktree", "hermes", "other"]
    );
    app.update(Action::Focus(zellij_launchpad_core::app::Focus::Tools));
    app.update(Action::Right);
    app.update(Action::Enter);
    let Some(zellij_launchpad_core::remote::RemoteRequest::Validate { generation, .. }) =
        app.take_remote_request()
    else {
        panic!("validation missing")
    };
    app.finish_remote_validation(generation, Ok("/home/fixture/literal; $HOME".into()));
    let launch = app.take_launch().unwrap();
    let definition = app.commands.get(launch.tool).unwrap();
    assert_eq!(definition.id.as_str(), "worktree");
    assert_eq!(definition.executable.as_deref(), Some("claude"));
    assert_eq!(definition.arguments, vec!["-w"]);
    let other = &app.commands.entries[3];
    assert_eq!(other.executable, definition.executable);
    assert_eq!(other.arguments, vec!["--model", "other model"]);
    assert_eq!(launch.path, "/home/fixture/literal; $HOME");
}

#[test]
fn arguments_are_lexed_without_expansion_preserving_empty_and_joined_words() {
    let mut app = App::default();
    app.configure(&config(&[("commands", "x"), ("command_x", "/fixture/path with spaces"),
        ("arguments_x", r#"-w --model "some model" '' "" a\ b pre"joined" '$HOME' ; '$(touch /tmp/no)' ~ *.rs 'a|b' '>'"#)]));
    let c = &app.commands.entries[1];
    assert_eq!(
        c.arguments,
        vec![
            "-w",
            "--model",
            "some model",
            "",
            "",
            "a b",
            "prejoined",
            "$HOME",
            ";",
            "$(touch /tmp/no)",
            "~",
            "*.rs",
            "a|b",
            ">"
        ]
    );
    assert_eq!(c.executable.as_deref(), Some("/fixture/path with spaces"));
}

#[test]
fn malformed_entries_are_diagnosed_and_skipped_without_disabling_valid_commands() {
    let mut app = App::default();
    app.configure(&config(&[
        ("commands", "shell,missing,empty,quote,bad/id,good"),
        ("command_shell", "evil"),
        ("command_empty", "  "),
        ("command_quote", "echo"),
        ("arguments_quote", "'unterminated"),
        ("command_good", "not-installed"),
    ]));
    assert_eq!(app.visible_tools().len(), 2);
    assert_eq!(
        app.commands.entries[1].executable.as_deref(),
        Some("not-installed")
    );
    assert_eq!(app.commands.errors.len(), 5);
    for text in [
        "reserved",
        "command_missing",
        "command_empty",
        "arguments_quote",
        "ID",
    ] {
        assert!(
            app.commands.errors.iter().any(|e| e.contains(text)),
            "missing diagnostic {text}"
        );
    }
    assert!(app.available(Tool::Shell));
}

#[test]
fn configuration_bounds_reject_oversize_fields_and_preserve_neighbor_entries() {
    for (field, value) in [
        ("command_x", "e".repeat(4097)),
        ("command_x", "bad\nexe".into()),
        ("arguments_x", "a".repeat(16385)),
        ("arguments_x", "'' ".repeat(257)),
        ("arguments_x", "nul\0arg".into()),
        ("label_x", "界".repeat(86)),
        ("label_x", "".into()),
        ("label_x", "bad\x1b[1m".into()),
    ] {
        let mut cfg = config(&[
            ("commands", "x,good"),
            ("command_x", "echo"),
            ("command_good", "echo"),
        ]);
        cfg.insert(field.into(), value);
        let mut app = App::default();
        app.configure(&cfg);
        assert_eq!(app.visible_tools().len(), 2, "{field}");
        assert_eq!(app.commands.errors.len(), 1, "{field}");
    }
    let mut cfg = config(&[("commands", &"x".repeat(8193))]);
    let mut app = App::default();
    app.configure(&cfg);
    assert_eq!(app.visible_tools(), vec![Tool::Shell]);
    assert!(app.commands.errors[0].contains("8192"));
    cfg.insert(
        "commands".into(),
        (0..65)
            .map(|n| format!("v{n}"))
            .collect::<Vec<_>>()
            .join(","),
    );
    for n in 0..65 {
        cfg.insert(format!("command_v{n}"), "echo".into());
    }
    app.configure(&cfg);
    assert_eq!(app.visible_tools().len(), 65);
    assert!(app.commands.errors[0].contains("64"));
    let id = "x".repeat(64);
    let mut cfg = config(&[("commands", &id)]);
    cfg.insert(format!("command_{id}"), "e".repeat(4096));
    cfg.insert(format!("label_{id}"), "l".repeat(256));
    cfg.insert(format!("arguments_{id}"), "a".repeat(16384));
    app.configure(&cfg);
    assert_eq!(app.visible_tools().len(), 2);
    assert!(app.commands.errors.is_empty());
    assert!(Tool::new(&"x".repeat(65)).is_none());
}

#[test]
fn many_long_unicode_labels_keep_selected_control_visible_and_clickable() {
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use zellij_launchpad_core::{
        app::Focus,
        view::{Pointer, render_with_hits},
    };
    let mut cfg = config(&[(
        "commands",
        &(0..64)
            .map(|n| format!("v{n}"))
            .collect::<Vec<_>>()
            .join(","),
    )]);
    for n in 0..64 {
        cfg.insert(format!("command_v{n}"), "fixture".into());
        cfg.insert(
            format!("label_v{n}"),
            format!("{n} 界é 👩🏽‍💻 {}", "long label ".repeat(12)),
        );
    }
    for (w, h) in [(80, 24), (40, 12), (40, 10), (200, 36)] {
        let mut app = App::default();
        app.configure(&cfg);
        app.update(Action::Focus(Focus::Tools));
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        for _ in 0..65 {
            let mut hits = None;
            terminal
                .draw(|f| hits = Some(render_with_hits(f, &app)))
                .unwrap();
            let hits = hits.unwrap();
            let area = Rect::new(0, 0, w, h);
            assert!(
                (0..h - 1).any(|y| (0..w).any(|x| hits.action(Pointer::Click, x, y, area)
                    == Some(Action::SelectTool(app.tool)))),
                "selected {} at {w}x{h}",
                app.tool.as_str()
            );
            for y in 0..h {
                for x in 0..w {
                    if w > 160 && !(20..180).contains(&x) {
                        assert!(hits.action(Pointer::Click, x, y, area).is_none());
                        assert_eq!(terminal.backend().buffer()[(x, y)].symbol(), " ");
                    }
                }
            }
            app.update(Action::Right);
        }
        assert_eq!(app.tool, Tool::Shell);
    }
}

#[test]
fn every_wrapped_help_cell_is_reachable_at_narrow_and_wide_sizes() {
    use ratatui::{Terminal, backend::TestBackend};
    use zellij_launchpad_core::{cells::width, view::render_with_hits};
    for label in [
        format!("{} END_OF_LABEL", "x".repeat(243)),
        format!("{} END_OF_LABEL", "界é👩🏽‍💻🇬🇧✈️ ".repeat(5)),
    ] {
        for (w, h) in [(40, 10), (80, 24), (200, 36)] {
            let id = "i".repeat(64);
            let mut app = App::default();
            app.configure(&BTreeMap::from([
                ("commands".into(), id.clone()),
                (format!("command_{id}"), "fixture".into()),
                (format!("label_{id}"), label.clone()),
            ]));
            assert!(app.commands.errors.is_empty());
            app.update(Action::Help);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let mut collected = String::new();
            for step in 0..1000 {
                // Inspect the completed render buffer, not TestBackend's diff replay
                // (it does not emulate terminal erasure of wide continuations).
                let frame = terminal
                    .draw(|f| {
                        render_with_hits(f, &app);
                    })
                    .unwrap();
                let row = |y| {
                    let mut line = String::new();
                    let mut x = 0;
                    while x < w {
                        let symbol = frame.buffer[(x, y)].symbol();
                        line.push_str(symbol);
                        x += width(symbol).max(1) as u16;
                    }
                    line
                };
                collected.push_str(&row(2));
                let before = app.help_scroll;
                app.update(Action::Down);
                if app.help_scroll == before {
                    for y in 3..h - 1 {
                        collected.push_str(&row(y));
                    }
                    break;
                }
                assert!(step < 999, "help scroll must be bounded");
            }
            let compact = |s: String| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            assert_eq!(
                compact(collected),
                compact(zellij_launchpad_core::help::lines(&app).join("")),
                "all text reachable at {w}x{h}"
            );
            app.update(Action::Home);
            assert_eq!(app.help_scroll, 0);
            app.update(Action::End);
            terminal
                .draw(|f| {
                    render_with_hits(f, &app);
                })
                .unwrap();
            let end: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(
                end.contains("END_OF_LABEL"),
                "End reaches final label at {w}x{h}"
            );
            app.update(Action::Up);
            let before = app.help_scroll;
            app.update(Action::Down);
            assert_eq!(
                app.help_scroll,
                before + 1,
                "Up after End moves immediately"
            );
        }
    }
}

#[test]
fn unavailable_long_history_ids_keep_a_visible_marker_and_safe_mouse_actions() {
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use zellij_launchpad_core::{
        app::{Focus, Launch},
        view::{Pointer, render_with_hits},
    };
    for (w, h) in [(40, 10), (40, 12), (80, 24), (200, 36)] {
        let mut app = App::from_remote("/fixture".into());
        app.take_remote_request();
        app.history.push(Launch {
            id: 7,
            path: "/fixture/old".into(),
            tool: Tool::new("removed-long-id").unwrap(),
            age: "1h ago".into(),
        });
        app.focus = Focus::History;
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut hits = None;
        let frame = terminal
            .draw(|f| hits = Some(render_with_hits(f, &app)))
            .unwrap();
        let hits = hits.unwrap();
        let area = Rect::new(0, 0, w, h);
        let (x, y) = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .find(|&(x, y)| {
                hits.action(Pointer::Click, x, y, area) == Some(Action::SelectHistory(7))
            })
            .unwrap();
        assert!(
            (x..x + 12).any(|col| frame.buffer[(col, y)].symbol() == "!"),
            "unavailable marker must survive tool clipping at {w}x{h}"
        );
        assert_eq!(
            frame.buffer[(x + 24, y)].symbol(),
            "~",
            "status must not shift directory cells"
        );
        app.update(hits.action(Pointer::Click, x + 4, y, area).unwrap());
        assert_eq!(app.recent, 0);
        assert!(app.take_launch().is_none(), "selection never launches");
        app.update(Action::Enter);
        let Some(zellij_launchpad_core::remote::RemoteRequest::Validate { generation, .. }) =
            app.take_remote_request()
        else {
            panic!("history validation missing")
        };
        app.finish_remote_validation(generation, Ok("/fixture/old".into()));
        assert!(app.message.as_ref().unwrap().contains("unavailable"));
        assert!(app.take_launch().is_none());
        app.update(Action::Tab);
        assert_eq!(app.tool.as_str(), "removed-long-id");
        app.update(Action::LaunchForm);
        let Some(zellij_launchpad_core::remote::RemoteRequest::Validate { generation, .. }) =
            app.take_remote_request()
        else {
            panic!("copied path validation missing")
        };
        app.finish_remote_validation(generation, Ok("/fixture/old".into()));
        assert!(app.message.as_ref().unwrap().contains("unavailable"));
        assert!(
            app.take_launch().is_none(),
            "copy must not substitute an available command"
        );
    }
}

#[test]
fn unconfigured_production_has_only_initial_shell() {
    let app = App::default();
    assert_eq!(app.visible_tools(), vec![Tool::Shell]);
    assert_eq!(app.tool, Tool::Shell);
}
