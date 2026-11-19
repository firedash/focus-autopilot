// SPDX-License-Identifier: Apache-2.0
//! `focusctl setup`: the three first-run steps in one command. Install the hooks, start `focusd`
//! in the background if it is not already running, run the doctor's checks, and print one
//! result with the next step. Running it again changes nothing: the hooks merge is idempotent
//! and a `focusd` that already answers is left alone.
//!
//! The one thing it starts is `focusd`, from the same folder as `focusctl` or from the PATH,
//! with its output in `focusd.log` beside `config.toml`. It does not set up start at login.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::doctor::{self, Check, Level};
use crate::{config, files};

/// How long to wait for a freshly started `focusd` to answer.
pub const START_WAIT: Duration = Duration::from_secs(8);

/// The `focusd` that goes with this `focusctl`: the one in the same folder, else the first on
/// the PATH.
pub fn find_focusd(exe_dir: Option<&Path>, path_env: &str) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "focusd.exe"
    } else {
        "focusd"
    };
    exe_dir
        .map(Path::to_path_buf)
        .into_iter()
        .chain(std::env::split_paths(path_env))
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Starts `program` so that it outlives this command and the terminal's Ctrl+C: no stdin, its
/// output appended to `log`, and its own process group (on Windows, detached with no console).
///
/// # Errors
/// When the log can't be opened or the program can't be started.
pub fn spawn_detached(program: &Path, args: &[&str], log: &Path) -> Result<Child, String> {
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| format!("{}: {e}", log.display()))?;
    let err = out
        .try_clone()
        .map_err(|e| format!("{}: {e}", log.display()))?;
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        command.creation_flags(0x0000_0208);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", program.display()))
}

