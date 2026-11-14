// SPDX-License-Identifier: Apache-2.0
//! Where the app keeps its files (design section 11).

use std::path::PathBuf;

const BUNDLE_ID: &str = "io.github.firedash.focusautopilot";

/// The data folder: `FOCUS_AUTOPILOT_HOME` if set, else the OS's per-user location.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("FOCUS_AUTOPILOT_HOME") {
        return Some(PathBuf::from(dir));
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if cfg!(target_os = "macos") {
        Some(home.join("Library/Application Support").join(BUNDLE_ID))
    } else {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map_or_else(|| home.join(".local/share"), PathBuf::from);
        Some(base.join(BUNDLE_ID))
    }
}

pub fn config_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("config.toml"))
}
