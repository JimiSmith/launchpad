#[test]
fn configured_example_forwards_exact_values_from_pinned_kdl_parser() {
    use zellij_utils::input::layout::{Layout, Run, TiledPaneLayout};
    fn plugin_config(pane: &TiledPaneLayout) -> Option<std::collections::BTreeMap<String, String>> {
        if let Some(Run::Plugin(plugin)) = &pane.run {
            return plugin.get_configuration().map(|c| c.inner().clone());
        }
        pane.children.iter().find_map(plugin_config)
    }
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/configured.kdl");
    let layout = Layout::from_path_without_config(&path).unwrap();
    let config = plugin_config(&layout.tabs()[0].1).unwrap();
    let parsed = zellij_launchpad_core::commands::Commands::parse(&config);
    assert!(parsed.errors.is_empty());
    assert_eq!(
        parsed
            .entries
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        vec!["shell", "claude", "hermes", "codex"]
    );
    assert_eq!(parsed.entries[1].arguments, vec!["-w"]);
    assert_eq!(parsed.entries[1].label, "Claude in Worktree");
}

#[test]
fn documented_argument_examples_parse_and_produce_literal_argv() {
    use zellij_utils::input::layout::{Layout, Run};
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let text = std::fs::read_to_string(root.join("docs/configured-commands.md")).unwrap();
    let snippets: Vec<_> = text
        .split("```kdl\n")
        .skip(1)
        .map(|s| s.split("```").next().unwrap())
        .collect();
    assert_eq!(snippets.len(), 3);
    let target = root.join("target/configured-commands/doc-examples");
    std::fs::create_dir_all(&target).unwrap();
    for (n, snippet) in snippets.iter().enumerate() {
        let extra = if n == 1 {
            "commands \"claude\"; command_claude \"claude\";\n"
        } else {
            ""
        };
        let layout = format!("layout {{ tab {{ pane {{ plugin location=\"file:/fixture.wasm\" {{\n{extra}{snippet}\n}}; }}; }}; }}");
        let path = target.join(format!("{n}.kdl"));
        std::fs::write(&path, layout).unwrap();
        let layout = Layout::from_path_without_config(&path).unwrap();
        let pane = &layout.tabs()[0].1.children[0];
        let Some(Run::Plugin(plugin)) = &pane.run else {
            panic!("plugin missing")
        };
        let parsed = zellij_launchpad_core::commands::Commands::parse(
            plugin.get_configuration().unwrap().inner(),
        );
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        if n == 1 {
            assert_eq!(
                parsed.entries[1].arguments,
                vec!["-w", "--model", "some model", ""]
            );
        }
        if n == 2 {
            assert_eq!(
                parsed.entries[1].arguments,
                vec!["-c", "printf \"%s\\n\" \"$PWD\""]
            );
        }
    }
}

#[test]
fn sample_layout_parses_in_exact_host_sdk() {
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/launchpad.kdl");
    let layout = zellij_utils::input::layout::Layout::from_path_without_config(&path).unwrap();
    assert!(layout.has_tabs());
    assert_eq!(layout.tabs().len(), 2);
}
