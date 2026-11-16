// SPDX-License-Identifier: Apache-2.0
//! `focusctl ui`: opens the statistics and configuration page in the browser.
//!
//! The page needs the receiver's token to read anything. Putting the token on a command line
//! would show it to every user of the machine in the process list, so instead a small private
//! file is written that sends the browser on to the page with the token after the `#`, which
//! browsers never send to a server, and the browser is asked to open that file.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The page's address with the key after the `#`.
///
/// # Errors
/// When the token holds characters that could break out of the address. Tokens written by
/// `focusctl` are hexadecimal.
pub fn address(port: u16, token: &str) -> Result<String, String> {
    if token.is_empty() {
        return Err(
            "config.toml has no receiver token. Run `focusctl install-hooks` first.".into(),
        );
    }
    if !token
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(
            "the receiver token has unusual characters. Run `focusctl rotate-token` to get a \
             fresh one."
                .into(),
        );
    }
    Ok(format!("http://127.0.0.1:{port}/ui/#t={token}"))
}

/// A page that does nothing but go to `address`.
pub fn redirect_page(address: &str) -> String {
    format!(
        "<!doctype html>\n<meta charset=\"utf-8\">\n<meta name=\"referrer\" content=\"no-referrer\">\n\
         <title>Opening Focus Autopilot</title>\n<script>location.replace(\"{address}\");</script>\n\
         <p>Opening Focus Autopilot. If nothing happens, check that <code>focusd</code> is \
         running.</p>\n"
    )
}

/// Writes the redirect page into the data folder, readable by the owner only.
///
/// # Errors
/// When the file can't be written.
pub fn write_page(data_dir: &Path, html: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("{}: {e}", data_dir.display()))?;
    let path = data_dir.join("open-ui.html");
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
        .open(&path)
        .and_then(|mut file| file.write_all(html.as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        // The mode above only applies when the file is created; fix an older, looser one.
        use std::os::unix::fs::PermissionsExt as _;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(path)
}

/// Asks the desktop to open a file in the default browser. The path is passed as one
/// argument, never through a shell.
///
/// # Errors
/// When the opener can't be started or reports failure.
pub fn open_in_browser(path: &Path) -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let status = Command::new(opener)
        .arg(path)
        .status()
        .map_err(|e| format!("could not run `{opener}`: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{opener}` could not open the page"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_goes_after_the_hash_where_servers_never_see_it() {
        let address = address(47821, "abc123").unwrap();
        assert_eq!(address, "http://127.0.0.1:47821/ui/#t=abc123");
        let (before_hash, _) = address.split_once('#').unwrap();
        assert!(!before_hash.contains("abc123"));
    }

    #[test]
    fn a_missing_or_odd_token_is_refused_with_what_to_do() {
        assert!(address(47821, "").unwrap_err().contains("install-hooks"));
        for odd in ["a\"b", "a b", "a<b", "a#b", "a\\b", "a'b"] {
            assert!(
                address(47821, odd).unwrap_err().contains("rotate-token"),
                "{odd}"
            );
        }
    }

    #[test]
    fn the_page_only_redirects_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let html = redirect_page(&address(5000, "tok").unwrap());
        assert!(html.contains("location.replace(\"http://127.0.0.1:5000/ui/#t=tok\")"));
        let path = write_page(dir.path(), &html).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), html);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            // An existing, looser file is tightened when rewritten.
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            write_page(dir.path(), &html).unwrap();
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
