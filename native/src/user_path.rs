//! The user's PATH as a new login shell, or on Windows a new process, gets it now.
use std::{ffi::OsString, time::Instant};

/// Where a launched tool gets its PATH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPath {
    /// The PATH of this process.
    Inherited,
    /// The PATH from `current`, else the PATH of this process.
    Current,
}

/// The PATH that `shell` gets as an interactive login shell. `None` if the shell fails or
/// does not finish before `deadline`.
#[cfg(unix)]
pub fn current(shell: &str, deadline: Instant) -> Option<OsString> {
    use std::os::unix::process::CommandExt;
    let mut command = std::process::Command::new(shell);
    // An external command, so that the query is the same in each shell syntax.
    command.args(["-l", "-i", "-c", "printenv PATH"]);
    // The shell gets no controlling terminal, so it cannot read or change the terminal of
    // this process.
    // SAFETY: setsid is async-signal-safe and touches no memory.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        })
    };
    let output = crate::host::run(command, "shell", deadline).ok()?;
    if !output.status.success() {
        return None;
    }
    last_line(&output.stdout).map(OsString::from)
}

/// The machine PATH, then the user PATH, from the registry, with variables expanded.
#[cfg(windows)]
pub fn current(_shell: &str, _deadline: Instant) -> Option<OsString> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    join([
        registry::value(
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
        registry::value(HKEY_CURRENT_USER, "Environment"),
    ])
}

/// The last line of `stdout` that is not empty. Profile scripts can write text before it.
pub fn last_line(stdout: &str) -> Option<&str> {
    stdout
        .lines()
        .map(str::trim_end)
        .rfind(|line| !line.is_empty())
}

/// The parts that are not empty, joined with `;`. `None` if there are none.
pub fn join(parts: [Option<OsString>; 2]) -> Option<OsString> {
    let mut joined = OsString::new();
    for part in parts.into_iter().flatten() {
        let part = part.to_string_lossy();
        let part = part.trim_matches(';');
        if part.is_empty() {
            continue;
        }
        if !joined.is_empty() {
            joined.push(";");
        }
        joined.push(part);
    }
    (!joined.is_empty()).then_some(joined)
}

#[cfg(windows)]
mod registry {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS},
        System::Registry::{HKEY, RRF_RT_REG_SZ, RegGetValueW},
    };

    /// The `Path` value of `key`. A `REG_EXPAND_SZ` value comes back expanded.
    pub fn value(root: HKEY, key: &str) -> Option<OsString> {
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (key, name) = (wide(key), wide("Path"));
        let mut size = 0u32;
        loop {
            let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
            let data = if buffer.is_empty() {
                std::ptr::null_mut()
            } else {
                buffer.as_mut_ptr().cast()
            };
            // SAFETY: the strings end in NUL, and `size` is the byte length of `data`.
            let status = unsafe {
                RegGetValueW(
                    root,
                    key.as_ptr(),
                    name.as_ptr(),
                    RRF_RT_REG_SZ,
                    std::ptr::null_mut(),
                    data,
                    &mut size,
                )
            };
            match status {
                // A null buffer asks only for the size. Expansion can make the value longer
                // between calls.
                ERROR_SUCCESS if data.is_null() && size > 0 => continue,
                ERROR_MORE_DATA => continue,
                ERROR_SUCCESS => {
                    buffer.truncate(size as usize / 2);
                    while buffer.last() == Some(&0) {
                        buffer.pop();
                    }
                    return Some(OsString::from_wide(&buffer));
                }
                _ => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{join, last_line};

    #[test]
    fn the_path_is_the_last_line_after_profile_output() {
        assert_eq!(
            last_line("Welcome\n/usr/bin:/bin\n\n"),
            Some("/usr/bin:/bin")
        );
        assert_eq!(last_line("/bin\r\n"), Some("/bin"));
        assert_eq!(last_line("\n \n"), None);
    }
    #[test]
    fn registry_paths_join_machine_then_user() {
        let some = |s: &str| Some(s.into());
        assert_eq!(
            join([some(r"C:\Windows;"), some(r"C:\Users\a\bin")]),
            some(r"C:\Windows;C:\Users\a\bin")
        );
        assert_eq!(join([None, some(r"C:\bin")]), some(r"C:\bin"));
        assert_eq!(join([some(";"), None]), None);
    }
}
