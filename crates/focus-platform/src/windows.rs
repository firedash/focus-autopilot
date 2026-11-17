// SPDX-License-Identifier: Apache-2.0
//! The Windows adapter (design section 8), through Microsoft's `windows-sys` bindings.
//!
//! What it reads: which window is in front and which executable owns it, which top-level
//! windows exist, and how long ago the last input was (a time, never the input). It reads the
//! length of a window's title to tell real windows from invisible helpers, never the title.
//! Nothing here needs a permission prompt or elevation.
//!
//! One thing it does that the macOS adapter does not: when Windows refuses to bring a window
//! forward, it sends one empty mouse event (no movement, no button) and asks again. See
//! [`WinPlatform::set_force_foreground`] and ADR 0006.
//!
//! Status: type-checked from Linux and built and tested on a Windows CI runner. It has not
//! been used by a person on a Windows desktop.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use focus_core::{AppRef, Sound};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_HOTKEY_ALREADY_REGISTERED, GetLastError, HWND, LPARAM, POINT,
};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetLastInputInfo, INPUT, INPUT_0, INPUT_MOUSE, LASTINPUTINFO, MOD_ALT, MOD_CONTROL,
    MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, MOUSEINPUT, RegisterHotKey, SendInput, UnregisterHotKey,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GW_OWNER, GWL_EXSTYLE, GetForegroundWindow, GetWindow,
    GetWindowLongW, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, MB_ICONASTERISK, MSG, PM_REMOVE, PeekMessageW, SW_RESTORE,
    SetForegroundWindow, ShowWindow, TranslateMessage, WM_HOTKEY, WS_EX_TOOLWINDOW,
};

use crate::hotkey::{self, HotkeyAction, Hotkeys};
use crate::windows_overlay::WinOverlay;
use crate::{Overlay, Platform, PlatformError, Result};

/// The id used when the executable behind a window can't be read (a protected process).
const UNKNOWN_EXE: &str = "unknown.exe";

/// Create it on the thread that runs the shell loop: its windows and hotkeys deliver their
/// messages to the thread that made them.
pub struct WinPlatform {
    overlay: WinOverlay,
    hotkeys_on: Cell<bool>,
    pressed: RefCell<Vec<HotkeyAction>>,
    force_foreground: Cell<bool>,
}

impl WinPlatform {
    /// Deliberately no `Default`: building one has side effects and a thread requirement.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            overlay: WinOverlay::new(),
            hotkeys_on: Cell::new(false),
            pressed: RefCell::new(Vec::new()),
            force_foreground: Cell::new(true),
        }
    }

    /// Whether a refused switch is retried after one empty mouse event. Windows lets a
    /// process bring a window forward only if it received the last input, so a switch made on
    /// a timer is normally refused; the empty event gives this process that standing. Off:
    /// such a switch stays refused and the core shows its "go back?" prompt, where the focus
    /// key works because a hotkey press is itself input to this process.
    pub fn set_force_foreground(&self, on: bool) {
        self.force_foreground.set(on);
    }

    /// Takes every waiting message off this thread's queue: hotkey presses are kept for
    /// [`Hotkeys::take_pressed`], the rest go to the overlay windows.
    fn pump(&self) {
        let mut msg = MSG {
            hwnd: std::ptr::null_mut(),
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT { x: 0, y: 0 },
        };
        // SAFETY: `msg` is a valid, writable MSG for each call; a null window handle means
        // "any message for this thread".
        while unsafe { PeekMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            if msg.message == WM_HOTKEY {
                if let Some(action) = action_of(msg.wParam) {
                    self.pressed.borrow_mut().push(action);
                }
            } else {
                // SAFETY: `msg` was filled in by PeekMessageW just above.
                unsafe {
                    TranslateMessage(&raw const msg);
                    DispatchMessageW(&raw const msg);
                }
            }
        }
    }
}

/// A window handle as the number kept in [`AppRef::window`].
fn handle_number(hwnd: HWND) -> Option<u64> {
    u64::try_from(hwnd.addr()).ok()
}

/// The reverse of [`handle_number`]. A window handle is a number the OS gave out, not memory
/// this process may read, so nothing is ever dereferenced through it.
fn handle_of(number: u64) -> Option<HWND> {
    let addr = usize::try_from(number).ok()?;
    Some(std::ptr::with_exposed_provenance_mut::<c_void>(addr))
}

