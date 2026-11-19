// SPDX-License-Identifier: Apache-2.0
//! Keeping a background child from holding on to this process's pipes.

/// Stops a child started from now on from inheriting this process's own standard handles.
///
/// On Windows `CreateProcess` hands a child every inheritable handle of its parent, not only
/// the three it is told to use. When `focusctl setup` runs with its output piped (`| cat`,
/// `2>&1 | Tee-Object`), the pipe's write end is one of them, so the background `focusd` kept
/// it open and the reader never saw the end of the output. Clearing the inherit flag on this
/// process's stdin, stdout and stderr fixes that; the process can still use them. A child's
/// own stdio, set through `std::process::Command`, is duplicated as inheritable separately and
/// is not affected. On other systems file descriptors close on exec, so this does nothing.
#[cfg(windows)]
pub fn keep_std_handles_private() {
    use windows_sys::Win32::Foundation::{
        HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: GetStdHandle only reads this process's own standard handle. The handle is
        // used only when it is neither null (no such handle) nor INVALID_HANDLE_VALUE, and
        // SetHandleInformation then clears one flag on it without closing or moving it.
        unsafe {
            let handle = GetStdHandle(which);
            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// See the Windows version.
#[cfg(not(windows))]
pub fn keep_std_handles_private() {}
