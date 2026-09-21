//! Host paths are data: WASI's `std::path` always uses Unix rules, even when
//! Zellij runs on Windows. Only paths inside the sandbox use `std::path`.
#[derive(Debug, Clone)]
pub struct HostPath {
    root: String,
    parts: Vec<String>,
    windows: bool,
}

fn drive(path: &str) -> bool {
    path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && path.as_bytes().get(1) == Some(&b':')
}

fn components(path: &str, windows: bool) -> Option<Vec<String>> {
    let parts: Vec<String> = path
        .split(|c| c == '/' || (windows && c == '\\'))
        .filter(|p| !p.is_empty() && *p != ".")
        .map(str::to_owned)
        .collect();
    if windows
        && parts.iter().any(|p| {
            p.contains([':', '<', '>', '"', '|', '?', '*'])
                || (p != ".." && p.ends_with(['.', ' ']))
        })
    {
        return None;
    }
    Some(parts)
}

impl HostPath {
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.is_empty() || raw.len() > 4096 || raw.chars().any(char::is_control) {
            return None;
        }
        let slashes = raw.replace('\\', "/");
        let windows = drive(raw) || raw.starts_with("\\\\") || raw.starts_with("//");
        let path = if windows { slashes.as_str() } else { raw };
        let extended_unc;
        let path = if let Some(rest) = path.strip_prefix("//?/UNC/") {
            extended_unc = format!("//{rest}");
            extended_unc.as_str()
        } else {
            path.strip_prefix("//?/").unwrap_or(path)
        };
        let (root, rest) = if windows && drive(path) && path.as_bytes().get(2) == Some(&b'/') {
            (path[..3].to_owned(), &path[3..])
        } else if windows {
            let mut parts = path.strip_prefix("//")?.splitn(3, '/');
            let server = parts.next()?;
            let share = parts.next()?;
            if [server, share]
                .iter()
                .any(|p| p.is_empty() || *p == "." || *p == ".." || components(p, true).is_none())
            {
                return None;
            }
            (format!("//{server}/{share}/"), parts.next().unwrap_or(""))
        } else {
            ("/".into(), path.strip_prefix('/')?)
        };
        Some(Self {
            root,
            parts: components(rest, windows)?,
            windows,
        })
    }

    pub fn is_home(&self) -> bool {
        !self.parts.is_empty() && !self.has_parent()
    }

    pub fn has_parent(&self) -> bool {
        self.parts.iter().any(|p| p == "..")
    }

    pub fn is_windows(&self) -> bool {
        self.windows
    }

    /// Preserve '..' until the sandbox validator has checked each preceding
    /// component; normalizing first would hide symlink traversal.
    pub fn relative(&self, raw: &str) -> Option<Vec<String>> {
        if raw.is_empty() || raw.len() > 4096 || raw.chars().any(char::is_control) {
            return None;
        }
        if raw == "~" {
            return Some(Vec::new());
        }
        if let Some(rest) = raw
            .strip_prefix("~/")
            .or_else(|| self.windows.then(|| raw.strip_prefix("~\\")).flatten())
        {
            return components(rest, self.windows);
        }
        if raw.starts_with('~') {
            return None;
        }
        if let Some(path) = Self::parse(raw) {
            return self.strip(&path);
        }
        // A drive-relative or root-relative Windows path must never be joined
        // onto HOME (nor may an absolute path of another flavour be joined).
        if raw.starts_with('/') || (self.windows && (raw.starts_with('\\') || drive(raw))) {
            return None;
        }
        components(raw, self.windows)
    }

    fn strip(&self, path: &Self) -> Option<Vec<String>> {
        let equal = |a: &str, b: &str| {
            if self.windows {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        };
        if self.windows != path.windows
            || !equal(&self.root, &path.root)
            || path.parts.len() < self.parts.len()
            || !self.parts.iter().zip(&path.parts).all(|(a, b)| equal(a, b))
        {
            return None;
        }
        Some(path.parts[self.parts.len()..].to_vec())
    }

    pub fn join(&self, relative: &[String]) -> String {
        let mut parts = self.parts.clone();
        parts.extend_from_slice(relative);
        let path = format!("{}{}", self.root, parts.join("/"));
        if self.windows {
            path.replace('/', "\\")
        } else {
            path
        }
    }
}

