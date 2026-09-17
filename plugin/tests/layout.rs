#[test]
fn sample_layout_parses_in_exact_host_sdk() {
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/launchpad.kdl");
    let layout = zellij_utils::input::layout::Layout::from_path_without_config(&path).unwrap();
    assert!(layout.has_tabs());
    assert_eq!(layout.tabs().len(), 2);
}
