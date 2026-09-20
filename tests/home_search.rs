use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use zellij_launchpad_core::search::HomeIndex;
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/search-tests");
        fs::create_dir_all(&base).unwrap();
        let path = base.join(format!(
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
fn fuzzy_matching_rejects_oversized_raw_and_expanded_queries() {
    use zellij_launchpad_core::search::{Directory, matches_in};
    let home = format!("/home/{}", "a".repeat(101));
    let dirs = [Directory {
        path: format!("{home}/needle"),
        note: "directory",
        error: None,
    }];
    assert!(matches_in(&"a".repeat(101), &dirs, &home).is_empty());
    assert!(matches_in("~/needle", &dirs, &home).is_empty());
    assert_eq!(matches_in("needle", &dirs, &home), vec![0]);
}

#[test]
fn completed_long_path_is_preserved_and_launches_exact_target() {
    use zellij_launchpad_core::app::{Action, App};
    let tree = Tree::new();
    let name = format!("{}-needle", "a".repeat(110));
    tree.dir(&name);
    let mut app = App::from_home(tree.0.clone(), tree.0.clone());
    while app.is_indexing() {
        app.index_tick();
    }
    app.update(Action::Clear);
    app.update(Action::Text("needle".into()));
    app.update(Action::Tab);
    assert_eq!(app.editor.text, format!("~/{name}"));
    app.update(Action::Text("ignored".into()));
    assert_eq!(app.editor.text, format!("~/{name}"));
    app.update(Action::Enter);
    let launch = app.take_launch().expect("one launch request");
    assert_eq!(launch.path, tree.0.join(name).to_str().unwrap());
}

#[test]
fn ignore_rule_bytes_share_the_index_memory_limit() {
    let tree = Tree::new();
    tree.dir("visible");
    fs::write(tree.0.join(".ignore"), "nonmatching-pattern\n".repeat(1000)).unwrap();
    let mut index = HomeIndex::new("/logical/home".into(), tree.0.clone()).unwrap();
    index.limits.max_bytes = 1024;
    while index.is_scanning() {
        index.step(128);
    }
    assert!(index.status().contains("limit"));
    assert!(index.dirs.is_empty());
    assert!(index.validate("~/visible").is_ok());
}

#[test]
fn symlinked_ignore_files_do_not_read_or_apply_outside_rules() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    tree.dir("home/visible");
    fs::write(tree.0.join("outside-rules"), "visible/\n").unwrap();
    symlink(tree.0.join("outside-rules"), tree.0.join("home/.ignore")).unwrap();
    let mut index = HomeIndex::new("/logical/home".into(), tree.0.join("home")).unwrap();
    while index.is_scanning() {
        index.step(128);
    }
    assert_eq!(index.dirs.len(), 1, "symlinked rules must not apply");
}

#[cfg(target_os = "linux")]
#[test]
fn ignore_discovery_does_not_open_parent_configs_or_pruned_descendants() {
    use std::{ffi::CString, io::Read, os::fd::FromRawFd};
    unsafe extern "C" {
        fn inotify_init1(flags: i32) -> i32;
        fn inotify_add_watch(fd: i32, path: *const std::ffi::c_char, mask: u32) -> i32;
    }
    let tree = Tree::new();
    tree.dir("home/visible");
    for name in ["blocked", ".secret", ".git", "node_modules"] {
        tree.dir(&format!("home/{name}/nested"));
        fs::write(tree.0.join(format!("home/{name}/.ignore")), "*\n").unwrap();
    }
    fs::write(
        tree.0.join("home/.ignore"),
        "blocked/\n!blocked/nested/\n!.secret/\n!.git/\n!node_modules/\n",
    )
    .unwrap();
    fs::write(tree.0.join(".ignore"), "visible/\n").unwrap();
    fs::write(tree.0.join(".gitignore"), "visible/\n").unwrap();
    fs::write(tree.0.join("external-rules"), "visible/\n").unwrap();
    std::os::unix::fs::symlink(
        tree.0.join("external-rules"),
        tree.0.join("home/.gitignore"),
    )
    .unwrap();
    let fd = unsafe { inotify_init1(0x800) }; // IN_NONBLOCK
    assert!(fd >= 0);
    let mut watch = unsafe { fs::File::from_raw_fd(fd) };
    for relative in [
        ".ignore",
        ".gitignore",
        "external-rules",
        "home/blocked/.ignore",
        "home/blocked/nested",
        "home/.secret/.ignore",
        "home/.secret/nested",
        "home/.git/.ignore",
        "home/.git/nested",
        "home/node_modules/.ignore",
        "home/node_modules/nested",
    ] {
        let path = CString::new(tree.0.join(relative).as_os_str().as_encoded_bytes()).unwrap();
        assert!(unsafe { inotify_add_watch(fd, path.as_ptr(), 0x20) } >= 0); // IN_OPEN
    }
    let mut index = HomeIndex::new("/logical/home".into(), tree.0.join("home")).unwrap();
    while index.is_scanning() {
        index.step(128);
    }
    assert_eq!(index.dirs.len(), 1, "parent rules cannot change results");
    let mut events = [0; 4096];
    let result = watch.read(&mut events);
    assert!(
        matches!(result, Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "outside config or pruned descendant was opened: {result:?}"
    );
}

#[test]
fn project_ignore_rules_apply_without_git_metadata_with_nested_negation() {
    let tree = Tree::new();
    for name in [
        "project/build/cache",
        "project/release/keep",
        "project/release/drop",
        "project/src/generated",
        "project/src/handwritten",
        "project/plain",
    ] {
        tree.dir(name);
    }
    fs::write(
        tree.0.join("project/.gitignore"),
        "build/\nrelease/*\n!release/keep/\nsrc/*\nplain/\n",
    )
    .unwrap();
    fs::write(tree.0.join("project/.ignore"), "!plain/\n**/generated/\n").unwrap();
    fs::write(
        tree.0.join("project/src/.gitignore"),
        "!handwritten/\n!generated/\n",
    )
    .unwrap();
    let mut index = HomeIndex::new("/logical/home".into(), tree.0.clone()).unwrap();
    index.limits.max_entries = 7;
    while index.is_scanning() {
        index.step(128);
    }
    let mut paths: Vec<_> = index.dirs.iter().map(|d| d.path.as_str()).collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            "/logical/home/project",
            "/logical/home/project/plain",
            "/logical/home/project/release",
            "/logical/home/project/release/keep",
            "/logical/home/project/src",
            "/logical/home/project/src/handwritten"
        ]
    );
    assert_eq!(index.status(), "HOME indexed · 6 dirs · F5 refresh");
    assert!(index.validate("~/project/build/cache").is_ok());
}

