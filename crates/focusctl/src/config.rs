// SPDX-License-Identifier: Apache-2.0
//! `config.toml` edits that `focusctl` makes: the receiver port and token. Everything else in
//! the file, comments included, is left as the user wrote it.

use std::fs;
use std::path::Path;

use toml_edit::{DocumentMut, value};

pub const DEFAULT_PORT: u16 = 47821;

pub struct Receiver {
    pub port: u16,
    pub token: String,
}

/// The configured port, or the default if there is no config yet. Never writes.
pub fn ensure_port(path: &Path) -> u16 {
    let port = || {
        let doc = fs::read_to_string(path).ok()?.parse::<DocumentMut>().ok()?;
        let port = doc.get("receiver")?.get("port")?.as_integer()?;
        u16::try_from(port).ok()
    };
    port().unwrap_or(DEFAULT_PORT)
}

/// The configured token, if any. Never writes.
pub fn token_of(path: &Path) -> Option<String> {
    let doc = fs::read_to_string(path).ok()?.parse::<DocumentMut>().ok()?;
    doc.get("receiver")?
        .get("token")?
        .as_str()
        .map(str::to_owned)
}

/// Reads the receiver settings, creating a token if there is none (or always when `rotate`),
/// and saves the file if it changed.
///
/// # Errors
/// An unreadable or invalid file, or a failed write.
pub fn ensure_token(path: &Path, rotate: bool) -> Result<Receiver, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let receiver = doc
        .entry("receiver")
        .or_insert_with(toml_edit::table)
        .as_table_mut()
        .ok_or("`receiver` in config.toml is not a table")?;
    let port = receiver
        .get("port")
        .and_then(toml_edit::Item::as_integer)
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or(DEFAULT_PORT);
    let existing = receiver
        .get("token")
        .and_then(toml_edit::Item::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    let token = match existing {
        Some(token) if !rotate => token,
        _ => {
            let token = focus_receiver::random_hex().map_err(|e| e.to_string())?;
            receiver["token"] = value(token.clone());
            token
        }
    };
    let new = doc.to_string();
    if new != text {
        write_private(path, &new)?;
    }
    Ok(Receiver { port, token })
}

fn write_private(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            // Only a folder we may own: never loosen or tighten an existing shared one.
            if dir
                .file_name()
                .is_some_and(|n| n.to_string_lossy().contains("focusautopilot"))
            {
                let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
            }
        }
    }
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_token_once_and_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "# mine\nenabled = true\n[away]\napps = [\"X\"]\n").unwrap();
        let first = ensure_token(&path, false).unwrap();
        assert_eq!(first.port, DEFAULT_PORT);
        assert_eq!(first.token.len(), 64);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# mine\nenabled = true"));
        assert!(text.contains(&first.token));
        assert_eq!(ensure_token(&path, false).unwrap().token, first.token);
        let rotated = ensure_token(&path, true).unwrap();
        assert_ne!(rotated.token, first.token);
        assert!(!fs::read_to_string(&path).unwrap().contains(&first.token));
    }

    #[test]
    fn port_lookup_survives_a_config_without_a_receiver_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(ensure_port(&path), DEFAULT_PORT);
        fs::write(&path, "enabled = true\n").unwrap();
        assert_eq!(ensure_port(&path), DEFAULT_PORT);
        assert_eq!(token_of(&path), None);
    }

    #[test]
    fn a_custom_port_is_respected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[receiver]\nport = 5555\n").unwrap();
        assert_eq!(ensure_token(&path, false).unwrap().port, 5555);
    }

    #[cfg(unix)]
    #[test]
    fn the_config_file_is_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        ensure_token(&path, false).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
