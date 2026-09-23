use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use zellij_launchpad_core::search::HomeIndex;
static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn custom_ignores_prune_trees_before_budgets_and_survive_restart() {
    let tree = Tree::new();
    tree.dir("cache-old");
    tree.dir("archive/deep");
    for i in 0..100 {
        tree.dir(&format!("cache/entry-{i}/deep"));
    }
    fs::write(tree.0.join(".ignore"), "!cache/\n!archive/\n").unwrap();
    let mut index = HomeIndex::new_with_ignore(
        "/logical/home".into(),
        tree.0.clone(),
        [
            "/logical/home/cache/",
            "/logical/home/cache",
            "/logical/home/cache/entry-0",
            "/logical/home/tmp/../archive/./",
            "/logical/home/missing",
            "/elsewhere",
        ]
        .map(str::to_owned)
        .into(),
    )
    .unwrap();
    for _ in 0..2 {
        index.limits.max_entries = 2;
        index.limits.max_bytes = 200;
        while index.is_scanning() {
            index.step(128);
        }
        assert_eq!(index.status(), "HOME indexed · 1 dirs · F5 refresh");
        assert_eq!(index.dirs[0].path, "/logical/home/cache-old");
        assert_eq!(
            index.validate("~/cache/entry-0/deep").unwrap(),
            "/logical/home/cache/entry-0/deep"
        );
        index = index.restart();
    }
}

#[test]
fn custom_ignores_handle_home_ancestors_and_empty_lists() {
    let tree = Tree::new();
    tree.dir("visible/nested");
    for ignore in ["/logical/home", "/logical", "/"] {
        let mut index =
            HomeIndex::new_with_ignore("/logical/home".into(), tree.0.clone(), vec![ignore.into()])
                .unwrap();
        index.step(1);
        assert!(!index.is_scanning());
        assert_eq!(index.status(), "HOME indexed · 0 dirs · F5 refresh");
        assert!(index.validate("~/visible/nested").is_ok());
    }
    for ignore in [
        vec![],
        vec!["/logical/home/elsewhere".into()],
        vec!["/logical/home-old".into()],
    ] {
        let mut index =
            HomeIndex::new_with_ignore("/logical/home".into(), tree.0.clone(), ignore).unwrap();
        while index.is_scanning() {
            index.step(128);
        }
        assert_eq!(index.dirs.len(), 2);
    }
    assert!(
        HomeIndex::new_with_ignore(
            "/logical/home".into(),
            tree.0.clone(),
            vec!["~/visible".into()]
        )
        .is_err()
    );
}

