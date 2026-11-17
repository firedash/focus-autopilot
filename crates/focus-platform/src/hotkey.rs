// SPDX-License-Identifier: Apache-2.0
//! Hotkey strings from `config.toml` (design section 11), parsed into a chord, plus the
//! [`Hotkeys`] trait the shell uses. The parser is plain Rust so it is tested everywhere; only
//! registering a chord with the OS is platform code.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    /// Acts on the visible toast: go back, switch now, or snooze.
    Focus,
    TogglePause,
    /// "This run will take a while": switch me away now (design backlog E4).
    DeclareLong,
}

/// The hotkey strings from the config. An empty string leaves that hotkey off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keys<'a> {
    pub focus: &'a str,
    pub pause: &'a str,
    pub declare_long: &'a str,
}

impl<'a> Keys<'a> {
    /// Each key with the action it triggers and the name used in messages.
    pub fn each(&self) -> [(&'a str, HotkeyAction, &'static str); 3] {
        [
            (self.focus, HotkeyAction::Focus, "focus"),
            (self.pause, HotkeyAction::TogglePause, "pause"),
            (self.declare_long, HotkeyAction::DeclareLong, "declare-long"),
        ]
    }
}

/// Four independent modifier flags read naturally as bools here.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    /// Upper case: `A`..`Z`, `0`..`9`, `SPACE`, `RETURN`, `ESCAPE`, `TAB`.
    pub key: String,
}

/// Parses `"Control+Option+Command+Space"` style strings. An empty string turns the hotkey
/// off. At least one modifier is required, so a bare key can't swallow typing everywhere.
///
/// # Errors
/// An unknown modifier or key, a missing key or modifier, or more than one key.
pub fn parse(text: &str) -> Result<Option<Chord>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let mut chord = Chord {
        ctrl: false,
        alt: false,
        shift: false,
        cmd: false,
        key: String::new(),
    };
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "control" | "ctrl" => chord.ctrl = true,
            "option" | "alt" => chord.alt = true,
            "shift" => chord.shift = true,
            "command" | "cmd" | "super" | "meta" => chord.cmd = true,
            key => {
                if !chord.key.is_empty() {
                    return Err(format!("`{text}` has more than one key"));
                }
                let upper = key.to_ascii_uppercase();
                let upper = match upper.as_str() {
                    "ENTER" => "RETURN".to_owned(),
                    "ESC" => "ESCAPE".to_owned(),
                    _ => upper,
                };
                if mac_keycode(&upper).is_none() {
                    return Err(format!("`{part}` in `{text}` is not a key I know"));
                }
                chord.key = upper;
            }
        }
    }
    if chord.key.is_empty() {
        return Err(format!("`{text}` has no key"));
    }
    if !(chord.ctrl || chord.alt || chord.shift || chord.cmd) {
        return Err(format!("`{text}` needs at least one modifier"));
    }
    Ok(Some(chord))
}

/// How a chord reads on a Mac menu: `⌃⌥⇧⌘Space`.
pub fn pretty(chord: &Chord) -> String {
    let mut out = String::new();
    for (on, symbol) in [
        (chord.ctrl, '⌃'),
        (chord.alt, '⌥'),
        (chord.shift, '⇧'),
        (chord.cmd, '⌘'),
    ] {
        if on {
            out.push(symbol);
        }
    }
    let key = match chord.key.as_str() {
        "SPACE" => "Space",
        "RETURN" => "Return",
        "ESCAPE" => "Esc",
        "TAB" => "Tab",
        other => other,
    };
    out.push_str(key);
    out
}

/// How a chord reads on Windows: `Ctrl+Alt+Shift+Win+Space`.
pub fn pretty_windows(chord: &Chord) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for (on, name) in [
        (chord.ctrl, "Ctrl"),
        (chord.alt, "Alt"),
        (chord.shift, "Shift"),
        (chord.cmd, "Win"),
    ] {
        if on {
            parts.push(name);
        }
    }
    parts.push(match chord.key.as_str() {
        "SPACE" => "Space",
        "RETURN" => "Enter",
        "ESCAPE" => "Esc",
        "TAB" => "Tab",
        other => other,
    });
    parts.join("+")
}

/// How a chord reads on the OS this build is for.
pub fn pretty_here(chord: &Chord) -> String {
    if cfg!(windows) {
        pretty_windows(chord)
    } else {
        pretty(chord)
    }
}

