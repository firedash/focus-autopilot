// SPDX-License-Identifier: Apache-2.0
//! Global hotkeys through Carbon's `RegisterEventHotKey`. It needs no permission (unlike
//! watching key events, which needs Accessibility) and never sees other keystrokes: the OS
//! reports only that a registered chord was pressed.
//!
//! This is the only code in the project that declares foreign functions by hand: the
//! `objc2` crates have no bindings for Carbon's hotkey calls.
//!
//! Status: type-checked from Linux only; it has not run on a Mac.

use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::hotkey::{Chord, mac_keycode};

type OsStatus = i32;
type EventHandlerFn = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> OsStatus;

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

/// `'keyb'`
const EVENT_CLASS_KEYBOARD: u32 = 0x6B65_7962;
/// `kEventHotKeyPressed`
const EVENT_HOT_KEY_PRESSED: u32 = 5;
/// `'----'`, `kEventParamDirectObject`
const PARAM_DIRECT_OBJECT: u32 = 0x2D2D_2D2D;
/// `'hkid'`, `typeEventHotKeyID`
const TYPE_EVENT_HOT_KEY_ID: u32 = 0x686B_6964;
/// `'fcap'`: identifies this app's hotkeys.
const SIGNATURE: u32 = 0x6663_6170;

const CMD_KEY: u32 = 1 << 8;
const SHIFT_KEY: u32 = 1 << 9;
const OPTION_KEY: u32 = 1 << 11;
const CONTROL_KEY: u32 = 1 << 12;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: EventHandlerFn,
        num_types: u32,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut *mut c_void,
    ) -> OsStatus;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: *mut c_void,
        options: u32,
        out_ref: *mut *mut c_void,
    ) -> OsStatus;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        buffer_size: u32,
        actual_size: *mut u32,
        data: *mut c_void,
    ) -> OsStatus;
}

/// Ids of hotkeys pressed since the last `take`.
static PRESSED: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static HANDLER_INSTALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_hot_key(_next: *mut c_void, event: *mut c_void, _user: *mut c_void) -> OsStatus {
    let mut id = EventHotKeyId {
        signature: 0,
        id: 0,
    };
    // SAFETY: `event` is the live event Carbon passed to this handler, and `id` is a valid,
    // writable `EventHotKeyID` whose size is passed as the buffer size.
    let status = unsafe {
        GetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOT_KEY_ID,
            ptr::null_mut(),
            u32::try_from(size_of::<EventHotKeyId>()).unwrap_or(0),
            ptr::null_mut(),
            ptr::from_mut(&mut id).cast(),
        )
    };
    if status == 0
        && id.signature == SIGNATURE
        && let Ok(mut pressed) = PRESSED.lock()
    {
        pressed.push(id.id);
    }
    0
}

fn install_handler() -> Result<(), String> {
    if HANDLER_INSTALLED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let spec = EventTypeSpec {
        event_class: EVENT_CLASS_KEYBOARD,
        event_kind: EVENT_HOT_KEY_PRESSED,
    };
    let mut handler_ref: *mut c_void = ptr::null_mut();
    // SAFETY: the target is the application event target, `on_hot_key` has the signature
    // Carbon expects and lives for the whole process, `spec` outlives the call, and
    // `handler_ref` is a valid out pointer.
    let status = unsafe {
        InstallEventHandler(
            GetApplicationEventTarget(),
            on_hot_key,
            1,
            &raw const spec,
            ptr::null_mut(),
            &raw mut handler_ref,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        HANDLER_INSTALLED.store(false, Ordering::SeqCst);
        Err(format!(
            "could not install the hotkey handler (OS error {status})"
        ))
    }
}

/// Registers one chord under `id`.
///
/// # Errors
/// When the key is unknown or the OS refuses (another app already owns the chord).
pub fn register(chord: &Chord, id: u32) -> Result<(), String> {
    install_handler()?;
    let key_code = mac_keycode(&chord.key).ok_or("unknown key")?;
    let mut modifiers = 0;
    for (on, flag) in [
        (chord.cmd, CMD_KEY),
        (chord.shift, SHIFT_KEY),
        (chord.alt, OPTION_KEY),
        (chord.ctrl, CONTROL_KEY),
    ] {
        if on {
            modifiers |= flag;
        }
    }
    let mut hot_key_ref: *mut c_void = ptr::null_mut();
    // SAFETY: valid key code and modifier flags, an id struct passed by value, the application
    // event target, and a valid out pointer for the registration reference.
    let status = unsafe {
        RegisterEventHotKey(
            key_code,
            modifiers,
            EventHotKeyId {
                signature: SIGNATURE,
                id,
            },
            GetApplicationEventTarget(),
            0,
            &raw mut hot_key_ref,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "the OS refused the hotkey (error {status}); another app may use it"
        ))
    }
}

/// Ids pressed since the last call, oldest first.
pub fn take() -> Vec<u32> {
    PRESSED
        .lock()
        .map(|mut pressed| std::mem::take(&mut *pressed))
        .unwrap_or_default()
}
