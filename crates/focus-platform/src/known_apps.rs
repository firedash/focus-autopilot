// SPDX-License-Identifier: Apache-2.0
//! Built-in home apps (design section 8): the terminals, IDEs and Claude app a prompt can be
//! sent from. `home.extra_apps` in the config adds to this list. An entry ending in `*`
//! matches by prefix.
//!
//! UNVERIFIED: these ids were written from memory. `focusd` prints the real id of the app in
//! front (`front: ...`), and says so when a prompt comes from an app that is not listed.

/// Bundle ids.
pub const MACOS: &[&str] = &[
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "com.github.wez.wezterm",
    "net.kovidgoyal.kitty",
    "org.alacritty",
    "dev.warp.Warp-Stable",
    "com.microsoft.VSCode",
    "com.microsoft.VSCodeInsiders",
    "com.todesktop.230313mzl4w4u92", // Cursor
    "com.exafunction.windsurf",
    "dev.zed.Zed",
    "com.jetbrains.*",
    "com.anthropic.claudefordesktop",
];

/// Executable names. A classic console window can report the console host or the shell in
/// it, depending on the Windows version, so both are listed.
pub const WINDOWS: &[&str] = &[
    "WindowsTerminal.exe",
    "conhost.exe",
    "OpenConsole.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "Code.exe",
    "Cursor.exe",
    "Windsurf.exe",
    "idea64.exe",
    "pycharm64.exe",
    "webstorm64.exe",
    "goland64.exe",
    "clion64.exe",
    "rider64.exe",
    "rustrover64.exe",
    "phpstorm64.exe",
    "rubymine64.exe",
    "claude.exe",
];

/// The built-in list for the OS this build targets, in the form `focus_core::Core::new` takes.
pub fn for_this_os() -> Vec<String> {
    let list = if cfg!(windows) { WINDOWS } else { MACOS };
    list.iter().map(|id| (*id).to_owned()).collect()
}