#[test]
fn custom_ignores_follow_host_path_rules_independently_of_filesystem_root() {
    let tree = Tree::new();
    tree.dir("Cache/deep");
    tree.dir("Cache-old");
    for (home, ignore, count) in [
        ("/logical/home", "/logical/home/cache", 3),
        ("/logical/home", "/logical/home/Cache", 1),
        (r"C:\Users\Ada", "c:/users/ada/cache/", 1),
        (r"\\server\share\Ada", r"\\?\UNC\SERVER\SHARE\ada\cache", 1),
        (r"C:\Users\Ada", r"D:\Users\Ada\Cache", 3),
        (r"C:\Users\Ada", "c:/users/", 0),
        (r"\\server\share\Ada", r"\\SERVER\SHARE\", 0),
    ] {
        let mut index =
            HomeIndex::new_with_ignore(home.into(), tree.0.clone(), vec![ignore.into()]).unwrap();
        while index.is_scanning() {
            index.step(128);
        }
        assert_eq!(index.dirs.len(), count, "{home}: {ignore}");
        if count == 1 {
            assert!(index.dirs[0].path.ends_with("Cache-old"));
        }
    }
}
struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("search-tests");
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
    #[cfg(windows)]
    fn hidden(&self, path: &str, hidden: bool) {
        use std::os::windows::fs::MetadataExt;
        let path = if path.is_empty() {
            self.0.clone()
        } else {
            self.0.join(path)
        };
        let result = std::process::Command::new("attrib.exe")
            .arg(if hidden { "+H" } else { "-H" })
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            fs::metadata(path).unwrap().file_attributes() & 0x2 != 0,
            hidden
        );
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn windows_host_paths_map_to_the_sandbox_and_remain_launchable() {
    use zellij_launchpad_core::{
        app::{Action, App},
        search::matches_in,
    };
    let tree = Tree::new();
    tree.dir("Projects/team notes");
    fs::write(tree.0.join("Projects/file"), "text").unwrap();
    for home in [r"C:\Users\Ada", r"\\server\share\Ada"] {
        let mut index = HomeIndex::new(home.into(), tree.0.clone()).unwrap();
        while index.is_scanning() {
            index.step(128);
        }
        let expected = format!(r"{home}\Projects\team notes");
        for raw in [
            "~/Projects/team notes",
            r"~\Projects\team notes",
            expected.as_str(),
        ] {
            assert_eq!(index.validate(raw).unwrap(), expected);
            let results = matches_in(raw, &index.dirs, home, &[]);
            assert_eq!(index.dirs[results[0]].path, expected);
        }
        for invalid in [
            r"..\outside",
            r"Projects\..\..\outside",
            r"D:\outside",
            r"Projects\file",
            r"Projects\missing",
        ] {
            assert!(index.validate(invalid).is_err(), "{invalid}");
        }
        let mut app = App::from_home(home.into(), tree.0.clone());
        app.update(Action::Clear);
        app.update(Action::Text(r"~\Projects\team notes".into()));
        app.update(Action::Enter);
        assert_eq!(app.take_launch().unwrap().path, expected);
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
    assert!(matches_in(&"a".repeat(101), &dirs, &home, &[]).is_empty());
    assert!(matches_in("~/needle", &dirs, &home, &[]).is_empty());
    assert_eq!(matches_in("needle", &dirs, &home, &[]), vec![0]);
}

#[test]
fn selected_long_path_is_preserved_and_launches_exact_target() {
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
    app.update(Action::Down);
    app.update(Action::Enter);
    assert_eq!(app.editor.text, format!("~/{name}"));
    app.update(Action::Text("ignored".into()));
    assert_eq!(app.editor.text, format!("~/{name}"));
    app.update(Action::Enter);
    let launch = app.take_launch().expect("one launch request");
    assert_eq!(launch.path, tree.0.join(name).to_str().unwrap());
}

#[test]
fn parent_ignore_files_and_git_excludes_apply_to_the_search_root() {
    let tree = Tree::new();
    tree.dir(".git/info");
    for name in [
        "git-ignored",
        "ignore-ignored",
        "excluded",
        "visible",
        "restored",
    ] {
        tree.dir(&format!("home/{name}/nested"));
    }
    fs::write(tree.0.join(".gitignore"), "git-ignored/\nrestored/\n").unwrap();
    fs::write(tree.0.join(".ignore"), "ignore-ignored/\n").unwrap();
    fs::write(tree.0.join(".git/info/exclude"), "excluded/\n").unwrap();
    fs::write(tree.0.join("home/.gitignore"), "!restored/\n").unwrap();
    let mut index = HomeIndex::new("/logical/home".into(), tree.0.join("home")).unwrap();
    while index.is_scanning() {
        index.step(128);
    }
    let mut paths: Vec<_> = index.dirs.iter().map(|d| d.path.as_str()).collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            "/logical/home/restored",
            "/logical/home/restored/nested",
            "/logical/home/visible",
            "/logical/home/visible/nested",
        ]
    );
}

#[test]
fn project_ignore_rules_apply_with_nested_negation() {
    let tree = Tree::new();
    tree.dir("project/.git");
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
#[cfg(windows)]
fn windows_hidden_attributes_prune_subtrees_before_index_limits() {
    let tree = Tree::new();
    for parent in ["AppData", "project/secret"] {
        for i in 0..20 {
            tree.dir(&format!("{parent}/descendant-{i}/nested"));
        }
        tree.hidden(parent, true);
    }
    tree.dir("visible");
    tree.dir("project/visible");
    fs::write(tree.0.join(".ignore"), "!AppData/\n!project/secret/\n").unwrap();
    let mut index = HomeIndex::new("/logical/home".into(), tree.0.clone()).unwrap();
    index.limits.max_entries = 4;
    index.limits.max_bytes = 16 * 1024;
    while index.is_scanning() {
        index.step(128);
    }
    let mut paths: Vec<_> = index.dirs.iter().map(|d| d.path.as_str()).collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            "/logical/home/project",
            "/logical/home/project/visible",
            "/logical/home/visible"
        ]
    );
    assert_eq!(index.status(), "HOME indexed · 3 dirs · F5 refresh");
    assert_eq!(
        index.validate("~/AppData/descendant-0").unwrap(),
        "/logical/home/AppData/descendant-0"
    );
    assert!(index.validate("~/project/secret/descendant-0").is_ok());
}

