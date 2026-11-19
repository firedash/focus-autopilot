// SPDX-License-Identifier: Apache-2.0
//! Starting a program in the background so that it outlives its starter, holds on to nothing
//! of the starter's, and writes to a log.
//!
//! On Windows `CreateProcess` hands a child every inheritable handle its parent has, not only
//! the three it is told to use. When `focusctl setup` runs with its output read through a pipe
//! (`| cat`, `2>&1 | Tee-Object`, or `powershell -c ... | cat`, where the outer pipe is a handle
//! `focusctl` itself inherited and is none of its standard handles), the background `focusd`
//! kept such a pipe open and the reader never saw the end of the output. So the child here is
//! started with a handle list: the only handles it inherits are the two it needs, the log and
//! `NUL`.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// A program started by [`spawn_background`]. Dropping it leaves the program running.
pub struct Background(imp::Child);

impl Background {
    /// Waits for the program to end and says whether it ended successfully.
    ///
    /// # Errors
    /// When the wait itself fails.
    pub fn wait(&mut self) -> io::Result<bool> {
        self.0.wait()
    }
}

/// Starts `program` with `args`, with no stdin, its stdout and stderr appended to `log`, no
/// console, its own process group, and none of this process's other handles.
///
/// # Errors
/// When the log can't be opened or the program can't be started.
pub fn spawn_background(program: &Path, args: &[&str], log: &Path) -> io::Result<Background> {
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let log = OpenOptions::new().create(true).append(true).open(log)?;
    imp::spawn(program, args, &log).map(Background)
}

#[cfg(not(windows))]
mod imp {
    use super::{File, io};
    use std::path::Path;
    use std::process::{Command, Stdio};

    pub struct Child(std::process::Child);

    impl Child {
        pub fn wait(&mut self) -> io::Result<bool> {
            self.0.wait().map(|status| status.success())
        }
    }

    pub fn spawn(program: &Path, args: &[&str], log: &File) -> io::Result<Child> {
        use std::os::unix::process::CommandExt as _;
        // File descriptors other than 0, 1 and 2 close on exec, so nothing else is inherited.
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log.try_clone()?)
            .process_group(0)
            .spawn()
            .map(Child)
    }
}