/// The file name of the executable that owns `pid`, such as `WindowsTerminal.exe`.
fn exe_of(pid: u32) -> Option<String> {
    // SAFETY: plain values in; a null handle means the process could not be opened.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut path = [0_u16; 1024];
    let mut len = u32::try_from(path.len()).unwrap_or(0);
    // SAFETY: `path` is writable for `len` UTF-16 units, `len` is a valid in/out length, and
    // `process` is the live handle opened above.
    let ok = unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &raw mut len)
    };
    // SAFETY: `process` came from OpenProcess and is closed exactly once.
    unsafe { CloseHandle(process) };
    if ok == 0 {
        return None;
    }
    let written = path.get(..usize::try_from(len).ok()?)?;
    let full = String::from_utf16_lossy(written);
    let name = full.rsplit(['\\', '/']).next()?;
    (!name.is_empty()).then(|| name.to_owned())
}

/// `WindowsTerminal.exe` reads as `WindowsTerminal`.
fn display_name(exe: &str) -> String {
    let stem = exe
        .len()
        .checked_sub(4)
        .and_then(|at| exe.split_at_checked(at))
        .filter(|(_, ext)| ext.eq_ignore_ascii_case(".exe"))
        .map_or(exe, |(stem, _)| stem);
    stem.to_owned()
}

fn app_of(hwnd: HWND) -> Option<AppRef> {
    let mut pid = 0_u32;
    // SAFETY: `pid` is a valid out pointer. A stale handle makes the call return 0 and leave
    // `pid` at 0, which is checked.
    unsafe { GetWindowThreadProcessId(hwnd, &raw mut pid) };
    if pid == 0 {
        return None;
    }
    let id = exe_of(pid).unwrap_or_else(|| UNKNOWN_EXE.to_owned());
    Some(AppRef {
        name: display_name(&id),
        id,
        pid,
        window: handle_number(hwnd),
    })
}

/// True for a window a person would call an app window: visible, top-level with no owner,
/// not a tool window, titled, and not hidden by the window manager.
fn is_app_window(hwnd: HWND) -> bool {
    // SAFETY: every call below takes the handle by value and tolerates a stale one by
    // returning zero or null; `cloaked` is a valid 4-byte out value of the size passed.
    unsafe {
        if IsWindowVisible(hwnd) == 0 || !GetWindow(hwnd, GW_OWNER).is_null() {
            return false;
        }
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE).cast_unsigned();
        if ex_style & WS_EX_TOOLWINDOW != 0 || GetWindowTextLengthW(hwnd) <= 0 {
            return false;
        }
        let mut cloaked = 0_u32;
        let read = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED.cast_unsigned(),
            (&raw mut cloaked).cast::<c_void>(),
            4,
        );
        !(read == 0 && cloaked != 0)
    }
}

/// Every top-level window, front to back.
fn top_level_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, out: LPARAM) -> i32 {
        let out = std::ptr::with_exposed_provenance_mut::<Vec<HWND>>(out.cast_unsigned());
        // SAFETY: `out` is the address of the Vec that `top_level_windows` keeps alive and
        // does not touch for the whole EnumWindows call, which runs this on the same thread.
        unsafe { (*out).push(hwnd) };
        1
    }
    let mut found: Vec<HWND> = Vec::new();
    let out = (&raw mut found).expose_provenance().cast_signed();
    // SAFETY: `collect` matches WNDENUMPROC, and `out` points at `found`, which outlives the
    // call.
    unsafe { EnumWindows(Some(collect), out) };
    found
}

fn app_windows() -> Vec<AppRef> {
    top_level_windows()
        .into_iter()
        .filter(|hwnd| is_app_window(*hwnd))
        .filter_map(app_of)
        .collect()
}

/// The window to bring forward for `target`: the saved one if it still exists, else the
/// frontmost window of the same executable.
fn window_for(target: &AppRef) -> Option<HWND> {
    let saved = target
        .window
        .and_then(handle_of)
        // SAFETY: IsWindow takes the handle by value and answers 0 for one that is gone.
        .filter(|hwnd| unsafe { IsWindow(*hwnd) } != 0);
    saved.or_else(|| {
        app_windows()
            .into_iter()
            .find(|app| app.id.eq_ignore_ascii_case(&target.id))
            .and_then(|app| app.window)
            .and_then(handle_of)
    })
}