#[test]
#[cfg(windows)]
fn hidden_windows_home_is_scanned_and_refresh_rechecks_attributes() {
    let tree = Tree::new();
    tree.dir("ordinary/nested");
    tree.hidden("", true);
    let mut index = HomeIndex::new(tree.0.clone(), tree.0.clone()).unwrap();
    while index.is_scanning() {
        index.step(128);
    }
    assert_eq!(index.dirs.len(), 2, "HOME itself must remain traversable");
    tree.hidden("ordinary", true);
    index = index.restart();
    while index.is_scanning() {
        index.step(128);
    }
    assert!(index.dirs.is_empty());
    assert!(index.validate("~/ordinary/nested").is_ok());
    tree.hidden("ordinary", false);
    index = index.restart();
    while index.is_scanning() {
        index.step(128);
    }
    assert_eq!(
        index.dirs.len(),
        2,
        "unhidden directories return after refresh"
    );
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
fn node_modules_is_controlled_by_ignore_files() {
    let tree = Tree::new();
    tree.dir(".git");
    tree.dir("node_modules/package");
    tree.dir("project/node_modules/package");
    let mut index = HomeIndex::new("/logical/home".into(), tree.0.clone()).unwrap();
    while index.is_scanning() {
        index.step(128);
    }
    assert_eq!(index.dirs.len(), 5, "unignored dependencies are indexed");
    fs::write(tree.0.join(".gitignore"), "node_modules/\n").unwrap();
    index = index.restart();
    while index.is_scanning() {
        index.step(128);
    }
    assert_eq!(index.dirs.len(), 1);
    assert_eq!(index.dirs[0].path, "/logical/home/project");
    fs::write(tree.0.join("project/.gitignore"), "!node_modules/\n").unwrap();
    index = index.restart();
    while index.is_scanning() {
        index.step(128);
    }
    let mut paths: Vec<_> = index.dirs.iter().map(|d| d.path.as_str()).collect();
    paths.sort();
    assert_eq!(
        paths,
        [
            "/logical/home/project",
            "/logical/home/project/node_modules",
            "/logical/home/project/node_modules/package",
        ]
    );
    assert!(index.validate("~/node_modules/package").is_ok());
}

#[test]
#[cfg(unix)]
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
    // A slice may yield on its time budget before visiting every entry.
    for _ in 0..1000 {
        if !index.is_scanning() {
            break;
        }
        index.step(100);
    }
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
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    #[cfg(unix)]
    let outside = Tree::new();
    tree.dir("team notes/修理");
    tree.dir("it's literal; $HOME");
    fs::write(tree.0.join("file"), "text").unwrap();
    #[cfg(unix)]
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
    // A slice may yield on its time budget before reaching the directory cap.
    for _ in 0..1000 {
        if !index.is_scanning() {
            break;
        }
        index.step(100);
    }
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
    // A short logical HOME keeps `~` queries under the input limit wherever
    // the checkout lives; the tree path alone can expand past it.
    let mut app = App::from_home("/home/ada".into(), tree.0.clone());
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
            .any(|&i| PathBuf::from(&app.dirs[i].path).ends_with("research/notes"))
    );
    assert!(
        !app.suggestions
            .iter()
            .any(|&i| app.dirs[i].path.contains(".archive"))
    );
    query(&mut app, "~/Projects/research");
    assert!(!app.suggestions.is_empty(), "explicit HOME queries match");
    query(&mut app, "~/Projects/.archive");
    assert!(app.suggestions.is_empty());
    app.update(Action::Enter);
    assert!(
        app.take_launch().is_some(),
        "an explicit hidden literal launches"
    );
    app.launch_rejected();
    query(&mut app, "修理");
    app.update(Action::Down);
    app.update(Action::Enter);
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
fn background_indexing_respects_dismissal_and_section_navigation() {
    use zellij_launchpad_core::app::{Action, App, Focus};
    let tree = Tree::new();
    for i in 0..300 {
        tree.dir(&format!("notes-{i}"));
    }
    // The default `~` query expands HOME; keep it under the input limit.
    let mut app = App::from_home("/home/ada".into(), tree.0.clone());
    // Recording in memory keeps one recent row for section-navigation coverage.
    app.simulate_launch = true;
    // Initialization may use a whole time slice before producing suggestions.
    for _ in 0..1000 {
        app.index_tick();
        if !app.suggestions.is_empty() {
            break;
        }
    }
    assert!(!app.suggestions.is_empty());
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
    assert_eq!(app.focus, Focus::Path);
    app.index_tick();
    assert!(
        app.suggestions.is_empty(),
        "a recent selection keeps suggestions closed until the path is edited"
    );
}

