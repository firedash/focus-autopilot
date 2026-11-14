// SPDX-License-Identifier: Apache-2.0
//! The opt-in LLM recap (design section 10, backlog F2): the text sent to the user's own
//! Claude Code CLI and the reading of its reply. Only truncated, redacted hook fields are
//! sent; never file contents or the transcript. Spawning the CLI is the shell's job.

use crate::events::clean_text;
use crate::recap::{RecapCard, need_text};
use crate::redact::redact;
use crate::run::Outcome;
use crate::session::Session;

const INSTRUCTIONS: &str = "\
Summarize this coding-agent run for a developer returning from a break.
Reply with exactly three lines of plain text, no markdown, each under 100 characters:
Asked: <what the developer asked>
Did: <what the agent did>
Needs: <the decision the developer must make, or \"nothing\">
Use only the facts below. Do not guess.
---";
const FILES_SENT: usize = 5;
const REPLY_LINES: usize = 3;
const REPLY_LINE_MAX_CHARS: usize = 120;

pub(crate) fn prompt(session: &Session) -> String {
    let run = &session.run;
    let mut facts = vec![format!("Prompt: {}", run.prompt)];
    if !run.commands.is_empty() {
        facts.push(format!("Commands run: {}", run.commands.join("; ")));
    }
    if !run.files.is_empty() {
        let files: Vec<&str> = run
            .files
            .iter()
            .take(FILES_SENT)
            .map(String::as_str)
            .collect();
        facts.push(format!("Files edited: {}", files.join(", ")));
    }
    facts.push(format!(
        "Tool calls: {}, sub-agents: {}",
        run.tools, run.subagents
    ));
    match (&run.need, &run.outcome) {
        (Some(need), _) => facts.push(format!("Waiting for: {}", need_text(need))),
        (None, Some(Outcome::Done { message, .. })) => {
            facts.push(format!("Last message: {message}"));
        }
        (None, Some(Outcome::Error(error))) => facts.push(format!("Stopped with error: {error}")),
        (None, None) => facts.push("Still running.".to_owned()),
    }
    format!("{INSTRUCTIONS}\n{}\n", redact(&facts.join("\n")))
}

/// Cuts at a word boundary and adds an ellipsis, rather than stopping mid-word.
fn shorten(line: &str, max_chars: usize) -> String {
    if line.chars().count() <= max_chars {
        return line.to_owned();
    }
    let room: String = line.chars().take(max_chars - 1).collect();
    let cut = room
        .rfind(' ')
        .filter(|&at| at > room.len() / 2)
        .unwrap_or(room.len());
    format!("{}…", room[..cut].trim_end())
}

/// Up to three non-empty lines, cleaned like any other outside text. `None` if there are none.
pub(crate) fn parse(reply: &str) -> Option<RecapCard> {
    let lines: Vec<String> = reply
        .lines()
        .map(|line| shorten(&clean_text(line.trim(), usize::MAX), REPLY_LINE_MAX_CHARS))
        .filter(|line| !line.is_empty())
        .take(REPLY_LINES)
        .collect();
    (!lines.is_empty()).then_some(RecapCard { lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_are_cut_to_three_clean_lines() {
        let reply = "\n Asked: fix tests \n\u{1b}[1mDid:\u{1b}[0m fixed sum.ts\n\nNeeds: nothing\nextra line\n";
        let card = parse(reply).unwrap();
        assert_eq!(
            card.lines,
            vec!["Asked: fix tests", "Did: fixed sum.ts", "Needs: nothing"]
        );
        assert_eq!(parse(" \n\n"), None);
        let long = parse(&"x".repeat(500)).unwrap();
        assert_eq!(long.lines[0].chars().count(), REPLY_LINE_MAX_CHARS);
        assert!(long.lines[0].ends_with('…'));
    }

    #[test]
    fn long_lines_are_cut_at_a_word_not_mid_word() {
        let line = format!("Did: {} supercalifragilistic tail", "word ".repeat(30));
        let card = parse(&line).unwrap();
        let shown = &card.lines[0];
        assert!(shown.chars().count() <= REPLY_LINE_MAX_CHARS);
        assert!(shown.ends_with("word…"), "{shown}");
    }
}
