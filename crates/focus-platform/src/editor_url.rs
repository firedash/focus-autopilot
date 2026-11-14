// SPDX-License-Identifier: Apache-2.0
//! The URL that asks an editor for the window that has a project folder open
//! (`vscode://file/<folder>`). Plain Rust so it is tested everywhere; opening the URL is
//! platform code. Opening a URL starts no process and needs no permission.

/// The URL scheme of an editor that opens a folder from `scheme://file/<path>`, by app id.
/// Only editors whose behaviour has been checked belong here.
pub fn scheme_for(app_id: &str) -> Option<&'static str> {
    match app_id {
        "com.microsoft.VSCode" | "Code.exe" => Some("vscode"),
        "com.microsoft.VSCodeInsiders" => Some("vscode-insiders"),
        "com.todesktop.230313mzl4w4u92" | "Cursor.exe" => Some("cursor"),
        _ => None,
    }
}

/// `scheme://file/<percent-encoded folder>`, or `None` if `app_id` is not such an editor or
/// `folder` is not an absolute path.
pub fn editor_url(app_id: &str, folder: &str) -> Option<String> {
    let scheme = scheme_for(app_id)?;
    let path = folder.replace('\\', "/");
    let path = if path.starts_with('/') {
        path
    } else if has_drive_letter(&path) {
        format!("/{path}")
    } else {
        return None;
    };
    if path.chars().any(char::is_control) {
        return None;
    }
    Some(format!("{scheme}://file{}", encode(&path)))
}

/// Upper-case hexadecimal digit for a value under 16.
fn hex_digit(nibble: u8) -> char {
    char::from(b"0123456789ABCDEF"[usize::from(nibble & 0x0F)])
}

fn has_drive_letter(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
}

/// Percent-encodes everything except unreserved characters, `/` and the `:` of a drive.
fn encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for (i, byte) in path.bytes().enumerate() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
            || (byte == b':' && i == 2);
        if keep {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(hex_digit(byte >> 4));
            out.push(hex_digit(byte & 0x0F));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const VSCODE: &str = "com.microsoft.VSCode";

    #[test]
    fn the_folder_from_the_manual_test_becomes_the_url_that_worked() {
        assert_eq!(
            editor_url(VSCODE, "/Users/avinash/focus-autopilot/").as_deref(),
            Some("vscode://file/Users/avinash/focus-autopilot/")
        );
    }

    #[test]
    fn spaces_and_url_characters_are_encoded() {
        assert_eq!(
            editor_url(VSCODE, "/Users/me/My Projects/a#b?c").as_deref(),
            Some("vscode://file/Users/me/My%20Projects/a%23b%3Fc")
        );
        assert_eq!(
            editor_url(VSCODE, "/work/café").as_deref(),
            Some("vscode://file/work/caf%C3%A9")
        );
    }

    #[test]
    fn each_editor_has_its_scheme_and_others_have_none() {
        assert!(
            editor_url("com.microsoft.VSCodeInsiders", "/x")
                .unwrap()
                .starts_with("vscode-insiders://file/")
        );
        assert!(
            editor_url("com.todesktop.230313mzl4w4u92", "/x")
                .unwrap()
                .starts_with("cursor://file/")
        );
        assert_eq!(editor_url("com.apple.Terminal", "/x"), None);
        assert_eq!(editor_url("com.jetbrains.intellij", "/x"), None);
    }

    #[test]
    fn windows_paths_work_and_relative_or_odd_paths_are_refused() {
        assert_eq!(
            editor_url("Code.exe", r"C:\Users\me\app").as_deref(),
            Some("vscode://file/C:/Users/me/app")
        );
        assert_eq!(editor_url(VSCODE, "relative/path"), None);
        assert_eq!(editor_url(VSCODE, ""), None);
        assert_eq!(editor_url(VSCODE, "/a\nb"), None);
    }
}