#[test]
fn restricted_state_is_visible_at_minimum_size() {
    use ratatui::{Terminal, backend::TestBackend};
    use zellij_launchpad_core::{app::App, view};
    let mut app = App::default();
    app.remote_failed("HOME access denied. Reopen Launchpad.".into());
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
    app.update(Action::Down);
    app.update(Action::Enter);
    assert_eq!(app.editor.text, "~/notes");
}

#[test]
fn search_diagnostics_are_readable_in_help() {
    use ratatui::{Terminal, backend::TestBackend};
    use zellij_launchpad_core::{
        app::{Action, App},
        theme::Theme,
        view,
    };
    let mut app = App::default();
    app.search_status = "HOME indexed".into();
    app.update(Action::Help);
    app.update(Action::End);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| view::render(f, &app)).unwrap();
    let b = terminal.backend().buffer();
    let y = (0..24)
        .find(|&y| {
            (0..80)
                .map(|x| b[(x, y)].symbol())
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                == ["Search", "HOME", "indexed"]
        })
        .unwrap();
    assert_eq!(b[(2, y)].fg, Theme::default().text);
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
    assert!(
        paths
            .iter()
            .any(|p| PathBuf::from(p).ends_with("Projects/research/notes"))
    );
    assert!(
        paths
            .iter()
            .any(|p| PathBuf::from(p).ends_with("team notes/修理"))
    );
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

fn ranked(raw: &str, paths: &[String], home: &str, recent: &[String]) -> Vec<String> {
    use zellij_launchpad_core::search::{Directory, matches_in};
    let dirs: Vec<_> = paths
        .iter()
        .map(|path| Directory {
            path: path.clone(),
            note: "directory",
            error: None,
        })
        .collect();
    matches_in(raw, &dirs, home, recent)
        .into_iter()
        .map(|i| paths[i].clone())
        .collect()
}

#[test]
fn equal_scores_prefer_shallower_directories_over_path_order() {
    // Frizbee's local alignment ignores trailing text, so `proj/zel` scores a
    // project and every subdirectory of a sibling project identically.
    let tree = Tree::new();
    for sub in [
        "crates/core/src",
        "crates/cli/src",
        "docs/adr",
        "assets",
        "target/debug",
    ] {
        tree.dir(&format!("Projects/zellij-agent-wrangler/{sub}"));
    }
    tree.dir("Projects/zellij-launchpad");
    let mut index = HomeIndex::new("/home/ada".into(), tree.0.clone()).unwrap();
    while index.is_scanning() {
        index.step(128);
    }
    let paths: Vec<_> = index.dirs.iter().map(|d| d.path.clone()).collect();
    for raw in ["proj/zel", "~/Projects/zel", "proj/zell"] {
        let results = ranked(raw, &paths, "/home/ada", &[]);
        let launchpad = results
            .iter()
            .position(|p| p == "/home/ada/Projects/zellij-launchpad")
            .unwrap();
        assert!(launchpad <= 1, "{raw}: {results:?}");
        for (rank, path) in results.iter().enumerate() {
            if path.starts_with("/home/ada/Projects/zellij-agent-wrangler/") {
                assert!(rank > launchpad, "{raw}: {results:?}");
            }
        }
    }
    // Windows HOME paths count components the same way.
    let home = r"C:\Users\Ada";
    let paths: Vec<_> = [
        r"C:\Users\Ada\Projects\zellij-agent-wrangler\crates\core",
        r"C:\Users\Ada\Projects\zellij-agent-wrangler",
        r"C:\Users\Ada\Projects\zellij-launchpad",
    ]
    .map(str::to_owned)
    .into();
    let results = ranked("proj/zel", &paths, home, &[]);
    assert_eq!(results[2], paths[0], "{results:?}");
}

