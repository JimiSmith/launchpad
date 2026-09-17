use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use zellij_launchpad_prototype::search::HomeIndex;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "launchpad-search-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn dir(&self, path: &str) {
        fs::create_dir_all(self.0.join(path)).unwrap();
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn skips_symlinks_and_unrepresentable_names() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    let outside = Tree::new();
    outside.dir("escape/secret");
    tree.dir("normal");
    tree.dir("bad\nname");
    symlink(&outside.0, tree.0.join("escape")).unwrap();
    symlink(&tree.0, tree.0.join("cycle")).unwrap();
    symlink(tree.0.join("normal"), tree.0.join("internal-link")).unwrap();
    let mut index = HomeIndex::new(tree.0.clone(), tree.0.clone()).unwrap();
    index.step(100);
    assert!(!index.is_scanning());
    assert_eq!(
        index.dirs.len(),
        1,
        "only normal; links and control names are not selectable"
    );
    assert!(index.dirs[0].path.ends_with("normal"));
}

#[test]
fn validates_literal_home_paths_not_files_or_escapes() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    let outside = Tree::new();
    tree.dir("team notes/修理");
    tree.dir("it's literal; $HOME");
    fs::write(tree.0.join("file"), "text").unwrap();
    symlink(&outside.0, tree.0.join("link")).unwrap();
    let index = HomeIndex::new(tree.0.clone(), tree.0.clone()).unwrap();
    for raw in [
        "~",
        ".",
        "~/team notes/修理",
        "team notes/修理",
        "it's literal; $HOME",
    ] {
        assert!(index.validate(raw).is_ok(), "{raw}");
    }
    for raw in [
        "",
        "~other",
        "/",
        "..",
        "../elsewhere",
        "~/file",
        "~/missing",
        "~/link",
        "~/link/../team notes",
    ] {
        assert!(index.validate(raw).is_err(), "{raw}");
    }
    let absolute = tree.0.join("team notes").to_str().unwrap().to_owned();
    assert_eq!(index.validate(&absolute).unwrap(), absolute);
    fs::remove_dir(tree.0.join("team notes/修理")).unwrap();
    assert!(index.validate("~/team notes/修理").is_err());
}

#[test]
fn bounded_scan_reports_limits_and_read_errors() {
    let tree = Tree::new();
    tree.dir("a/b/c");
    let mut index = HomeIndex::new(tree.0.clone(), tree.0.clone()).unwrap();
    index.limits.max_directories = 1;
    index.step(100);
    assert!(!index.is_scanning());
    assert_eq!(index.dirs.len(), 1);
    assert!(index.status().contains("limit"));
    let mut missing = HomeIndex::new(tree.0.join("missing"), tree.0.join("missing")).unwrap();
    missing.step(100);
    assert!(!missing.is_scanning());
    assert!(missing.status().contains("unavailable"));
}

#[test]
fn app_searches_real_home_accepts_then_revalidates_without_invented_history() {
    use zellij_launchpad_prototype::app::{Action, App, Screen};
    let tree = Tree::new();
    tree.dir("Projects/research/notes");
    tree.dir("Projects/.archive/notes");
    tree.dir("team notes/修理");
    let mut app = App::from_home(tree.0.clone(), tree.0.clone());
    assert!(app.history.is_empty());
    for _ in 0..100 {
        app.index_tick();
    }
    let query = |app: &mut App, text: &str| {
        app.update(Action::Clear);
        app.update(Action::Text(text.into()));
    };
    query(&mut app, "nts");
    assert!(
        app.suggestions
            .iter()
            .any(|&i| app.dirs[i].path.ends_with("research/notes"))
    );
    assert!(
        !app.suggestions
            .iter()
            .any(|&i| app.dirs[i].path.contains(".archive"))
    );
    query(&mut app, "~/Projects/.archive");
    assert!(!app.suggestions.is_empty());
    query(&mut app, "修理");
    app.update(Action::Tab);
    assert_eq!(app.editor.text, "~/team notes/修理");
    assert_eq!(app.screen, Screen::Dashboard);
    fs::remove_dir(tree.0.join("team notes/修理")).unwrap();
    app.update(Action::Enter);
    assert_eq!(app.screen, Screen::Dashboard);
    assert!(app.message.as_ref().unwrap().contains("unavailable"));
    assert!(app.history.is_empty());
    query(&mut app, "~/Projects/research/notes");
    app.update(Action::Enter);
    assert!(matches!(app.screen, Screen::Terminal(_)));
    assert_eq!(app.history.len(), 1);
    app.update(Action::Reset);
    for _ in 0..100 {
        app.index_tick();
    }
    query(&mut app, "nts");
    assert!(!app.suggestions.is_empty(), "F5 keeps the real home source");
    assert!(app.history.is_empty());
}