/// Windows virtual-key codes for the keys `parse` accepts. Letters and digits are their
/// ASCII codes.
pub fn win_vk(key: &str) -> Option<u32> {
    match key {
        "SPACE" => Some(0x20),
        "RETURN" => Some(0x0D),
        "ESCAPE" => Some(0x1B),
        "TAB" => Some(0x09),
        _ => {
            let mut chars = key.chars();
            let only = chars.next().filter(|_| chars.next().is_none())?;
            (only.is_ascii_uppercase() || only.is_ascii_digit()).then(|| u32::from(only))
        }
    }
}

/// macOS virtual key codes (ANSI layout) for the keys `parse` accepts.
pub fn mac_keycode(key: &str) -> Option<u32> {
    Some(match key {
        "A" => 0,
        "S" => 1,
        "D" => 2,
        "F" => 3,
        "H" => 4,
        "G" => 5,
        "Z" => 6,
        "X" => 7,
        "C" => 8,
        "V" => 9,
        "B" => 11,
        "Q" => 12,
        "W" => 13,
        "E" => 14,
        "R" => 15,
        "Y" => 16,
        "T" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "9" => 25,
        "7" => 26,
        "8" => 28,
        "0" => 29,
        "O" => 31,
        "U" => 32,
        "I" => 34,
        "P" => 35,
        "RETURN" => 36,
        "L" => 37,
        "J" => 38,
        "K" => 40,
        "TAB" => 48,
        "SPACE" => 49,
        "ESCAPE" => 53,
        "N" => 45,
        "M" => 46,
        _ => return None,
    })
}

/// Global hotkeys. Registering one must not trigger an OS permission prompt.
pub trait Hotkeys {
    /// Registers the hotkeys. An empty string leaves one off. Returns one message for each
    /// hotkey that couldn't be registered (bad string, or another app owns the chord), each
    /// starting with the hotkey's name.
    fn register(&self, keys: &Keys<'_>) -> Vec<String>;

    /// Presses since the last call, oldest first.
    fn take_pressed(&self) -> Vec<HotkeyAction>;

    /// True once at least one hotkey is registered, so the shell knows to poll for presses.
    fn active(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_chords_parse() {
        let mac = parse("Control+Option+Command+Space").unwrap().unwrap();
        assert!(mac.ctrl && mac.alt && mac.cmd && !mac.shift);
        assert_eq!(mac.key, "SPACE");
        assert_eq!(pretty(&mac), "⌃⌥⌘Space");
        let win = parse("Ctrl+Alt+Shift+P").unwrap().unwrap();
        assert_eq!(pretty(&win), "⌃⌥⇧P");
        assert_eq!(mac_keycode("SPACE"), Some(49));
        assert_eq!(mac_keycode("P"), Some(35));
    }

    #[test]
    fn chords_read_and_register_the_windows_way_too() {
        let win = parse("Ctrl+Alt+Space").unwrap().unwrap();
        assert_eq!(pretty_windows(&win), "Ctrl+Alt+Space");
        let all = parse("Control+Option+Shift+Command+Enter")
            .unwrap()
            .unwrap();
        assert_eq!(pretty_windows(&all), "Ctrl+Alt+Shift+Win+Enter");
        assert_eq!(win_vk("SPACE"), Some(0x20));
        assert_eq!(win_vk("P"), Some(0x50));
        assert_eq!(win_vk("7"), Some(0x37));
        assert_eq!(win_vk("PP"), None);
        assert_eq!(win_vk("p"), None);
        // Every key the parser accepts has a code on both systems.
        for key in ('A'..='Z').chain('0'..='9').map(|c| c.to_string()) {
            assert!(
                win_vk(&key).is_some() && mac_keycode(&key).is_some(),
                "{key}"
            );
        }
        for key in ["SPACE", "RETURN", "ESCAPE", "TAB"] {
            assert!(win_vk(key).is_some() && mac_keycode(key).is_some(), "{key}");
        }
    }

    #[test]
    fn empty_means_off_and_bad_strings_are_errors() {
        assert_eq!(parse(""), Ok(None));
        assert_eq!(parse("  "), Ok(None));
        assert!(parse("Space").is_err());
        assert!(parse("Control+").is_err());
        assert!(parse("Control+A+B").is_err());
        assert!(parse("Control+Banana").is_err());
        assert!(parse("Cmd+Enter").is_ok());
        assert_eq!(parse("cmd+esc").unwrap().unwrap().key, "ESCAPE");
    }

    #[test]
    fn every_accepted_letter_and_digit_has_a_distinct_keycode() {
        let mut seen = std::collections::BTreeSet::new();
        for key in ('A'..='Z').chain('0'..='9') {
            let code = mac_keycode(&key.to_string()).unwrap();
            assert!(seen.insert(code), "{key} repeats code {code}");
        }
    }
}
