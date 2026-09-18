use launchpad_plugin::State;
use zellij_launchpad_prototype::app::{Action, App, Screen};

#[test]
fn host_dashboard_and_help_describe_replacement_not_simulation() {
    let mut state = State::default();
    state.app.host_launch = true;
    let frame = state.render_frame(36, 120);
    assert!(frame.contains("replace this pane"));
    assert!(!frame.contains("SIMULATED"));
    state.app.update(Action::Help);
    let frame = state.render_frame(50, 160);
    assert!(
        frame.contains("No command availability checks"),
        "{frame:?}"
    );
    assert!(!frame.contains("simulate"));
    assert!(!frame.contains("Toggle Copilot"));
    let mut demo = State::default();
    demo.app = App::demo();
    assert!(demo.render_frame(36, 120).contains("DEMO"));
}
#[test]
fn escape_closes_real_untouched_dashboard_without_fake_closed_screen() {
    let mut state = State::default();
    state.app.host_launch = true;
    state.app.update(Action::Escape);
    assert!(state.app.quit);
    assert_eq!(state.app.screen, Screen::Dashboard);
}