#[test]
fn background_indexing_respects_dismissal_and_history_copy() {
    use zellij_launchpad_prototype::app::{Action, App, Focus};
    let tree = Tree::new();
    for i in 0..300 {
        tree.dir(&format!("notes-{i}"));
    }
    let mut app = App::from_home(tree.0.clone(), tree.0.clone());
    app.index_tick();
    assert!(app.is_indexing());
    app.update(Action::Escape);
    app.index_tick();
    assert!(
        app.suggestions.is_empty(),
        "background work cannot reopen dismissed suggestions"
    );
    app.update(Action::Clear);
    app.update(Action::Text("~".into()));
    app.update(Action::Enter); // ~ validates without needing an indexed candidate
    app.update(Action::Escape);
    app.update(Action::Focus(Focus::History));
    app.update(Action::Tab);
    app.index_tick();
    assert!(
        app.suggestions.is_empty(),
        "history copy must remain dismissed"
    );
}

#[test]
fn restricted_state_is_visible_at_minimum_size() {
    use ratatui::{Terminal, backend::TestBackend};
    use zellij_launchpad_prototype::{app::App, view};
    let mut app = App::default();
    app.search_status = "HOME access denied. Reopen plugin.".into();
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    terminal.draw(|f| view::render(f, &app)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("HOME access denied"));
}

#[test]
fn logical_home_is_distinct_from_filesystem_mapping() {
    let tree = Tree::new();
    tree.dir("nested/notes");
    let mut index = HomeIndex::new("/logical/user-home".into(), tree.0.clone()).unwrap();
    for _ in 0..100 {
        index.step(1);
    }
    assert!(
        index
            .dirs
            .iter()
            .all(|d| d.path.starts_with("/logical/user-home/"))
    );
    assert_eq!(
        index.validate("~/nested/notes").unwrap(),
        "/logical/user-home/nested/notes"
    );
    assert!(index.validate(tree.0.to_str().unwrap()).is_err());
}

#[test]
fn directory_entry_and_depth_caps_remain_explicit() {
    let tree = Tree::new();
    tree.dir("a/b/c");
    let mut index = HomeIndex::new(tree.0.clone(), tree.0.clone()).unwrap();
    index.limits.max_entries = 1;
    for _ in 0..100 {
        index.step(1);
    }
    assert_eq!(index.dirs.len(), 1);
    assert!(index.status().contains("limit"));
    let mut index = HomeIndex::new(tree.0.clone(), tree.0.clone()).unwrap();
    index.limits.max_depth = 1;
    for _ in 0..100 {
        index.step(1);
    }
    assert_eq!(index.dirs.len(), 1);
    assert!(index.status().contains("limit"));
}

#[test]
fn hidden_home_spelling_does_not_hide_normal_descendants() {
    use zellij_launchpad_prototype::app::{Action, App};
    let tree = Tree::new();
    tree.dir(".home/notes");
    tree.dir(".home/.secret/notes");
    let home = tree.0.join(".home");
    let mut app = App::from_home(home.clone(), home);
    for _ in 0..100 {
        app.index_tick();
    }
    app.update(Action::Clear);
    app.update(Action::Text("notes".into()));
    assert_eq!(app.suggestions.len(), 1);
    app.update(Action::Tab);
    assert_eq!(app.editor.text, "~/notes");
}

#[test]
fn search_status_uses_readable_text_color_not_separator_color() {
    use ratatui::{Terminal, backend::TestBackend};
    use zellij_launchpad_prototype::{app::App, theme::MUTED, view};
    let app = App::default();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| view::render(f, &app)).unwrap();
    assert_eq!(terminal.backend().buffer()[(2, 10)].fg, MUTED);
}

#[test]
fn indexes_nested_directories_incrementally_not_files() {
    let tree = Tree::new();
    tree.dir("Projects/research/notes");
    tree.dir("team notes/修理");
    fs::write(tree.0.join("not-a-directory"), "text").unwrap();
    let mut index = HomeIndex::new(tree.0.clone(), tree.0.clone()).unwrap();
    assert!(index.is_scanning());
    assert!(
        index.dirs.is_empty(),
        "constructing an index does not walk home"
    );
    index.step(1);
    assert!(
        index.is_scanning(),
        "one work unit cannot walk a nested tree"
    );
    for _ in 0..100 {
        index.step(1);
    }
    assert!(!index.is_scanning());
    let paths: Vec<_> = index.dirs.iter().map(|d| d.path.as_str()).collect();
    assert!(paths.iter().any(|p| p.ends_with("Projects/research/notes")));
    assert!(paths.iter().any(|p| p.ends_with("team notes/修理")));
    assert!(!paths.iter().any(|p| p.ends_with("not-a-directory")));
}