pub fn label(path: &str, home: &str) -> Option<String> {
    let rest = HostPath::parse(home)?.strip(&HostPath::parse(path)?)?;
    Some(if rest.is_empty() {
        "~".into()
    } else {
        format!("~/{}", rest.join("/"))
    })
}

/// Compare host acknowledgements without depending on the build OS's path
/// separator rules. Do not resolve '..' or symlinks into a different identity.
pub fn same(a: &str, b: &str) -> bool {
    match (HostPath::parse(a), HostPath::parse(b)) {
        (Some(a), Some(b)) => a.strip(&b).is_some_and(|rest| rest.is_empty()),
        _ => false,
    }
}

pub fn basename(path: &str) -> String {
    HostPath::parse(path)
        .map(|p| p.parts.last().cloned().unwrap_or_else(|| p.join(&[])))
        .unwrap_or_else(|| "/".into())
}

pub fn normalize(raw: &str, home: &str) -> Option<String> {
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return None;
    }
    let home = HostPath::parse(home)?;
    let mut path = if raw == "~" {
        home.clone()
    } else if let Some(rest) = raw
        .strip_prefix("~/")
        .or_else(|| home.windows.then(|| raw.strip_prefix("~\\")).flatten())
    {
        let mut path = home.clone();
        path.parts.extend(components(rest, home.windows)?);
        path
    } else if let Some(path) = HostPath::parse(raw) {
        path
    } else {
        let relative = home.relative(raw)?;
        let mut path = home.clone();
        path.parts.extend(relative);
        path
    };
    let mut reduced = Vec::new();
    for part in path.parts {
        if part == ".." {
            reduced.pop();
        } else {
            reduced.push(part);
        }
    }
    path.parts = reduced;
    Some(path.join(&[]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_are_host_data_on_every_build_target() {
        for home in [r"C:\Users\Ada", "C:/Users/Ada", r"\\?\C:\Users\Ada"] {
            let path = HostPath::parse(home).unwrap();
            assert!(path.is_home());
            for raw in [
                r"~\Projects\notes",
                "~/Projects/notes",
                r"C:\Users\Ada\Projects\notes",
                "c:/users/ada/Projects/notes",
            ] {
                let rest = path.relative(raw).unwrap();
                assert_eq!(path.join(&rest), r"C:\Users\Ada\Projects\notes");
            }
            for raw in [
                r"C:notes",
                r"\notes",
                r"D:\notes",
                r"C:\Users\Adam\notes",
                r"~/C:\outside",
                r"~/.. \outside",
                r"~/file:stream",
            ] {
                assert!(path.relative(raw).is_none(), "{raw}");
            }
        }
        assert_eq!(
            normalize(r"~\Projects\..\notes", r"C:\Users\Ada").unwrap(),
            r"C:\Users\Ada\notes"
        );
        assert_eq!(
            label(r"C:\Users\Ada\notes", "c:/users/ada").unwrap(),
            "~/notes"
        );
        assert_eq!(basename(r"C:\Users\Ada\notes\"), "notes");
        assert_eq!(basename(r"C:\"), r"C:\");
    }

    #[test]
    fn unc_and_unix_paths_keep_their_own_rules() {
        for home in [
            r"\\server\share\Ada",
            r"\\?\UNC\server\share\Ada",
            "//server/share/Ada",
        ] {
            let path = HostPath::parse(home).unwrap();
            assert_eq!(
                path.join(&path.relative("~/notes").unwrap()),
                r"\\server\share\Ada\notes"
            );
            assert!(path.relative(r"\\server\other\Ada\notes").is_none());
        }
        assert_eq!(
            normalize(r"~/literal\name", "/home/ada").unwrap(),
            r"/home/ada/literal\name"
        );
        assert_eq!(basename(r"/home/ada/literal\name"), r"literal\name");
        for invalid in [
            "C:relative",
            r"\\.\C:\Users",
            r"\\server",
            "relative",
            "~/notes",
        ] {
            assert!(HostPath::parse(invalid).is_none(), "{invalid}");
        }
    }
}
