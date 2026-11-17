// SPDX-License-Identifier: Apache-2.0
//! The Windows toast and recap card: two always-on-top popup windows that never take the
//! keyboard focus and let every click through to whatever is underneath (design section 8).
//!
//! Status: type-checked from Linux and built and tested on a Windows CI runner. It has not
//! been looked at by a person on a Windows desktop.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DT_CALCRECT,
    DT_NOPREFIX, DT_WORDBREAK, DeleteObject, DrawTextW, EndPaint, FW_NORMAL, FillRect, GetDC,
    GetMonitorInfoW, HFONT, InvalidateRect, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromWindow, PAINTSTRUCT, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
    UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetForegroundWindow, HTTRANSPARENT,
    HWND_TOPMOST, LWA_ALPHA, MA_NOACTIVATE, RegisterClassW, SW_HIDE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetProcessDPIAware, SetWindowPos, ShowWindow,
    WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// Sizes in pixels at 100% scaling; they grow with the system's scaling setting.
const TOAST_WIDTH: i32 = 460;
const TOAST_MIN_HEIGHT: i32 = 64;
const RECAP_WIDTH: i32 = 520;
const RECAP_MIN_HEIGHT: i32 = 150;
const PAD: i32 = 14;
/// Distance from the top of the screen's work area.
const TOP_MARGIN: i32 = 48;
/// Blue, green, red: a near-black with a little blue, like the macOS panels.
const BACKGROUND: u32 = 0x001C_1717;
const TEXT_COLOUR: u32 = 0x00FF_FFFF;
/// Out of 255: slightly see-through.
const OPACITY: u8 = 240;
const STANDARD_DPI: i32 = 96;

/// What a window needs in order to paint itself.
struct Painted {
    /// UTF-16, no terminator: drawn by length.
    text: Vec<u16>,
    font: HFONT,
}

