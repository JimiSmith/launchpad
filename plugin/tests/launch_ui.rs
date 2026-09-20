use zellij_launchpad::State;
use zellij_launchpad_core::app::Action;

#[test]
fn dashboard_and_help_describe_replacement_not_simulation() {
    let mut state = State::default();
    let frame = state.render_frame(36, 120);
    assert!(frame.contains("replace this pane"));
    assert!(!frame.contains("suppressed"));
    state.app.update(Action::Help);
    let frame = state.render_frame(50, 160);
    assert!(
        frame.contains("No command availability checks"),
        "{frame:?}"
    );
    assert!(!frame.contains("simulate"), "{frame:?}");
    assert!(!frame.contains("Copilot"), "no fixture toggle remains");
}

#[test]
fn the_harness_mode_labels_itself_in_the_header_and_in_help() {
    let mut state = State::default();
    state.app.simulate_launch = true;
    assert!(state.render_frame(36, 120).contains("launch suppressed"));
    state.app.update(Action::Help);
    assert!(state.render_frame(50, 160).contains("simulate_launch"));
}

#[test]
fn escape_closes_an_untouched_dashboard_without_a_fake_closed_screen() {
    let mut state = State::default();
    state.app.update(Action::Escape);
    assert!(state.app.quit, "Esc asks the host to close our own pane");
}
