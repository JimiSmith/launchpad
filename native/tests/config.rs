use zellij_launchpad::config::Config;
use zellij_launchpad_core::{app::App, commands::Tool};

#[test]
fn toml_arguments_remain_literal_and_invalid_entries_do_not_hide_valid_commands() {
    let config: Config = toml::from_str(
        r##"
[[commands]]
id = "bad"
executable = 17
[[commands]]
id = "shell"
executable = "no"
[[commands]]
id = "agent"
label = "Agent in Worktree"
executable = "agent"
arguments = ["-w", "a b", "", "$HOME", ";", "$(touch never)", "*.rs"]
[[commands]]
id = "agent"
executable = "ignored-duplicate"
[theme]
accent = "#112233"
error = "invalid"
"##,
    )
    .unwrap();
    let mut app = App::default();
    config.apply(&mut app);
    assert_eq!(app.commands.entries.len(), 2);
    assert_eq!(app.commands.entries[0].id, Tool::Shell);
    assert_eq!(
        app.commands.entries[1].arguments,
        ["-w", "a b", "", "$HOME", ";", "$(touch never)", "*.rs"]
    );
    assert_eq!(app.commands.errors.len(), 2);
    assert_eq!(app.theme_errors.len(), 1);
    assert_eq!(app.theme.accent, ratatui::style::Color::Rgb(17, 34, 51));
}
#[test]
fn missing_default_is_shell_only_but_explicit_missing_is_an_error() {
    let missing = std::path::Path::new("/nonexistent/launchpad-config.toml");
    let mut app = App::default();
    Config::load(missing, false).unwrap().apply(&mut app);
    assert_eq!(app.commands.entries.len(), 1);
    assert!(Config::load(missing, true).is_err());
    assert!(toml::from_str::<Config>("commands = ???").is_err());
}
