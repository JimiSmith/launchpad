use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};
use zellij_launchpad::{config::home_from_env, zellij::tab_name};

#[test]
fn home_discovery_keeps_the_windows_fallbacks() {
    let mut env = BTreeMap::from([
        ("USERPROFILE", OsString::from(r"C:\Users\Ada")),
        ("HOMEDRIVE", "D:".into()),
        ("HOMEPATH", r"\Ada".into()),
    ]);
    let home = |env: &BTreeMap<&str, OsString>| home_from_env(|key| env.get(key).cloned());
    assert_eq!(home(&env), Some(PathBuf::from(r"C:\Users\Ada")));
    env.insert("HOME", "C:/custom/home".into());
    assert_eq!(home(&env), Some(PathBuf::from("C:/custom/home")));
    env.insert("HOME", "".into());
    env.remove("USERPROFILE");
    assert_eq!(home(&env), Some(PathBuf::from(r"D:\Ada")));
    env.remove("HOMEDRIVE");
    assert!(home(&env).is_none());
}

#[test]
fn windows_tab_names_use_the_basename() {
    assert_eq!(tab_name(r"C:\Users\Ada\notes", "Shell"), "notes · Shell");
    assert_eq!(tab_name(r"\\server\share\notes", "Tool"), "notes · Tool");
}

#[test]
fn windows_cwd_label_keeps_the_exact_invoking_directory_exception() {
    use zellij_launchpad::worker::validate;
    use zellij_launchpad_core::{host_path::label, search::HomeIndex};
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/native-windows-cwd");
    std::fs::create_dir_all(root.join("nested/notes")).unwrap();
    let root = root.canonicalize().unwrap();
    let cwd = root.join("nested/notes");
    let index = HomeIndex::new(root.clone(), root.clone()).unwrap();
    let cwd = cwd.to_str().unwrap();
    let relative = label(cwd, root.to_str().unwrap()).unwrap();
    assert_eq!(relative, "~/nested/notes");
    assert_eq!(validate(&index, cwd, &relative).unwrap(), cwd);
}
