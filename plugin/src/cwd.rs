//! Instance-local identity across the HOME worker's guarded self-reload.
use std::path::Path;

pub fn load(data: &Path, initial: &Path) -> Result<String, String> {
    let file = data.join("original-cwd");
    let cwd = match std::fs::read_to_string(&file) {
        Ok(cwd) => cwd,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let cwd = initial.to_str().ok_or("Invoking directory is not UTF-8")?;
            std::fs::write(&file, cwd).map_err(|e| e.to_string())?;
            cwd.to_owned()
        }
        Err(error) => return Err(error.to_string()),
    };
    // The host serializes paths lossily. Reject U+FFFD, including a legitimate
    // occurrence, rather than risk opening an existing UTF-8 replacement twin.
    if zellij_launchpad_core::host_path::HostPath::parse(&cwd).is_none()
        || cwd.len() > 4096
        || cwd.chars().any(char::is_control)
        || cwd.contains('\u{fffd}')
    {
        return Err("Invoking directory unavailable or unsupported; no fallback.".into());
    }
    Ok(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../target/cwd-tab-naming/unit")
                .join(format!(
                    "{}-{:?}",
                    std::process::id(),
                    std::thread::current().id()
                ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn remounted_home_never_replaces_the_original_long_literal_identity() {
        let f = Fixture::new();
        let cwd = format!("/outside/{} 修理 e\u{301}; $HOME", "x".repeat(180));
        assert_eq!(load(&f.0, Path::new(&cwd)).unwrap(), cwd);
        assert_eq!(load(&f.0, Path::new("/owned/home")).unwrap(), cwd);
    }
    #[test]
    fn windows_invoking_cwd_survives_the_home_reload_literally() {
        let f = Fixture::new();
        let cwd = r"C:\Projects\team notes";
        assert_eq!(load(&f.0, Path::new(cwd)).unwrap(), cwd);
        assert_eq!(load(&f.0, Path::new(r"C:\Users\Ada")).unwrap(), cwd);
    }
    #[test]
    fn host_replacement_char_cannot_turn_non_utf8_into_a_different_path() {
        let f = Fixture::new();
        // 0.45.1 serializes PluginIds.initial_cwd through Path::display().
        // An existing UTF-8 twin must not be launched for a lossy host identity.
        let twin = f.0.join("replacement-\u{fffd}");
        std::fs::create_dir(&twin).unwrap();
        assert!(load(&f.0, &twin).is_err());
    }
}
