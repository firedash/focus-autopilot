// SPDX-License-Identifier: Apache-2.0
//! Runs the user's own Claude Code CLI to summarise a run (design section 10, backlog F2).
//! Spawned with an argument array, never through a shell; no tools, no MCP servers, no hooks,
//! an empty working folder, the facts on stdin. The only outbound network use in the app, and
//! only when `recap.mode = "llm"`.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The arguments from the design. `--tools ""` removes the built-in tools, a strict MCP
/// config with none given loads no servers, and `disableAllHooks` stops the recap session
/// from firing focusd's own hooks.
pub const CLAUDE_ARGS: &[&str] = &[
    "-p",
    "--model",
    "haiku",
    "--tools",
    "",
    "--strict-mcp-config",
    "--settings",
    r#"{"disableAllHooks": true}"#,
    "--no-session-persistence",
    "--max-turns",
    "1",
    "--output-format",
    "text",
];

/// Runs `program args` in `dir`, writes `prompt` to its stdin, and returns what it prints.
/// Kills it if it takes longer than `timeout`.
///
/// # Errors
/// A readable reason, for the terminal log: the program is missing, it timed out, or it
/// exited with an error (with the start of its stderr).
pub fn run_cli(
    program: &str,
    args: &[&str],
    dir: &Path,
    prompt: &str,
    timeout: Duration,
) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut child = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start `{program}`: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A write error means the program already exited; its status says why.
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "`{program}` took longer than {}s",
                    timeout.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let read = |pipe: Option<&mut dyn std::io::Read>| {
        let mut text = String::new();
        if let Some(pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    };
    let out = read(child.stdout.as_mut().map(|p| p as &mut dyn std::io::Read));
    if status.success() {
        Ok(out)
    } else {
        let err = read(child.stderr.as_mut().map(|p| p as &mut dyn std::io::Read));
        let err: String = err.split_whitespace().collect::<Vec<_>>().join(" ");
        Err(format!(
            "`{program}` failed ({status}): {}",
            err.chars().take(200).collect::<String>()
        ))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("focus-recap-test-{}", std::process::id()))
    }

    #[test]
    fn the_prompt_goes_in_on_stdin_and_the_reply_comes_back() {
        let out = run_cli("cat", &[], &dir(), "Asked: x\n", Duration::from_secs(5)).unwrap();
        assert_eq!(out, "Asked: x\n");
    }

    #[test]
    fn a_slow_program_is_killed_at_the_timeout() {
        let started = Instant::now();
        let err = run_cli("sleep", &["30"], &dir(), "", Duration::from_millis(300)).unwrap_err();
        assert!(err.contains("took longer"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_missing_program_and_a_failing_program_explain_themselves() {
        let err = run_cli(
            "no-such-program-xyz",
            &[],
            &dir(),
            "",
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(err.contains("could not start"), "{err}");
        let err = run_cli(
            "sh",
            &["-c", "echo bad flag >&2; exit 2"],
            &dir(),
            "",
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(err.contains("bad flag"), "{err}");
    }

    #[test]
    fn the_design_arguments_are_what_the_design_says() {
        assert!(CLAUDE_ARGS.windows(2).any(|w| w == ["--tools", ""]));
        assert!(CLAUDE_ARGS.contains(&"--strict-mcp-config"));
        assert!(CLAUDE_ARGS.iter().any(|a| a.contains("disableAllHooks")));
    }
}