fn in_front(hwnd: HWND) -> bool {
    // SAFETY: no arguments; returns null when nothing is in front.
    unsafe { GetForegroundWindow() == hwnd }
}

/// One mouse event that moves nothing and presses nothing. Its only effect is that this
/// process counts as having received the last input, which is the condition Windows sets for
/// `SetForegroundWindow`.
fn empty_mouse_event() {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: 0,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let size = i32::try_from(size_of::<INPUT>()).unwrap_or(0);
    // SAFETY: `input` is one valid INPUT and `size` is its size, as the call requires.
    unsafe { SendInput(1, &raw const input, size) };
}

impl Platform for WinPlatform {
    fn frontmost(&self) -> Result<Option<AppRef>> {
        // SAFETY: no arguments; returns null when nothing is in front, as during a switch.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return Ok(None);
        }
        Ok(app_of(hwnd))
    }

    fn watch_frontmost(&self, _tx: Sender<Option<AppRef>>) {
        // Not implemented: the shell polls `frontmost`, as on macOS.
    }

    fn running_apps(&self) -> Result<Vec<AppRef>> {
        Ok(app_windows())
    }

    fn seconds_since_input(&self) -> Result<f64> {
        let mut info = LASTINPUTINFO {
            cbSize: u32::try_from(size_of::<LASTINPUTINFO>()).unwrap_or(0),
            dwTime: 0,
        };
        // SAFETY: `info` is a valid LASTINPUTINFO with its size set, as the call requires.
        if unsafe { GetLastInputInfo(&raw mut info) } == 0 {
            return Err(PlatformError::Os("GetLastInputInfo failed".to_owned()));
        }
        // SAFETY: no arguments. Both values are milliseconds since boot and wrap together.
        let now = unsafe { GetTickCount() };
        Ok(f64::from(now.wrapping_sub(info.dwTime)) / 1000.0)
    }

    fn activate(&self, target: &AppRef) -> Result<()> {
        let hwnd = window_for(target).ok_or(PlatformError::NotRunning)?;
        // SAFETY: both calls take the handle by value; a window that has just closed makes
        // them fail harmlessly, and the core verifies the switch afterwards.
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
        if !in_front(hwnd) && self.force_foreground.get() {
            empty_mouse_event();
            // SAFETY: as above.
            unsafe { SetForegroundWindow(hwnd) };
        }
        Ok(())
    }

    fn play_sound(&self, _sound: Sound) {
        // SAFETY: plain value in; plays the system's "asterisk" sound without blocking.
        unsafe { MessageBeep(MB_ICONASTERISK) };
    }
}

impl Overlay for WinPlatform {
    fn show_toast(&self, text: &str) {
        self.overlay.show_toast(text);
    }

    fn hide_toast(&self) {
        self.overlay.hide_toast();
    }

    fn show_recap(&self, lines: &[String], for_how_long: Duration) {
        self.overlay.show_recap(lines, for_how_long);
    }

    fn tick(&self, now: Instant) {
        self.pump();
        self.overlay.tick(now);
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.overlay.next_deadline()
    }
}

const ACTIONS: [HotkeyAction; 3] = [
    HotkeyAction::Focus,
    HotkeyAction::TogglePause,
    HotkeyAction::DeclareLong,
];

/// The id each action is registered under with the OS.
fn hotkey_id(action: HotkeyAction) -> i32 {
    match action {
        HotkeyAction::Focus => 1,
        HotkeyAction::TogglePause => 2,
        HotkeyAction::DeclareLong => 3,
    }
}

/// The action for the id a `WM_HOTKEY` message carries.
fn action_of(id: usize) -> Option<HotkeyAction> {
    ACTIONS
        .into_iter()
        .find(|action| usize::try_from(hotkey_id(*action)).is_ok_and(|n| n == id))
}

fn register_chord(chord: &hotkey::Chord, id: i32) -> std::result::Result<(), String> {
    let vk = hotkey::win_vk(&chord.key).ok_or("that key has no Windows key code")?;
    let mut modifiers = MOD_NOREPEAT;
    for (on, flag) in [
        (chord.ctrl, MOD_CONTROL),
        (chord.alt, MOD_ALT),
        (chord.shift, MOD_SHIFT),
        (chord.cmd, MOD_WIN),
    ] {
        if on {
            modifiers |= flag;
        }
    }
    // SAFETY: a null window handle ties the hotkey to this thread, whose queue `pump` reads.
    // Unregistering an id that isn't registered fails harmlessly.
    let ok = unsafe {
        UnregisterHotKey(std::ptr::null_mut(), id);
        RegisterHotKey(std::ptr::null_mut(), id, modifiers, vk)
    };
    if ok != 0 {
        return Ok(());
    }
    // SAFETY: no arguments; reads this thread's last error code.
    let code = unsafe { GetLastError() };
    Err(if code == ERROR_HOTKEY_ALREADY_REGISTERED {
        "another program already uses this key combination".to_owned()
    } else {
        format!("Windows refused it (error {code})")
    })
}

