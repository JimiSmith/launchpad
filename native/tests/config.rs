use zellij_launchpad::config::Config;
use zellij_launchpad_core::{app::App, commands::Tool};

#[test]
fn ignore_entries_are_validated_individually_and_warnings_survive_reset() {
    use zellij_launchpad_core::app::Action;
    let config: Config = toml::from_str(r#"
ignore = ["/home/ada/cache/../archive/./", "~/cache", "cache", 42, "", "/bad\nname", "C:relative", 'C:\Users\Ada\cache', '/home/ada/$HOME/*']
"#).unwrap();
    let mut app = App::default();
    assert_eq!(
        config.apply(&mut app),
        [
            "/home/ada/archive",
            r"C:\Users\Ada\cache",
            "/home/ada/$HOME/*"
        ]
    );
    assert_eq!(app.ignore_errors.len(), 6);
    assert!(app.ignore_errors[0].starts_with("ignore[2]:"));
    let warnings = app.ignore_errors.clone();
    app.update(Action::Reset);
    assert_eq!(app.ignore_errors, warnings);
    assert_eq!(app.config_errors().count(), 6);
    let help = zellij_launchpad_core::help::lines(&app).join("\n");
    assert!(help.contains("Config error: ignore[2]:"));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|f| zellij_launchpad_core::view::render(f, &app))
        .unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(screen.contains("ignore[2]:"));
}

#[test]
fn ignore_defaults_structure_and_size_validation() {
    let mut app = App::default();
    for text in ["", "ignore = []"] {
        let config: Config = toml::from_str(text).unwrap();
        assert!(config.apply(&mut app).is_empty());
        assert!(app.ignore_errors.is_empty());
    }
    for text in ["ignore = '/absolute/path'", "ignore = 42", "[ignore]"] {
        assert!(toml::from_str::<Config>(text).is_err());
    }
    let text = format!("ignore = ['/{}']", "a".repeat(4096));
    let config: Config = toml::from_str(&text).unwrap();
    assert!(config.apply(&mut app).is_empty());
    assert_eq!(app.ignore_errors.len(), 1);
    Config::default().apply(&mut app);
    assert!(app.ignore_errors.is_empty());
}

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