#[test]
fn equal_scores_prefer_recent_launches_then_depth() {
    let home = "/home/ada";
    let paths: Vec<_> = [
        "/home/ada/Projects/zellij-agent-wrangler/crates",
        "/home/ada/Projects/zellij-agent-wrangler",
        "/home/ada/Projects/zellij-launchpad",
    ]
    .map(str::to_owned)
    .into();
    let wrangler = paths[1].clone();
    let launchpad = paths[2].clone();
    // Without history: depth, then path order.
    assert_eq!(
        ranked("proj/zel", &paths, home, &[])[..2],
        [wrangler.clone(), launchpad.clone()]
    );
    // History spellings normalize to the index's canonical form.
    for spelling in [
        "/home/ada/Projects/zellij-launchpad",
        "/home/ada/Projects/zellij-launchpad/",
        "~/Projects/zellij-launchpad",
        "/home/ada/Projects/./zellij-launchpad",
    ] {
        let results = ranked("proj/zel", &paths, home, &[spelling.into()]);
        assert_eq!(results[0], launchpad, "{spelling}");
        assert_eq!(results[1], wrangler, "{spelling}");
    }
    // More recent (earlier in history) wins; history never beats a higher score
    // or a directory named by the last segment (`crates` does not contain `zel`).
    let recent = [launchpad.clone(), wrangler.clone()];
    assert_eq!(
        ranked("proj/zel", &paths, home, &recent),
        [launchpad.clone(), wrangler.clone(), paths[0].clone()]
    );
    let recent = [wrangler.clone(), launchpad.clone()];
    assert_eq!(
        ranked("proj/zel", &paths, home, &recent),
        [wrangler.clone(), launchpad.clone(), paths[0].clone()]
    );
    assert_eq!(
        ranked("proj/zel", &paths, home, &[paths[0].clone()]),
        [wrangler.clone(), launchpad.clone(), paths[0].clone()]
    );
    assert_eq!(ranked("launchpad", &paths, home, &[wrangler])[0], launchpad);
    // Windows history compares case-insensitively and accepts either separator.
    let home = r"C:\Users\Ada";
    let paths: Vec<_> = [
        r"C:\Users\Ada\Projects\zellij-agent-wrangler",
        r"C:\Users\Ada\Projects\zellij-launchpad",
    ]
    .map(str::to_owned)
    .into();
    for spelling in [
        r"c:\users\ada\projects\ZELLIJ-LAUNCHPAD\",
        "C:/Users/Ada/Projects/zellij-launchpad",
        r"~\Projects\zellij-launchpad",
    ] {
        assert_eq!(
            ranked("proj/zel", &paths, home, &[spelling.into()])[0],
            paths[1],
            "{spelling}"
        );
    }
}