impl Hotkeys for WinPlatform {
    fn register(&self, keys: &hotkey::Keys<'_>) -> Vec<String> {
        let mut problems = Vec::new();
        for (text, action, name) in keys.each() {
            match hotkey::parse(text) {
                Ok(None) => {}
                Ok(Some(chord)) => match register_chord(&chord, hotkey_id(action)) {
                    Ok(()) => self.hotkeys_on.set(true),
                    Err(e) => problems.push(format!("{name} hotkey `{text}`: {e}")),
                },
                Err(e) => problems.push(format!("{name} hotkey: {e}")),
            }
        }
        problems
    }

    fn take_pressed(&self) -> Vec<HotkeyAction> {
        std::mem::take(&mut *self.pressed.borrow_mut())
    }

    fn active(&self) -> bool {
        self.hotkeys_on.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_names_read_without_their_extension() {
        assert_eq!(display_name("WindowsTerminal.exe"), "WindowsTerminal");
        assert_eq!(display_name("Code.EXE"), "Code");
        assert_eq!(display_name("odd"), "odd");
        assert_eq!(display_name(".exe"), "");
    }

    #[test]
    fn window_handles_survive_the_round_trip_through_a_number() {
        let hwnd = handle_of(0x1234).unwrap();
        assert_eq!(handle_number(hwnd), Some(0x1234));
    }

    #[test]
    fn hotkey_ids_map_back_to_their_actions() {
        for action in ACTIONS {
            let id = usize::try_from(hotkey_id(action)).unwrap();
            assert_eq!(action_of(id), Some(action));
        }
        assert_eq!(action_of(99), None);
    }

    /// These run against the real OS on a Windows machine: they prove the calls link and
    /// return, not that a person sees the right thing.
    #[test]
    fn the_os_answers_the_read_only_questions() {
        let platform = WinPlatform::new();
        let idle = platform.seconds_since_input().unwrap();
        assert!(idle >= 0.0, "idle time {idle}");
        // No desktop window may be in front on a build machine: both answers are fine.
        let front = platform.frontmost().unwrap();
        let apps = platform.running_apps().unwrap();
        println!(
            "front: {front:?}; {} app windows; idle {idle} s",
            apps.len()
        );
        for app in &apps {
            assert!(!app.id.is_empty());
            assert!(app.window.is_some());
        }
        // This process is running, so its own executable can be named.
        let own = exe_of(std::process::id()).unwrap();
        assert!(own.to_ascii_lowercase().ends_with(".exe"), "{own}");
    }

    #[test]
    fn activating_an_app_that_is_not_running_is_an_error() {
        let platform = WinPlatform::new();
        let gone = AppRef {
            id: "no-such-program-here.exe".to_owned(),
            name: "none".to_owned(),
            pid: 0,
            window: None,
        };
        assert_eq!(platform.activate(&gone), Err(PlatformError::NotRunning));
    }

    #[test]
    fn a_hotkey_registers_and_a_second_owner_is_refused_with_a_reason() {
        let first = WinPlatform::new();
        let keys = hotkey::Keys {
            focus: "Ctrl+Alt+Shift+Y",
            pause: "",
            declare_long: "",
        };
        let problems = first.register(&keys);
        // A machine with no interactive desktop may refuse every hotkey; that is not what
        // this checks.
        if !problems.is_empty() {
            println!("hotkeys unavailable here: {problems:?}");
            return;
        }
        assert!(first.active());
        // The same chord from another thread is another owner.
        let second = std::thread::spawn(move || {
            let other = WinPlatform::new();
            other.register(&hotkey::Keys {
                focus: "Ctrl+Alt+Shift+Y",
                pause: "",
                declare_long: "",
            })
        })
        .join()
        .unwrap();
        assert_eq!(second.len(), 1, "{second:?}");
        assert!(second[0].contains("already uses"), "{second:?}");
    }
}
