// SPDX-License-Identifier: Apache-2.0
//! Where the app keeps its files (design section 11).

use std::path::PathBuf;

const BUNDLE_ID: &str = "io.github.firedash.focusautopilot";

/// The user's home folder: `HOME`, or on Windows, where `HOME` is normally not set,
/// `USERPROFILE`. Claude Code keeps its settings under it on every system.
pub fn home_dir() -> Option<PathBuf> {
    home_from(
        std::env::var_os("HOME"),
        std::env::var_os("USERPROFILE"),
        cfg!(windows),
    )
}

fn home_from(
    home: Option<std::ffi::OsString>,
    profile: Option<std::ffi::OsString>,
    windows: bool,
) -> Option<PathBuf> {
    let set = |value: Option<std::ffi::OsString>| value.filter(|v| !v.is_empty());
    let chosen = if windows {
        set(home).or_else(|| set(profile))
    } else {
        set(home)
    };
    chosen.map(PathBuf::from)
}

/// The data folder: `FOCUS_AUTOPILOT_HOME` if set, else the OS's per-user location.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("FOCUS_AUTOPILOT_HOME") {
        return Some(PathBuf::from(dir));
    }
    let home = home_dir()?;
    if cfg!(target_os = "macos") {
        Some(home.join("Library/Application Support").join(BUNDLE_ID))
    } else if cfg!(windows) {
        // Per-user and not synced between machines: the token and history stay local.
        let base = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map_or_else(|| home.join("AppData").join("Local"), PathBuf::from);
        Some(base.join(BUNDLE_ID))
    } else {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map_or_else(|| home.join(".local/share"), PathBuf::from);
        Some(base.join(BUNDLE_ID))
    }
}

pub fn config_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn windows_falls_back_to_the_user_profile_and_other_systems_do_not() {
        let home = || Some(OsString::from("/home/me"));
        let profile = || Some(OsString::from("C:\\Users\\me"));
        assert_eq!(home_from(home(), profile(), true), Some("/home/me".into()));
        assert_eq!(
            home_from(None, profile(), true),
            Some("C:\\Users\\me".into())
        );
        assert_eq!(
            home_from(Some(OsString::new()), profile(), true),
            Some("C:\\Users\\me".into())
        );
        assert_eq!(home_from(None, profile(), false), None);
        assert_eq!(home_from(None, None, true), None);
    }
}