#[test]
fn launched_directories_win_suggestion_ties_locally_and_remotely() {
    use zellij_launchpad_core::{
        app::{Action, App, Launch, Tool},
        remote::RemoteRequest,
    };
    let tree = Tree::new();
    tree.dir("Projects/zellij-agent-wrangler");
    tree.dir("Projects/zellij-launchpad");
    let launchpad = tree.0.join("Projects").join("zellij-launchpad");
    let launchpad = launchpad.to_str().unwrap().to_owned();
    let history = vec![Launch {
        id: 1,
        path: launchpad.clone(),
        tool: Tool::Shell,
        age: "Just now".into(),
    }];
    let mut app = App::from_home(tree.0.clone(), tree.0.clone());
    while app.is_indexing() {
        app.index_tick();
    }
    app.update(Action::Clear);
    app.update(Action::Text("proj/zel".into()));
    assert_ne!(app.dirs[app.suggestions[0]].path, launchpad);
    app.replace_history(history.clone());
    app.update(Action::Clear);
    app.update(Action::Text("proj/zel".into()));
    assert_eq!(app.dirs[app.suggestions[0]].path, launchpad);
    // The native worker ranks with the same history.
    let mut app = App::from_remote("/home/ada".into());
    app.replace_history(history);
    let Some(RemoteRequest::Query { recent, .. }) = app.take_remote_request() else {
        panic!()
    };
    assert_eq!(recent, [launchpad]);
}

#[test]
fn last_segment_names_the_checkout_over_launched_folders_inside_it() {
    // Projects/<repo>/<branch> worktrees: a launched subfolder ties with its
    // checkout on score, but only the checkout is named by `main` or `105`.
    let home = "/home/ada";
    let paths: Vec<_> = [
        "/home/ada/Projects/zellij-launchpad/fix-history",
        "/home/ada/Projects/zellij-launchpad/fix-history/src",
        "/home/ada/Projects/zellij-launchpad/main",
        "/home/ada/Projects/zellij-launchpad/main/docs",
        "/home/ada/Projects/zellij-launchpad/main/native",
        "/home/ada/Projects/ov-tracker/issue-105",
        "/home/ada/Projects/ov-tracker/issue-105/src/web",
    ]
    .map(str::to_owned)
    .into();
    let recent = [paths[3].clone(), paths[1].clone(), paths[6].clone()];
    for (raw, checkout, launched) in [
        ("launchpad/main", &paths[2], &paths[3]),
        ("proj/launchpad/main", &paths[2], &paths[3]),
        ("launchpad/fix", &paths[0], &paths[1]),
        ("tracker/105", &paths[5], &paths[6]),
    ] {
        let results = ranked(raw, &paths, home, &recent);
        assert_eq!(results[..2], [checkout.clone(), launched.clone()], "{raw}");
    }
    // Substring, not fuzzy: `tie` is scattered through `native` but only
    // contained in the checkout's name.
    let paths: Vec<_> = [
        "/home/ada/Projects/zellij-launchpad/feat-search-tiebreaks",
        "/home/ada/Projects/zellij-launchpad/feat-search-tiebreaks/native",
    ]
    .map(str::to_owned)
    .into();
    assert_eq!(
        ranked("launchpad/tie", &paths, home, &[paths[1].clone()]),
        paths
    );
}

#[test]
fn home_queries_name_no_folder_for_the_last_segment_tie_break() {
    // `~`, `./` and `..` expand to HOME, whose own name (`ada`) must not
    // promote `canada` or `ada-lib` over recent launches.
    let home = "/home/ada";
    let paths: Vec<_> = [
        "/home/ada/canada",
        "/home/ada/Projects",
        "/home/ada/Projects/ada-lib",
        "/home/ada/Projects/zellij-launchpad",
        "/home/ada/work",
    ]
    .map(str::to_owned)
    .into();
    let recent = ["~/Projects/zellij-launchpad".into(), "~/work".into()];
    for raw in ["~", "~/", "./", "~/Projects/.."] {
        let results = ranked(raw, &paths, home, &recent);
        assert_eq!(results[..2], [paths[3].clone(), paths[4].clone()], "{raw}");
    }
    // A trailing `/` means no segment rule: `Projects-archive` ties with the
    // launched checkout but gets no boost for containing `Projects`.
    let paths: Vec<_> = [
        "/home/ada/Projects/Projects-archive",
        "/home/ada/Projects/zellij-launchpad",
    ]
    .map(str::to_owned)
    .into();
    let recent = ["~/Projects/zellij-launchpad".into()];
    assert_eq!(ranked("~/Projects/", &paths, home, &recent)[0], paths[1]);
    assert_eq!(ranked("~/Projects/", &paths, home, &[])[0], paths[0]);
}
