use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};
use zellij_launchpad_core::{
    app::App,
    commands::{Command, Commands, Tool},
    shortcut::Shortcut,
    theme::Theme,
};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    commands: Vec<toml::Value>,
    theme: BTreeMap<String, String>,
    ignore: Vec<toml::Value>,
    /// Shell's executable; unset means the host's default shell.
    default_shell: Option<toml::Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    id: String,
    label: Option<String>,
    executable: String,
    #[serde(default)]
    arguments: Vec<String>,
    shortcut: Option<String>,
}
impl Config {
    pub fn load(path: &Path, explicit: bool) -> Result<Self, String> {
        use std::io::Read;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if !explicit && e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(e) => return Err(format!("Cannot read config {}: {e}", path.display())),
        };
        let mut text = String::new();
        file.take(1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        if text.len() > 1024 * 1024 {
            return Err("Config exceeds 1 MiB".into());
        }
        toml::from_str(&text).map_err(|e| format!("Invalid config {}: {e}", path.display()))
    }
    /// Apply UI settings and return validated exclusions for the search worker.
    pub fn apply(self, app: &mut App) -> Vec<String> {
        let mut ignore = Vec::new();
        app.ignore_errors.clear();
        for (index, value) in self.ignore.iter().enumerate() {
            match value
                .as_str()
                .and_then(zellij_launchpad_core::host_path::normalize_absolute)
            {
                Some(path) => ignore.push(path),
                None => app.ignore_errors.push(format!(
                    "ignore[{}]: require a literal absolute path of at most 4096 bytes without controls; entry skipped",
                    index + 1
                )),
            }
        }
        let mut commands = Commands::default();
        if let Some(value) = &self.default_shell {
            match value.as_str().filter(|s| valid_executable(s)) {
                Some(shell) => commands.entries[0].executable = Some(shell.trim().into()),
                None => commands.errors.push(
                    "default_shell: require an executable of at most 4096 bytes without controls; using the default shell".into(),
                ),
            }
        }
        let mut seen = HashSet::new();
        for value in self.commands {
            let definition: Definition = match value.try_into() {
                Ok(d) => d,
                Err(e) => {
                    commands.errors.push(format!("Command: {e}"));
                    continue;
                }
            };
            if !seen.insert(definition.id.clone()) {
                continue;
            }
            if seen.len() > 64 {
                commands
                    .errors
                    .push("commands exceeds 64 unique IDs; remaining entries skipped".into());
                break;
            }
            let shortcut = definition.shortcut.clone();
            let mut command = match definition.validate() {
                Ok(command) => command,
                Err(error) => {
                    commands.errors.push(error);
                    continue;
                }
            };
            // A bad shortcut is dropped; the command itself stays usable.
            if let Some(text) = shortcut {
                let fail = |s: String| format!("{}: shortcut {text:?}: {s}", command.id.as_str());
                match Shortcut::parse(&text) {
                    Err(e) => commands.errors.push(fail(e)),
                    Ok(s) if crate::input::clashes(s) => commands
                        .errors
                        .push(fail("conflicts with a built-in key".into())),
                    Ok(s) => match commands.by_shortcut(s) {
                        Some(other) => commands
                            .errors
                            .push(fail(format!("already used by {}", other.as_str()))),
                        None => command.shortcut = Some(s),
                    },
                }
            }
            commands.entries.push(command);
        }
        app.commands = commands;
        let known = [
            "background",
            "surface",
            "raised",
            "border",
            "text",
            "muted",
            "accent",
            "on_accent",
            "error",
        ];
        let values = self
            .theme
            .iter()
            .map(|(k, v)| (format!("theme_{k}"), v.clone()))
            .collect();
        (app.theme, app.theme_errors) = Theme::parse(&values);
        for key in self.theme.keys().filter(|k| !known.contains(&k.as_str())) {
            app.theme_errors
                .push(format!("Unknown theme colour: {key}"));
        }
        ignore
    }
}
impl Definition {
    fn validate(self) -> Result<Command, String> {
        let fail = |s| format!("{}: {s}", self.id);
        let id = Tool::new(&self.id).ok_or_else(|| fail("invalid command ID"))?;
        if id == Tool::Shell {
            return Err(fail("shell is reserved; set default_shell instead"));
        }
        if !valid_executable(&self.executable) {
            return Err(fail(
                "require an executable of at most 4096 bytes without controls",
            ));
        }
        if self.arguments.len() > 256
            || self.arguments.iter().map(String::len).sum::<usize>() > 16384
            || self.arguments.iter().any(|a| a.contains('\0'))
        {
            return Err(fail(
                "arguments: maximum 256 arguments / 16384 bytes, no NUL",
            ));
        }
        let label = self.label.as_deref().unwrap_or(&self.id);
        if label.trim().is_empty() || label.len() > 256 || label.chars().any(char::is_control) {
            return Err(fail(
                "label: require nonempty text, at most 256 bytes, no controls",
            ));
        }
        Ok(Command {
            id,
            label: label.into(),
            executable: Some(self.executable),
            arguments: self.arguments,
            shortcut: None,
        })
    }
}
fn valid_executable(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= 4096 && !text.chars().any(char::is_control)
}
pub fn xdg_path(variable: &str, fallback: &str, home: &Path) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(fallback))
        .join("zellij-launchpad")
}

/// HOME remains an explicit override; Windows normally supplies USERPROFILE.
pub fn home_from_env(get: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    let get = |key| get(key).filter(|value| !value.is_empty());
    get("HOME")
        .or_else(|| get("USERPROFILE"))
        .or_else(|| {
            let mut home = get("HOMEDRIVE")?;
            home.push(get("HOMEPATH")?);
            Some(home)
        })
        .map(PathBuf::from)
}
