pub use crate::search::Directory;
pub fn history() -> Vec<crate::app::Launch> {
    use crate::app::{Launch, Tool};

    [
        (Tool::Claude, "launchpad", "12m ago"),
        (Tool::Shell, "notes", "38m ago"),
        (Tool::Codex, "service", "1h ago"),
        (Tool::Hermes, "team notes", "2h ago"),
        (Tool::Claude, "launchpad", "3h ago"),
        (Tool::Copilot, "service", "4h ago"),
        (Tool::Shell, "current", "Yesterday"),
        (Tool::Codex, "修理", "Yesterday"),
        (Tool::Hermes, "notes", "Yesterday"),
        (Tool::Shell, "it's literal; $HOME", "2d ago"),
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
    .map(|(path, note, error)| Directory {
        path: path.into(),
        note,
        error,
    })
    .collect()
}
/// Lexical fixture resolution only: never consults the host or a shell.
pub fn normalize(raw: &str) -> Option<String> {
    crate::search::normalize_in(raw, HOME, CWD)
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
    crate::search::matches_in(raw, dirs, HOME, CWD)
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