/// Polls until `focusd` answers on `port`, or `wait` runs out.
fn wait_until_listening(port: u16, token: &str, wait: Duration) -> bool {
    let end = Instant::now() + wait;
    loop {
        if doctor::is_listening(port, token) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// What the command decided, for the caller to print and turn into an exit code.
#[derive(Debug, PartialEq, Eq)]
pub struct SetupResult {
    pub ready: bool,
    pub summary: String,
}

/// Runs the steps, reporting each through `say`. `start` is how `focusd` gets started: it
/// is called only when nothing is listening on the port, and returns a line to show.
///
/// # Errors
/// Only when the hooks or `config.toml` can't be written; every other problem is a failed
/// check in the result.
pub fn run(
    settings: &Path,
    config_path: &Path,
    start: &mut dyn FnMut() -> Result<String, String>,
    wait: Duration,
    say: &mut dyn FnMut(&str),
) -> Result<SetupResult, String> {
    // 1. Hooks, and the token they carry.
    let receiver = config::ensure_token(config_path, false)?;
    let outcome = files::apply(
        settings,
        &files::Op::Install {
            port: receiver.port,
            token: receiver.token.clone(),
        },
        false,
    )?;
    let hooks_changed = outcome.changed();
    say(&if hooks_changed {
        format!("1/3 hooks: added to {}", outcome.target.display())
    } else {
        format!(
            "1/3 hooks: already in {}, nothing changed",
            outcome.target.display()
        )
    });

    // 2. focusd, unless something already answers on the port.
    let (port, token) = (receiver.port, receiver.token.as_str());
    let mut started = false;
    let mut start_problem = None;
    if doctor::is_listening(port, token) {
        say(&format!("2/3 focusd: already running on port {port}"));
    } else {
        match start() {
            Ok(line) => {
                if wait_until_listening(port, token, wait) {
                    started = true;
                    say(&format!("2/3 focusd: started ({line})"));
                } else {
                    say(&format!(
                        "2/3 focusd: started, but nothing answered on port {port} within {} s ({line})",
                        wait.as_secs()
                    ));
                }
            }
            Err(problem) => {
                say(&format!("2/3 focusd: not started: {problem}"));
                start_problem = Some(problem);
            }
        }
    }

    // 3. The doctor's checks. No prompt seen yet is expected on a first run, and launch at
    // login is optional, so neither is shown here; `focusctl doctor` has both.
    say("3/3 checks:");
    let known_home = focus_platform::known_apps::for_this_os();
    let mut checks = Vec::new();
    let (config_check, parsed) = doctor::config(config_path, &known_home);
    checks.push(config_check);
    checks.push(doctor::hooks(settings, port, token));
    checks.push(doctor::daemon(port, token).0);
    let claude = doctor::find_claude(&std::env::var("PATH").unwrap_or_default());
    checks.push(doctor::claude_cli(claude.as_deref(), None, parsed.as_ref()));
    for check in &checks {
        say(&format_check(check));
    }

    let first_failure = checks.iter().find(|c| c.level == Level::Fail);
    let summary = if let Some(check) = first_failure {
        let fix = check.fix.as_deref().unwrap_or("run `focusctl doctor`");
        // A start that failed has a better explanation than "not running".
        let fix = match &start_problem {
            Some(problem) if check.what.contains("not running") => problem.as_str(),
            _ => fix,
        };
        format!("Not ready: {}. Next: {fix}.", check.what)
    } else {
        let how = if started {
            "focusd is running in the background"
        } else {
            "focusd was already running"
        };
        let restart = if hooks_changed {
            "restart any Claude Code session that was already open, then "
        } else {
            ""
        };
        format!(
            "Ready: {how}. Next: {restart}send a prompt in Claude Code and switch to another \
             window. `focusctl doctor` checks again at any time."
        )
    };
    Ok(SetupResult {
        ready: first_failure.is_none(),
        summary,
    })
}

fn format_check(check: &Check) -> String {
    let tag = match check.level {
        Level::Ok => "ok  ",
        Level::Note => "note",
        Level::Fail => "FAIL",
    };
    match &check.fix {
        Some(fix) => format!("      {tag}  {}\n            {fix}", check.what),
        None => format!("      {tag}  {}", check.what),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    const NOT_FOUND: &str =
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const UNAUTHORIZED: &str =
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    /// A stand-in for focusd's statistics endpoint on a loopback port. With a token it answers
    /// 200 to that token and 401 to any other; without one it answers 404 to everything, like
    /// some other program would. Dropping it stops the thread.
    struct Fake {
        addr: SocketAddr,
        stop: Arc<AtomicBool>,
    }

    impl Fake {
        fn start(listener: TcpListener, token: Option<String>) -> Fake {
            let addr = listener.local_addr().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if flag.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(mut stream) = stream else { continue };
                    // The client writes the request in pieces: read to the blank line.
                    let mut request = String::new();
                    let mut chunk = [0_u8; 512];
                    while !request.contains("\r\n\r\n") {
                        match stream.read(&mut chunk) {
                            Ok(n) if n > 0 => {
                                request.push_str(&String::from_utf8_lossy(&chunk[..n]));
                            }
                            _ => break,
                        }
                    }
                    let body = r#"{"week":{"prompts":0}}"#;
                    let reply = match &token {
                        None => NOT_FOUND.to_owned(),
                        Some(t) if request.contains(&format!("X-Focus-Token: {t}")) => format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        ),
                        Some(_) => UNAUTHORIZED.to_owned(),
                    };
                    let _ = stream.write_all(reply.as_bytes());
                }
            });
            Fake { addr, stop }
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            // Wakes the accept loop so it sees the flag.
            let _ = TcpStream::connect(self.addr);
        }
    }

    fn bind() -> TcpListener {
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap()
    }

    /// A port nothing listens on.
    fn free_port() -> u16 {
        bind().local_addr().unwrap().port()
    }

    /// A temporary data folder with a config on `port` and a path for a Claude Code settings
    /// file. Nothing outside the temporary folder is read or written.
    fn temp_setup(port: u16) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("data").join("config.toml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, format!("[receiver]\nport = {port}\n")).unwrap();
        let settings = dir.path().join("claude").join("settings.json");
        (dir, config, settings)
    }

    fn setup_once(
        settings: &Path,
        config: &Path,
        start: &mut dyn FnMut() -> Result<String, String>,
        wait: Duration,
    ) -> (SetupResult, Vec<String>) {
        let mut lines = Vec::new();
        let result = run(settings, config, start, wait, &mut |l| {
            lines.push(l.to_owned());
        })
        .unwrap();
        (result, lines)
    }

    #[test]
    fn it_installs_the_hooks_starts_focusd_and_a_second_run_changes_nothing() {
        let port = free_port();
        let (_dir, config, settings) = temp_setup(port);
        // The token exists before the first run so the stand-in can check it.
        let token = config::ensure_token(&config, false).unwrap().token;
        let mut fake = None;
        let mut starts = 0;
        let (first, lines) = setup_once(
            &settings,
            &config,
            &mut || {
                starts += 1;
                // "Starting focusd" is bringing the stand-in up on the configured port.
                let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
                    .map_err(|e| format!("test double could not bind: {e}"))?;
                fake = Some(Fake::start(listener, Some(token.clone())));
                Ok("test double".to_owned())
            },
            Duration::from_secs(3),
        );
        assert!(first.ready, "{} {lines:?}", first.summary);
        assert_eq!(starts, 1);
        assert!(lines[0].contains("added to"), "{lines:?}");
        assert!(first.summary.contains("restart any Claude Code session"));
        let settings_after_first = std::fs::read_to_string(&settings).unwrap();
        assert!(settings_after_first.contains("X-Focus-Token"));
        let config_after_first = std::fs::read_to_string(&config).unwrap();

        let (second, lines) = setup_once(
            &settings,
            &config,
            &mut || Err("must not start again".to_owned()),
            Duration::from_secs(1),
        );
        assert!(second.ready, "{} {lines:?}", second.summary);
        assert!(lines[0].contains("nothing changed"), "{lines:?}");
        assert!(lines[1].contains("already running"), "{lines:?}");
        assert!(
            second
                .summary
                .starts_with("Ready: focusd was already running")
        );
        assert!(!second.summary.contains("restart any"));
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            settings_after_first
        );
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            config_after_first
        );
        drop(fake);
    }

    #[test]
    fn existing_settings_are_kept() {
        let (_dir, config, settings) = temp_setup(free_port());
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "{\n  \"model\": \"x\"\n}\n").unwrap();
        let _ = setup_once(
            &settings,
            &config,
            &mut || Err("no".to_owned()),
            Duration::ZERO,
        );
        let text = std::fs::read_to_string(&settings).unwrap();
        assert!(text.contains("\"model\": \"x\""));
        assert!(text.contains("X-Focus-Token"));
    }

    #[test]
    fn a_focusd_that_cannot_be_started_gives_one_failure_with_its_reason() {
        let (_dir, config, settings) = temp_setup(free_port());
        let (result, _) = setup_once(
            &settings,
            &config,
            &mut || Err("focusd was not found next to focusctl or on the PATH".to_owned()),
            Duration::from_secs(1),
        );
        assert!(!result.ready);
        assert!(
            result
                .summary
                .starts_with("Not ready: focusd is not running"),
            "{}",
            result.summary
        );
        assert!(result.summary.contains("not found next to focusctl"));
        // The hooks were still installed, in the temporary file.
        assert!(
            std::fs::read_to_string(&settings)
                .unwrap()
                .contains("hooks")
        );
    }

    #[test]
    fn a_focusd_that_never_answers_is_not_ready() {
        let (_dir, config, settings) = temp_setup(free_port());
        let (result, lines) = setup_once(
            &settings,
            &config,
            &mut || Ok("a program that does nothing".to_owned()),
            Duration::from_millis(300),
        );
        assert!(!result.ready);
        assert!(result.summary.starts_with("Not ready:"));
        assert!(
            lines.iter().any(|l| l.contains("nothing answered")),
            "{lines:?}"
        );
    }

    #[test]
    fn another_program_on_the_port_is_not_started_over() {
        let listener = bind();
        let port = listener.local_addr().unwrap().port();
        let _other = Fake::start(listener, None);
        let (_dir, config, settings) = temp_setup(port);
        let mut started = false;
        let (result, _) = setup_once(
            &settings,
            &config,
            &mut || {
                started = true;
                Ok(String::new())
            },
            Duration::from_millis(300),
        );
        assert!(!started);
        assert!(!result.ready);
        assert!(
            result
                .summary
                .contains("answers, but not as this version of focusd"),
            "{}",
            result.summary
        );
    }

    #[test]
    fn focusd_is_found_beside_focusctl_before_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let beside = dir.path().join("beside");
        let on_path = dir.path().join("onpath");
        std::fs::create_dir_all(&beside).unwrap();
        std::fs::create_dir_all(&on_path).unwrap();
        let name = if cfg!(windows) {
            "focusd.exe"
        } else {
            "focusd"
        };
        std::fs::write(on_path.join(name), "").unwrap();
        let path_env = std::env::join_paths([&on_path]).unwrap();
        let path_env = path_env.to_string_lossy();
        assert_eq!(
            find_focusd(Some(&beside), &path_env),
            Some(on_path.join(name))
        );
        std::fs::write(beside.join(name), "").unwrap();
        assert_eq!(
            find_focusd(Some(&beside), &path_env),
            Some(beside.join(name))
        );
        assert_eq!(find_focusd(None, ""), None);
    }

    #[test]
    fn a_detached_program_runs_and_its_output_goes_to_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("logs").join("focusd.log");
        // The test program itself, asked to list its tests: it prints and exits.
        let me = std::env::current_exe().unwrap();
        let mut child = spawn_detached(&me, &["--list"], &log).unwrap();
        assert!(child.wait().unwrap().success());
        assert!(!std::fs::read_to_string(&log).unwrap().is_empty());
        assert!(spawn_detached(&dir.path().join("missing"), &[], &log).is_err());
    }
}
