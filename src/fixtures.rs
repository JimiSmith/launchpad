#[derive(Debug, Clone)]
pub struct Directory {
    pub path: &'static str,
    pub note: &'static str,
    pub error: Option<&'static str>,
}
pub fn history() -> Vec<crate::app::Launch> {
    use crate::app::{Launch, Tool::*};
    [
        (Claude, "launchpad", "12m ago"),
        (Shell, "notes", "38m ago"),
        (Codex, "service", "1h ago"),
        (Hermes, "team notes", "2h ago"),
        (Claude, "launchpad", "3h ago"),
        (Copilot, "service", "4h ago"),
        (Shell, "current", "Yesterday"),
        (Codex, "修理", "Yesterday"),
        (Hermes, "notes", "Yesterday"),
        (Shell, "it's literal; $HOME", "2d ago"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (tool, name, age))| Launch {
        id: 10 - i as u64,
        path: format!("{CWD}/{name}"),
        tool,
        age: age.into(),
    })
    .collect()
}
pub const HOME: &str = "/home/demo";
pub const CWD: &str = "/home/demo/Projects";
pub fn directories() -> Vec<Directory> {
    [
        ("/home/demo", "demo home", None),
        ("/home/demo/Projects", "invoking directory", None),
        ("/home/demo/Projects/launchpad", "project", None),
        ("/home/demo/Projects/notes", "project", None),
        ("/home/demo/Projects/service", "project", None),
        ("/home/demo/Projects/service/api", "nested directory", None),
        (
            "/home/demo/Projects/research/notes",
            "nested directory",
            None,
        ),
        ("/home/demo/Projects/team notes", "spaces are literal", None),
        ("/home/demo/Projects/修理", "Unicode directory", None),
        ("/home/demo/Projects/café", "accented name", None),
        (
            "/home/demo/Projects/it's literal; $HOME",
            "literal filename",
            None,
        ),
        ("/home/demo/Projects/.archive", "hidden directory", None),
        ("/home/demo/Projects/current", "symlink → launchpad", None),
        ("/home/demo/Documents", "directory", None),
        (
            "/home/demo/restricted",
            "permission denied",
            Some("Permission denied. Choose an accessible fixture."),
        ),
        (
            "/home/demo/broken-link",
            "broken symlink",
            Some("Broken symlink. Choose another fixture directory."),
        ),
        (
            "/home/demo/Projects/missing",
            "missing directory",
            Some("Directory missing. Choose a suggestion; nothing was created."),
        ),
    ]
    .into_iter()
    .map(|(path, note, error)| Directory { path, note, error })
    .collect()
}
/// Lexical fixture resolution only: never consults the host or a shell.
pub fn normalize(raw: &str) -> Option<String> {
    if raw.is_empty()
        || raw.chars().any(char::is_control)
        || (raw.starts_with('~') && raw != "~" && !raw.starts_with("~/"))
    {
        return None;
    }
    let path = if raw == "~" {
        HOME.to_owned()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        format!("{HOME}/{rest}")
    } else if raw.starts_with('/') {
        raw.to_owned()
    } else {
        format!("{CWD}/{raw}")
    };
    let mut parts = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(p),
        }
    }
    Some(format!("/{}", parts.join("/")))
}
pub fn short(path: &str) -> String {
    if path == HOME {
        "~".into()
    } else if let Some(rest) = path.strip_prefix(&format!("{HOME}/")) {
        format!("~/{rest}")
    } else {
        path.into()
    }
}
/// Frizbee scores both basename and complete path; ties use logical path order.
/// Use Matcher::new, NOT query syntax: quotes, spaces and $ remain literal.
pub fn matches(raw: &str, dirs: &[Directory]) -> Vec<usize> {
    use neo_frizbee::{Config, Matcher};
    let config = Config {
        max_typos: Some(0),
        ..Config::default()
    };
    let explicit = raw.starts_with('/')
        || raw.starts_with('~')
        || raw.starts_with("./")
        || raw.starts_with("../");
    let query = if explicit {
        normalize(raw).unwrap_or_else(|| raw.into())
    } else {
        raw.to_owned()
    };
    let allow_hidden = raw
        .split('/')
        .any(|p| p.starts_with('.') && p != "." && p != "..");
    let candidates: Vec<_> = dirs
        .iter()
        .enumerate()
        .filter(|(_, d)| allow_hidden || !d.path.split('/').any(|p| p.starts_with('.')))
        .collect();
    let paths: Vec<_> = candidates.iter().map(|(_, d)| d.path).collect();
    let names: Vec<_> = candidates
        .iter()
        .map(|(_, d)| d.path.rsplit('/').next().unwrap_or(d.path))
        .collect();
    let mut scores = vec![None; candidates.len()];
    let mut matcher = Matcher::new(&query, &config);
    for m in matcher
        .match_list(&paths)
        .into_iter()
        .chain(matcher.match_list(&names))
    {
        let score = &mut scores[m.index as usize];
        *score = Some(score.unwrap_or(0).max(m.score));
    }
    let mut ranked: Vec<_> = scores
        .into_iter()
        .enumerate()
        .filter_map(|(i, score)| score.map(|s| (candidates[i].0, s)))
        .collect();
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| dirs[a.0].path.cmp(dirs[b.0].path))
    });
    ranked.into_iter().map(|(i, _)| i).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frizbee_finds_bare_nested_and_noncontiguous_names() {
        let dirs = directories();
        for query in ["notes", "nts", "prj/nts"] {
            let results = matches(query, &dirs);
            assert!(
                results
                    .iter()
                    .any(|&i| dirs[i].path == "/home/demo/Projects/notes"),
                "{query}"
            );
            assert_eq!(results, matches(query, &dirs));
        }
        assert!(
            matches("notes", &dirs)
                .iter()
                .any(|&i| dirs[i].path.contains("research/notes"))
        );
        assert!(matches("archive", &dirs).is_empty());
        assert!(!matches(".archive", &dirs).is_empty());
        assert!(!matches("修理", &dirs).is_empty());
        assert_eq!(normalize("./notes/"), Some(format!("{CWD}/notes")));
        assert_eq!(normalize("../Documents"), Some(format!("{HOME}/Documents")));
        assert_eq!(
            normalize("~/Projects/it's literal; $HOME"),
            Some(format!("{CWD}/it's literal; $HOME"))
        );
        assert_eq!(normalize("~someone"), None);
        assert_eq!(short(&format!("{CWD}/notes")), "~/Projects/notes");
    }
}