thread_local! {
    /// What each overlay window shows, keyed by its handle as a number. The windows live on
    /// one thread and are painted by it, so the window procedure reads this without a lock.
    static PAINTED: RefCell<HashMap<usize, Painted>> = RefCell::new(HashMap::new());
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// A null-terminated UTF-16 string, as window class and font names must be.
fn wide_z(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn scaled(pixels: i32, dpi: i32) -> i32 {
    pixels.saturating_mul(dpi) / STANDARD_DPI
}

/// The rectangle inside `client` that the text is drawn in.
fn text_box(client: RECT, pad: i32) -> RECT {
    RECT {
        left: client.left + pad,
        top: client.top + pad,
        right: (client.right - pad).max(client.left + pad),
        bottom: (client.bottom - pad).max(client.top + pad),
    }
}

/// Fills the window and draws its text. Called for `WM_PAINT`.
fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT {
        hdc: std::ptr::null_mut(),
        fErase: 0,
        rcPaint: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        fRestore: 0,
        fIncUpdate: 0,
        rgbReserved: [0; 32],
    };
    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: `hwnd` is the live window this message was sent to; `ps` and `client` are
    // valid out values. BeginPaint returns a device context that stays valid until the
    // EndPaint at the bottom of this function, and every GDI object made here is released
    // here.
    unsafe {
        let hdc = BeginPaint(hwnd, &raw mut ps);
        if hdc.is_null() {
            return;
        }
        GetClientRect(hwnd, &raw mut client);
        let brush = CreateSolidBrush(BACKGROUND);
        FillRect(hdc, &raw const client, brush);
        DeleteObject(brush);
        PAINTED.with_borrow(|painted| {
            let Some(shown) = painted.get(&hwnd.addr()) else {
                return;
            };
            SetBkMode(hdc, TRANSPARENT.cast_signed());
            SetTextColor(hdc, TEXT_COLOUR);
            let before = SelectObject(hdc, shown.font);
            let mut area = text_box(client, PAD);
            let len = i32::try_from(shown.text.len()).unwrap_or(0);
            DrawTextW(
                hdc,
                shown.text.as_ptr(),
                len,
                &raw mut area,
                DT_WORDBREAK | DT_NOPREFIX,
            );
            SelectObject(hdc, before);
        });
        EndPaint(hwnd, &raw const ps);
    }
}

/// The window procedure for both overlay windows.
unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_PAINT => {
            paint(hwnd);
            0
        }
        // The whole window is painted in WM_PAINT; erasing first would only flicker.
        WM_ERASEBKGND => 1,
        // "Not here": the click goes to the window underneath.
        WM_NCHITTEST => isize::try_from(HTTRANSPARENT).unwrap_or(-1),
        // And should a click arrive anyway, it must not make this window the active one.
        WM_MOUSEACTIVATE => isize::try_from(MA_NOACTIVATE).unwrap_or(3),
        // SAFETY: the arguments are passed through exactly as Windows gave them.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

struct Panel {
    hwnd: HWND,
    font: HFONT,
    width: i32,
    min_height: i32,
    dpi: i32,
}

impl Panel {
    /// A null `hwnd` means the window could not be made; every method then does nothing.
    fn new(class: &[u16], width: i32, min_height: i32, font_points: i32, dpi: i32) -> Self {
        let face = wide_z("Segoe UI");
        // SAFETY: `class` and `face` are null-terminated UTF-16 strings that outlive the
        // calls; every other argument is a plain value or null. A negative height asks for
        // that character height in pixels.
        let (hwnd, font) = unsafe {
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT,
                class.as_ptr(),
                std::ptr::null(),
                WS_POPUP,
                0,
                0,
                scaled(width, dpi),
                scaled(min_height, dpi),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            );
            if !hwnd.is_null() {
                SetLayeredWindowAttributes(hwnd, 0, OPACITY, LWA_ALPHA);
            }
            let font = CreateFontW(
                -(font_points.saturating_mul(dpi) / 72),
                0,
                0,
                0,
                FW_NORMAL.cast_signed(),
                0,
                0,
                0,
                u32::from(DEFAULT_CHARSET),
                0,
                0,
                u32::from(CLEARTYPE_QUALITY),
                0,
                face.as_ptr(),
            );
            (hwnd, font)
        };
        Self {
            hwnd,
            font,
            width: scaled(width, dpi),
            min_height: scaled(min_height, dpi),
            dpi,
        }
    }

    /// How tall the window must be for `text` once wrapped to its width.
    fn height_for(&self, text: &[u16]) -> i32 {
        let pad = scaled(PAD, self.dpi);
        let mut area = RECT {
            left: 0,
            top: 0,
            right: (self.width - 2 * pad).max(1),
            bottom: 0,
        };
        let len = i32::try_from(text.len()).unwrap_or(0);
        // SAFETY: `self.hwnd` is this panel's live window, `text` is valid for `len` units,
        // and `area` is a valid in/out rectangle. With DT_CALCRECT nothing is drawn. The
        // device context is released and the previous font put back before returning.
        unsafe {
            let hdc = GetDC(self.hwnd);
            if hdc.is_null() {
                return self.min_height;
            }
            let before = SelectObject(hdc, self.font);
            DrawTextW(
                hdc,
                text.as_ptr(),
                len,
                &raw mut area,
                DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX,
            );
            SelectObject(hdc, before);
            ReleaseDC(self.hwnd, hdc);
        }
        (area.bottom + 2 * pad).max(self.min_height)
    }

    /// The work area (the screen minus the taskbar) of the monitor the user is looking at.
    fn work_area() -> RECT {
        let mut info = MONITORINFO {
            cbSize: u32::try_from(size_of::<MONITORINFO>()).unwrap_or(0),
            rcMonitor: RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            rcWork: RECT {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 720,
            },
            dwFlags: 0,
        };
        // SAFETY: the foreground handle may be null, which falls back to the primary
        // monitor; `info` is a valid MONITORINFO with its size set.
        unsafe {
            let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
            GetMonitorInfoW(monitor, &raw mut info);
        }
        info.rcWork
    }

    fn show(&self, text: &str) {
        if self.hwnd.is_null() {
            return;
        }
        let text = wide(text);
        let height = self.height_for(&text);
        PAINTED.with_borrow_mut(|painted| {
            painted.insert(
                self.hwnd.addr(),
                Painted {
                    text,
                    font: self.font,
                },
            );
        });
        let area = Self::work_area();
        let x = area.left + (area.right - area.left - self.width) / 2;
        let y = area.top + scaled(TOP_MARGIN, self.dpi);
        // SAFETY: `self.hwnd` is this panel's live window. SWP_NOACTIVATE shows it without
        // making it the active window; UpdateWindow paints it now, on this thread.
        unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                x,
                y,
                self.width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            InvalidateRect(self.hwnd, std::ptr::null(), 1);
            UpdateWindow(self.hwnd);
        }
    }

    fn hide(&self) {
        if self.hwnd.is_null() {
            return;
        }
        // SAFETY: `self.hwnd` is this panel's live window.
        unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }

    #[cfg(test)]
    fn visible(&self) -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        // SAFETY: takes the handle by value; answers 0 for a null or stale one.
        !self.hwnd.is_null() && unsafe { IsWindowVisible(self.hwnd) } != 0
    }
}