#[test]
fn hidden_subtrees_are_pruned_before_entry_and_memory_budgets() {
    let tree = Tree::new();
    for i in 0..100 {
        tree.dir(&format!(".cache/hidden-{i}/nested"));
    }
    tree.dir("visible");
    let mut index = HomeIndex::new("/logical/home".into(), tree.0.clone()).unwrap();
    index.limits.max_entries = 2;
    index.limits.max_bytes = 200;
    while index.is_scanning() {
        index.step(128);
    }
    assert_eq!(index.status(), "HOME indexed · 1 dirs · F5 refresh");
    assert_eq!(index.dirs[0].path, "/logical/home/visible");
    assert_eq!(
        index.validate("~/.cache/hidden-0/nested").unwrap(),
        "/logical/home/.cache/hidden-0/nested"
    );
}

#[test]
fn prunes_git_and_node_modules_subtrees_at_every_depth() {
    let tree = Tree::new();
    for prefix in ["", "project/"] {
        for name in [".git", "node_modules"] {
            for i in 0..100 {
                tree.dir(&format!("{prefix}{name}/ignored-{i}/nested"));
            }
        }
    }
    for name in [
        "project/src",
        ".github/workflows",
        "node_modules_backup/keep",
        "Node_modules/keep",
    ] {
        tree.dir(name);
    }
    let mut index = HomeIndex::new(tree.0.clone(), tree.0.clone()).unwrap();
    // Excluded trees must not consume the entry budget, not merely be hidden.
    index.limits.max_entries = 7; // Six retained entries; ignored names cost nothing.
    for _ in 0..1000 {
        index.step(128);
        if !index.is_scanning() {
            break;
        }
    }
    assert!(!index.is_scanning());
    assert!(
        index.status().starts_with("HOME indexed"),
        "{}",
        index.status()
    );
    let mut paths: Vec<_> = index
        .dirs
        .iter()
        .map(|d| {
            PathBuf::from(&d.path)
                .strip_prefix(&tree.0)
                .unwrap()
                .to_owned()
        })
        .collect();
    paths.sort();
    let mut expected: Vec<_> = [
        "project",
        "project/src",
        "node_modules_backup",
        "node_modules_backup/keep",
        "Node_modules",
        "Node_modules/keep",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    expected.sort();
    assert_eq!(paths, expected);
    assert_eq!(index.status(), "HOME indexed · 6 dirs · F5 refresh");
    // Index exclusions are not a restriction on explicitly selected paths.
    assert!(index.validate("~/project/node_modules/ignored-0").is_ok());
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
    use zellij_launchpad_core::app::{Action, App};
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
    assert!(app.suggestions.is_empty());
    app.update(Action::Enter);
    assert!(
        app.take_launch().is_some(),
        "an explicit hidden literal launches"
    );
    app.launch_rejected();
    query(&mut app, "修理");
    app.update(Action::Tab);
    assert_eq!(app.editor.text, "~/team notes/修理");
    assert!(app.take_launch().is_none());
    fs::remove_dir(tree.0.join("team notes/修理")).unwrap();
    app.update(Action::Enter);
    assert!(app.take_launch().is_none());
    assert!(app.message.as_ref().unwrap().contains("unavailable"));
    assert!(app.history.is_empty());
    query(&mut app, "~/Projects/research/notes");
    app.update(Action::Enter);
    assert!(app.take_launch().is_some());
    assert!(app.history.is_empty(), "the shared store owns real history");
    app.launch_rejected();
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
    use zellij_launchpad_core::app::{Action, App, Focus};
    let tree = Tree::new();
    for i in 0..300 {
        tree.dir(&format!("notes-{i}"));
    }
    let mut app = App::from_home(tree.0.clone(), tree.0.clone());
    // Recording in memory keeps one recent row to copy back into the form.
    app.simulate_launch = true;
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
    assert_eq!(app.history.len(), 1);
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
    use zellij_launchpad_core::{app::App, view};
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
    use zellij_launchpad_core::app::{Action, App};
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
    use zellij_launchpad_core::{app::App, theme::Theme, view};
    let app = App::default();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| view::render(f, &app)).unwrap();
    assert_eq!(
        terminal.backend().buffer()[(2, 10)].fg,
        Theme::default().muted
    );
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
#[test]
fn byte_budget_stops_before_retaining_long_paths() {
    let tree = Tree::new();
    tree.dir("a-long-directory-name");
    let mut index = HomeIndex::new(tree.0.clone(), tree.0.clone()).unwrap();
    index.limits.max_bytes = 1;
    for _ in 0..100 {
        index.step(100);
    }
    assert!(!index.is_scanning());
    assert!(index.dirs.is_empty());
    assert!(index.status().contains("limit"));
}
