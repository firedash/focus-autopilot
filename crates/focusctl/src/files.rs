// SPDX-License-Identifier: Apache-2.0
//! File handling for `settings.json` and `config.toml`: strict JSON only, symlinks followed,
//! a backup, and an atomic write that starts over if the file changed underneath us.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::hooks;

pub enum Op {
    Install { port: u16, token: String },
    Uninstall { port: u16 },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub target: PathBuf,
    /// Set when the settings file is a symlink and `target` is where it points.
    pub via_symlink: Option<PathBuf>,
    pub old: String,
    pub new: String,
}

impl Outcome {
    pub fn changed(&self) -> bool {
        self.old != self.new
    }
}

/// Follows a symlink so the link itself is never replaced by a regular file.
fn resolve(path: &Path) -> Result<(PathBuf, Option<PathBuf>), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let target = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok((target, Some(path.to_owned())))
        }
        _ => Ok((path.to_owned(), None)),
    }
}

fn compute(old: &str, op: &Op) -> Result<String, String> {
    let existed = !old.trim().is_empty();
    let mut value: Value = if existed {
        serde_json::from_str(old)
            .map_err(|e| format!("settings.json is not strict JSON ({e}); nothing was changed"))?
    } else {
        Value::Object(serde_json::Map::new())
    };
    match op {
        Op::Install { port, token } => hooks::install(&mut value, *port, token),
        Op::Uninstall { port } => hooks::uninstall(&mut value, *port).map(|_| ()),
    }
    .map_err(|e| format!("{e:?}"))?;
    if existed {
        let new = hooks::render(&value, &hooks::indent_of(old), old.ends_with('\n'));
        // Nothing to do: keep the exact original bytes rather than a re-rendering.
        let before: Value = serde_json::from_str(old).map_err(|e| e.to_string())?;
        Ok(if before == value { old.to_owned() } else { new })
    } else {
        Ok(hooks::render(&value, "  ", true))
    }
}

/// Computes the new `settings.json` and, unless `dry_run`, writes it.
///
/// # Errors
/// An unreadable or non-strict-JSON file, an unexpected shape, or a failed write.
pub fn apply(settings: &Path, op: &Op, dry_run: bool) -> Result<Outcome, String> {
    let (target, via_symlink) = resolve(settings)?;
    for _ in 0..3 {
        let old = match fs::read_to_string(&target) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("{}: {e}", target.display())),
        };
        let mut new = compute(&old, op)?;
        if matches!(op, Op::Uninstall { .. }) {
            new = restore_original_bytes(&target, &old, new);
        }
        let outcome = Outcome {
            target: target.clone(),
            via_symlink: via_symlink.clone(),
            old,
            new,
        };
        if dry_run || !outcome.changed() {
            return Ok(outcome);
        }
        if write_checked(&outcome)? {
            return Ok(outcome);
        }
    }
    Err("settings.json kept changing while it was being updated; try again".into())
}

/// After an uninstall, the file's content equals the user's original but its formatting may
/// differ from what they wrote. If the first backup holds exactly that content, use its bytes.
fn restore_original_bytes(target: &Path, current: &str, new: String) -> String {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".focus-autopilot.bak");
    let Ok(backup) = fs::read_to_string(target.with_file_name(name)) else {
        return new;
    };
    let same = |a: &str, b: &str| {
        matches!((serde_json::from_str::<Value>(a), serde_json::from_str::<Value>(b)),
            (Ok(x), Ok(y)) if x == y)
    };
    if current != new && same(&backup, &new) {
        backup
    } else {
        new
    }
}

/// `Ok(false)` when the file changed since it was read, so the caller starts over.
fn write_checked(o: &Outcome) -> Result<bool, String> {
    let dir = o.target.parent().ok_or("settings path has no folder")?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if !o.old.is_empty() {
        let mut name = o.target.file_name().unwrap_or_default().to_os_string();
        name.push(".focus-autopilot.bak");
        let backup = dir.join(name);
        // Keep the first backup: it is the file as the user wrote it.
        if !backup.exists() {
            fs::write(&backup, &o.old).map_err(|e| format!("{}: {e}", backup.display()))?;
        }
    }
    let tmp = dir.join(format!(
        ".settings.focus-autopilot.{}.tmp",
        std::process::id()
    ));
    {
        let mut file = fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        file.write_all(o.new.as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    if let Ok(meta) = fs::metadata(&o.target) {
        let _ = fs::set_permissions(&tmp, meta.permissions());
    }
    let current = fs::read_to_string(&o.target).unwrap_or_default();
    if current != o.old {
        let _ = fs::remove_file(&tmp);
        return Ok(false);
    }
    fs::rename(&tmp, &o.target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{}: {e}", o.target.display())
    })?;
    Ok(true)
}

/// A line diff in unified style, without context trimming; the files are small.
pub fn diff(old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let mut lcs = vec![vec![0_usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = String::new();
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            out.push('+');
            out.push_str(b[j]);
            out.push('\n');
            j += 1;
        } else {
            out.push('-');
            out.push_str(a[i]);
            out.push('\n');
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(port: u16, token: &str) -> Op {
        Op::Install {
            port,
            token: token.into(),
        }
    }

    #[test]
    fn install_twice_then_uninstall_round_trips_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let original = "{\n    \"model\": \"x\", \"note\": \"caf\\u00e9\",\n    \"hooks\": {\n        \"Stop\": [ { \"hooks\": [ { \"type\": \"command\", \"command\": \"say done\" } ] } ]\n    }\n}\n";
        fs::write(&path, original).unwrap();
        assert!(apply(&path, &install(47821, "t"), false).unwrap().changed());
        let once = fs::read_to_string(&path).unwrap();
        assert!(!apply(&path, &install(47821, "t"), false).unwrap().changed());
        assert_eq!(fs::read_to_string(&path).unwrap(), once);
        apply(&path, &Op::Uninstall { port: 47821 }, false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        let backup = dir.path().join("settings.json.focus-autopilot.bak");
        assert_eq!(fs::read_to_string(backup).unwrap(), original);
    }

    #[test]
    fn dry_run_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{}\n").unwrap();
        let outcome = apply(&path, &install(47821, "t"), true).unwrap();
        assert!(outcome.changed());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
        assert!(diff(&outcome.old, &outcome.new).contains("+  \"hooks\": {"));
    }

    #[test]
    fn non_strict_json_is_refused_and_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let text = "{\n  // comment\n  \"a\": 1\n}\n";
        fs::write(&path, text).unwrap();
        assert!(apply(&path, &install(47821, "t"), false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_file_stays_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles-settings.json");
        let link = dir.path().join("settings.json");
        fs::write(&real, "{}\n").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let outcome = apply(&link, &install(47821, "t"), false).unwrap();
        assert!(outcome.via_symlink.is_some());
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::read_to_string(&real).unwrap().contains("X-Focus-Token"));
    }

    #[test]
    fn a_missing_file_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".claude/settings.json");
        apply(&path, &install(47821, "t"), false).unwrap();
        assert!(
            fs::read_to_string(path)
                .unwrap()
                .contains("UserPromptSubmit")
        );
    }
}
