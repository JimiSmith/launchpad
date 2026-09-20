//! Stable command identity, independent of labels and executable definitions.
use std::collections::BTreeMap;

/// Inline bounded identity keeps actions cheap to copy without interning/leaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tool {
    bytes: [u8; 64],
    len: u8,
}
#[allow(non_upper_case_globals)]
impl Tool {
    const fn literal(id: &str) -> Self {
        let mut bytes = [0; 64];
        let mut i = 0;
        while i < id.len() {
            bytes[i] = id.as_bytes()[i];
            i += 1;
        }
        Self {
            bytes,
            len: id.len() as u8,
        }
    }
    /// The one built-in identity: Zellij's configured default shell.
    pub const Shell: Self = Self::literal("shell");
    pub fn new(id: &str) -> Option<Self> {
        (!id.is_empty()
            && id.len() <= 64
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)))
        .then(|| Self::literal(id))
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub id: Tool,
    pub label: String,
    pub executable: Option<String>,
    pub arguments: Vec<String>,
}
#[derive(Debug, Clone)]
pub struct Commands {
    pub entries: Vec<Command>,
    pub errors: Vec<String>,
}
impl Default for Commands {
    fn default() -> Self {
        Self {
            entries: vec![Command {
                id: Tool::Shell,
                label: "Shell".into(),
                executable: None,
                arguments: Vec::new(),
            }],
            errors: Vec::new(),
        }
    }
}
impl Commands {
    pub fn parse(configuration: &BTreeMap<String, String>) -> Self {
        let mut result = Self::default();
        if configuration
            .get("commands")
            .is_some_and(|s| s.len() > 8192)
        {
            result
                .errors
                .push("commands exceeds 8192 bytes; Shell only".into());
            return result;
        }
        let mut seen = std::collections::HashSet::new();
        for name in configuration
            .get("commands")
            .map(String::as_str)
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if !seen.insert(name) {
                continue;
            }
            if seen.len() > 64 {
                result
                    .errors
                    .push("commands exceeds 64 unique IDs; remaining entries skipped".into());
                break;
            }
            let definition = (|| -> Result<Command, String> {
                let id = Tool::new(name)
                    .ok_or("Invalid command ID (use 1–64 ASCII letters, digits, _, . or -)")?;
                if id == Tool::Shell {
                    return Err("shell is reserved for Zellij's default shell".into());
                }
                let executable = configuration
                    .get(&format!("command_{name}"))
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| format!("command_{name}: executable is required"))?;
                if executable.len() > 4096 || executable.chars().any(char::is_control) {
                    return Err(format!(
                        "command_{name}: maximum 4096 bytes, no control characters"
                    ));
                }
                let raw = configuration
                    .get(&format!("arguments_{name}"))
                    .map(String::as_str)
                    .unwrap_or("");
                if raw.len() > 16384 || raw.contains('\0') {
                    return Err(format!("arguments_{name}: maximum 16384 bytes, no NUL"));
                }
                let arguments = shlex::split(raw).ok_or_else(|| {
                    format!("arguments_{name}: malformed quoting or trailing escape")
                })?;
                if arguments.len() > 256 {
                    return Err(format!("arguments_{name}: maximum 256 arguments"));
                }
                let label = configuration
                    .get(&format!("label_{name}"))
                    .cloned()
                    .unwrap_or_else(|| name.into());
                if label.trim().is_empty()
                    || label.len() > 256
                    || label.chars().any(char::is_control)
                {
                    return Err(format!(
                        "label_{name}: require nonempty text, at most 256 bytes, no controls"
                    ));
                }
                Ok(Command {
                    id,
                    label,
                    executable: Some(executable.clone()),
                    arguments,
                })
            })();
            match definition {
                Ok(c) => result.entries.push(c),
                Err(e) => result.errors.push(e),
            }
        }
        result
    }
    pub fn get(&self, id: Tool) -> Option<&Command> {
        self.entries.iter().find(|c| c.id == id)
    }
}