#[cfg(windows)]
mod imp {
    use super::{File, io};
    use std::ffi::{OsStr, OsString, c_void};
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use std::path::Path;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, DeleteProcThreadAttributeList,
        EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE,
        InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
        STARTUPINFOEXW, UpdateProcThreadAttribute, WaitForSingleObject,
    };

    /// The program's process handle, closed when this is dropped.
    pub struct Child(HANDLE);

    impl Child {
        pub fn wait(&mut self) -> io::Result<bool> {
            // SAFETY: `self.0` is the process handle CreateProcessW returned and is still open;
            // an infinite wait on it has no other requirement.
            if unsafe { WaitForSingleObject(self.0, INFINITE) } != WAIT_OBJECT_0 {
                return Err(io::Error::last_os_error());
            }
            let mut code = 0_u32;
            // SAFETY: the same open handle, and `code` is a valid place to write the result.
            if unsafe { GetExitCodeProcess(self.0, &raw mut code) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(code == 0)
        }
    }

    impl Drop for Child {
        fn drop(&mut self) {
            // SAFETY: the handle is owned by this value and closed only here.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// One argument as the Windows command-line parser reads it back: quoted when it holds a
    /// space, tab or quote, with backslashes before a quote doubled.
    pub fn quote_arg(arg: &OsStr) -> Vec<u16> {
        let wide: Vec<u16> = arg.encode_wide().collect();
        let plain = !wide.is_empty()
            && !wide
                .iter()
                .any(|&c| c == u16::from(b' ') || c == u16::from(b'\t') || c == u16::from(b'"'));
        if plain {
            return wide;
        }
        let mut out = vec![u16::from(b'"')];
        let mut backslashes = 0_usize;
        for &c in &wide {
            if c == u16::from(b'\\') {
                backslashes += 1;
                out.push(c);
                continue;
            }
            if c == u16::from(b'"') {
                // Double the backslashes before it, then escape the quote itself.
                out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes + 1));
            }
            backslashes = 0;
            out.push(c);
        }
        // Backslashes before the closing quote must not escape it.
        out.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes));
        out.push(u16::from(b'"'));
        out
    }

    /// The command line for `program` and `args`, as UTF-16 with a final NUL.
    pub fn command_line(program: &Path, args: &[&str]) -> Vec<u16> {
        let mut line = Vec::new();
        let parts =
            std::iter::once(program.as_os_str().to_owned()).chain(args.iter().map(OsString::from));
        for (i, part) in parts.enumerate() {
            if i > 0 {
                line.push(u16::from(b' '));
            }
            line.extend(quote_arg(&part));
        }
        line.push(0);
        line
    }

    /// Makes a handle inheritable, which a handle list requires of every handle on it.
    fn make_inheritable(file: &File) -> io::Result<HANDLE> {
        let handle = file.as_raw_handle();
        // SAFETY: `handle` is open for as long as `file` is, and only one flag on it changes.
        if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(handle)
    }

    pub fn spawn(program: &Path, args: &[&str], log: &File) -> io::Result<Child> {
        let nul = File::open("NUL")?;
        let nul_handle = make_inheritable(&nul)?;
        let log_handle = make_inheritable(log)?;
        // Every entry on a handle list must be unique; stdout and stderr share the log.
        let mut inherited: [HANDLE; 2] = [nul_handle, log_handle];

        // An attribute list holding the handle list, in a buffer the size Windows asks for.
        let mut size = 0_usize;
        // SAFETY: a null list with a size out-parameter only asks how large the list must be;
        // it fails with ERROR_INSUFFICIENT_BUFFER by design, so the result is not checked.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &raw mut size) };
        let mut buffer = vec![0_u8; size];
        let list: LPPROC_THREAD_ATTRIBUTE_LIST = buffer.as_mut_ptr().cast::<c_void>();
        // SAFETY: `list` points to `size` writable bytes, as asked for just above.
        if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &raw mut size) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `list` is initialised; `inherited` is an array of open, inheritable handles
        // that outlives the CreateProcessW call below; the attribute's value is copied by size.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                inherited.as_mut_ptr().cast::<c_void>(),
                std::mem::size_of_val(&inherited),
                null_mut(),
                null(),
            )
        };
        if updated == 0 {
            let error = io::Error::last_os_error();
            // SAFETY: `list` was initialised above and is deleted once.
            unsafe { DeleteProcThreadAttributeList(list) };
            return Err(error);
        }

        // SAFETY: STARTUPINFOEXW is plain data for which all zeroes is a valid empty value.
        let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        startup.StartupInfo.cb = u32::try_from(std::mem::size_of::<STARTUPINFOEXW>()).unwrap_or(0);
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = nul_handle;
        startup.StartupInfo.hStdOutput = log_handle;
        startup.StartupInfo.hStdError = log_handle;
        startup.lpAttributeList = list;

        let mut line = command_line(program, args);
        // SAFETY: PROCESS_INFORMATION is plain data; CreateProcessW fills it in.
        let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `line` is a writable, NUL-terminated UTF-16 command line; `startup` is a valid
        // STARTUPINFOEXW whose size and attribute list are set, as EXTENDED_STARTUPINFO_PRESENT
        // says; the handles it names are open and inheritable and on the handle list; the other
        // pointers are null, meaning defaults; `info` is a valid place for the result.
        let created = unsafe {
            CreateProcessW(
                null(),
                line.as_mut_ptr(),
                null(),
                null(),
                1,
                EXTENDED_STARTUPINFO_PRESENT | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
                null(),
                null(),
                &raw const startup.StartupInfo,
                &raw mut info,
            )
        };
        let error = io::Error::last_os_error();
        // SAFETY: `list` was initialised above and is deleted once, after its last use.
        unsafe { DeleteProcThreadAttributeList(list) };
        if created == 0 {
            return Err(error);
        }
        // SAFETY: the thread handle is ours to close and is not used again.
        unsafe { CloseHandle(info.hThread) };
        Ok(Child(info.hProcess))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn quoted(text: &str) -> String {
            String::from_utf16(&quote_arg(OsStr::new(text))).unwrap()
        }

        #[test]
        fn arguments_are_quoted_the_way_windows_reads_them_back() {
            assert_eq!(quoted("--config"), "--config");
            assert_eq!(quoted(""), "\"\"");
            assert_eq!(quoted("C:\\a b\\c.toml"), "\"C:\\a b\\c.toml\"");
            assert_eq!(quoted("say \"hi\""), "\"say \\\"hi\\\"\"");
            // Backslashes before the closing quote are doubled so they do not escape it.
            assert_eq!(quoted("C:\\a b\\"), "\"C:\\a b\\\\\"");
        }

        #[test]
        fn the_command_line_starts_with_the_quoted_program() {
            let line = command_line(
                Path::new("C:\\Program Files\\x\\focusd.exe"),
                &["--config", "C:\\d d\\c.toml"],
            );
            assert_eq!(line.last(), Some(&0));
            let text = String::from_utf16(&line[..line.len() - 1]).unwrap();
            assert_eq!(
                text,
                "\"C:\\Program Files\\x\\focusd.exe\" --config \"C:\\d d\\c.toml\""
            );
        }
    }
}
