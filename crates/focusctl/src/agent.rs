// SPDX-License-Identifier: Apache-2.0
//! Launch at login (design backlog G3): a per-user launch agent that starts `focusd`. This
//! writes the plist; it does not load it. `focusctl` prints the one `launchctl` command that
//! does, so nothing is started behind the user's back.

use std::fs;
use std::path::{Path, PathBuf};

pub const LABEL: &str = "io.github.firedash.focusautopilot";

/// Escapes text for an XML element.
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The launch agent's property list.
///
/// A launch agent does not inherit the shell's `PATH`, so the one in force at install time is
/// saved, which is how `claude` (for LLM recaps) stays findable. It restarts after a crash but
/// not after a clean exit, and waits 30 s between tries so a bad config can't loop.
pub fn plist(focusd: &Path, path_env: &str, log: &Path) -> String {
    let program = xml(&focusd.to_string_lossy());
    let path = xml(path_env);
    let log = xml(&log.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{program}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ThrottleInterval</key>
  <integer>30</integer>
  <key>LimitLoadToSessionType</key>
  <string>Aqua</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>{path}</string>
  </dict>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
    )
}

pub fn plist_path(agents_dir: &Path) -> PathBuf {
    agents_dir.join(format!("{LABEL}.plist"))
}

/// Writes the plist and returns where it is.
///
/// # Errors
/// When `focusd` isn't next to `focusctl`, or the file can't be written.
pub fn install(
    agents_dir: &Path,
    focusctl: &Path,
    path_env: &str,
    log: &Path,
) -> Result<PathBuf, String> {
    let focusd = focusctl
        .parent()
        .map(|dir| dir.join("focusd"))
        .filter(|p| p.is_file())
        .ok_or(
            "focusd must be in the same folder as focusctl (build both with cargo build --release)",
        )?;
    if let Some(dir) = log.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(agents_dir).map_err(|e| format!("{}: {e}", agents_dir.display()))?;
    let path = plist_path(agents_dir);
    fs::write(&path, plist(&focusd, path_env, log))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Removes the plist. `Ok(false)` when there was none.
///
/// # Errors
/// When the file exists but can't be removed.
pub fn uninstall(agents_dir: &Path) -> Result<bool, String> {
    let path = plist_path(agents_dir);
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plist_has_what_launchd_needs() {
        let text = plist(
            Path::new("/Users/me/focus/target/release/focusd"),
            "/usr/local/bin:/usr/bin",
            Path::new("/Users/me/Library/Application Support/x/focusd.log"),
        );
        assert!(text.contains("<string>io.github.firedash.focusautopilot</string>"));
        assert!(text.contains("<string>/Users/me/focus/target/release/focusd</string>"));
        assert!(text.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(text.contains("<key>PATH</key>\n    <string>/usr/local/bin:/usr/bin</string>"));
        assert!(text.contains("<key>SuccessfulExit</key>\n    <false/>"));
        assert!(text.contains("Aqua"));
        assert!(text.contains("focusd.log"));
    }

    #[test]
    fn paths_with_xml_characters_are_escaped() {
        let text = plist(Path::new("/a&b/<x>/focusd"), "/p&q", Path::new("/l"));
        assert!(text.contains("/a&amp;b/&lt;x&gt;/focusd"));
        assert!(text.contains("/p&amp;q"));
        assert!(!text.contains("a&b"));
    }

    #[test]
    fn install_writes_next_to_focusd_and_uninstall_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("focusd"), "").unwrap();
        let agents = dir.path().join("LaunchAgents");
        let path = install(
            &agents,
            &bin.join("focusctl"),
            "/usr/bin",
            &dir.path().join("d/focusd.log"),
        )
        .unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("RunAtLoad"));
        assert!(uninstall(&agents).unwrap());
        assert!(!uninstall(&agents).unwrap());
    }

    #[test]
    fn install_refuses_without_a_focusd_beside_focusctl() {
        let dir = tempfile::tempdir().unwrap();
        let err = install(
            &dir.path().join("a"),
            &dir.path().join("focusctl"),
            "",
            &dir.path().join("l"),
        )
        .unwrap_err();
        assert!(err.contains("focusd"), "{err}");
    }
}