pub struct WinOverlay {
    toast: Panel,
    recap: Panel,
    recap_until: Cell<Option<Instant>>,
}

impl WinOverlay {
    pub fn new() -> Self {
        let class = wide_z("FocusAutopilotOverlay");
        // SAFETY: SetProcessDPIAware and GetDpiForSystem take no arguments. `class` is a
        // null-terminated name that outlives RegisterClassW, and `overlay_proc` has the
        // signature a window procedure must have. Registering the class a second time (a
        // second overlay in one process) fails harmlessly: the first registration stands.
        let dpi = unsafe {
            SetProcessDPIAware();
            let class_info = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(overlay_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: GetModuleHandleW(std::ptr::null()),
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            RegisterClassW(&raw const class_info);
            i32::try_from(GetDpiForSystem()).unwrap_or(STANDARD_DPI)
        };
        let dpi = dpi.max(STANDARD_DPI);
        Self {
            toast: Panel::new(&class, TOAST_WIDTH, TOAST_MIN_HEIGHT, 11, dpi),
            recap: Panel::new(&class, RECAP_WIDTH, RECAP_MIN_HEIGHT, 10, dpi),
            recap_until: Cell::new(None),
        }
    }

    pub fn show_toast(&self, text: &str) {
        self.toast.show(text);
    }

    pub fn hide_toast(&self) {
        self.toast.hide();
    }

    pub fn show_recap(&self, lines: &[String], for_how_long: Duration) {
        self.recap.show(&lines.join("\n"));
        self.recap_until.set(Some(Instant::now() + for_how_long));
    }

    pub fn tick(&self, now: Instant) {
        if self.recap_until.get().is_some_and(|until| now >= until) {
            self.recap.hide();
            self.recap_until.set(None);
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.recap_until.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_grow_with_the_scaling_setting() {
        assert_eq!(scaled(460, 96), 460);
        assert_eq!(scaled(460, 144), 690);
        assert_eq!(scaled(14, 120), 17);
    }

    #[test]
    fn the_text_box_is_inset_and_never_inside_out() {
        let client = RECT {
            left: 0,
            top: 0,
            right: 100,
            bottom: 40,
        };
        let inner = text_box(client, 14);
        assert_eq!(
            (inner.left, inner.top, inner.right, inner.bottom),
            (14, 14, 86, 26)
        );
        let tiny = text_box(
            RECT {
                left: 0,
                top: 0,
                right: 10,
                bottom: 10,
            },
            14,
        );
        assert!(tiny.right >= tiny.left && tiny.bottom >= tiny.top);
    }

    /// Against the real OS: the windows can be made, shown without becoming the foreground
    /// window, grow for long text, and hide again.
    #[test]
    fn the_boxes_show_and_hide_without_taking_the_foreground() {
        let overlay = WinOverlay::new();
        if overlay.toast.hwnd.is_null() {
            println!("no window could be created here: skipping");
            return;
        }
        overlay.show_toast("Claude is done in api. Back to Terminal in 5 s.");
        assert!(overlay.toast.visible());
        // SAFETY: no arguments.
        let front = unsafe { GetForegroundWindow() };
        assert_ne!(front, overlay.toast.hwnd, "the toast took the foreground");
        let short = overlay.toast.height_for(&wide("Short."));
        let long = overlay
            .toast
            .height_for(&wide(&"A long sentence. ".repeat(40)));
        assert!(long > short, "{long} vs {short}");
        overlay.hide_toast();
        assert!(!overlay.toast.visible());

        let lines = vec!["Project: api".to_owned(), "Asked:   Add retry".to_owned()];
        overlay.show_recap(&lines, Duration::from_millis(10));
        assert!(overlay.recap.visible());
        assert!(overlay.next_deadline().is_some());
        overlay.tick(Instant::now() + Duration::from_secs(1));
        assert!(!overlay.recap.visible());
        assert_eq!(overlay.next_deadline(), None);
    }
}
