// SPDX-License-Identifier: Apache-2.0
//! Redacts likely secrets. Used at the receiver, before anything is stored, and again before
//! text is shown on screen or sent to the opt-in LLM recap (design section 10): common API-key formats, `password=` and `token=` style pairs, bearer tokens and PEM
//! blocks. This is a best-effort filter on already truncated text, not a guarantee.

const REDACTED: &str = "[redacted]";
/// Prefixes of well-known credential formats.
const KEY_PREFIXES: [&str; 20] = [
    "sk-",
    "sk_live_",
    "sk_test_",
    "ghp_",
    "gho_",
    "ghs_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "AKIA",
    "AIza",
    "eyJ",
    "glpat-",
    "npm_",
    "hf_",
    "ghu_",
    "ghr_",
    "xoxa-",
    "xoxs-",
    "rk_live_",
];
/// A prefixed word shorter than this is more likely prose than a key.
const MIN_KEY_CHARS: usize = 16;
/// Names whose value is secret in `name=value` or `name: value`.
const SECRET_NAMES: [&str; 8] = [
    "password", "passwd", "pwd", "token", "secret", "api_key", "apikey", "api-key",
];
const PEM_BEGIN: &str = "-----BEGIN ";
const PEM_END: &str = "-----END ";

/// Replaces likely secrets with `[redacted]`. Best effort, on text that is already bounded.
pub fn redact(text: &str) -> String {
    let text = redact_pem(text);
    let mut out = String::with_capacity(text.len());
    let mut hide_next = false;
    let mut after_name = false;
    for piece in text.split_inclusive(char::is_whitespace) {
        let word = piece.trim_end();
        let space = &piece[word.len()..];
        if word.is_empty() {
            out.push_str(piece);
            continue;
        }
        if std::mem::take(&mut hide_next) {
            out.push_str(REDACTED);
            after_name = false;
        } else if let Some(name) = secret_assignment(word) {
            out.push_str(name);
            out.push_str(REDACTED);
        } else if looks_like_key(word) {
            out.push_str(REDACTED);
        } else {
            out.push_str(word);
            // `Bearer <token>`, `--password <value>`, `token: <value>` and `API_KEY = <value>`
            // hide the next word.
            let assigns = after_name && (word == "=" || word == ":");
            hide_next = assigns || word.eq_ignore_ascii_case("bearer") || announces_secret(word);
            after_name = is_secret_name(word);
        }
        out.push_str(space);
    }
    out
}

/// For `name=value` or `name:value` with a secret name, the part to keep (`name=`).
fn secret_assignment(word: &str) -> Option<&str> {
    let at = word.find(['=', ':'])?;
    let (name, value) = (&word[..at], &word[at + 1..]);
    (!value.is_empty() && is_secret_name(name)).then(|| &word[..=at])
}

/// `password:` or `token=` with the value in the next word, or a flag such as `--password`.
fn announces_secret(word: &str) -> bool {
    word.strip_suffix([':', '=']).is_some_and(is_secret_name)
        || (word.starts_with('-') && is_secret_name(word))
}

fn is_secret_name(name: &str) -> bool {
    let name = name.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
    let name = name.trim_end_matches(['"', '\'']);
    SECRET_NAMES.iter().any(|secret| {
        name.len() >= secret.len()
            && name.is_char_boundary(name.len() - secret.len())
            && name[name.len() - secret.len()..].eq_ignore_ascii_case(secret)
    })
}

fn looks_like_key(word: &str) -> bool {
    let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    word.len() >= MIN_KEY_CHARS && KEY_PREFIXES.iter().any(|prefix| word.starts_with(prefix))
}

fn redact_pem(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(begin) = rest.find(PEM_BEGIN) {
        out.push_str(&rest[..begin]);
        out.push_str(REDACTED);
        let after_begin = &rest[begin + PEM_BEGIN.len()..];
        // Skip to the end of the END line; an unterminated block hides everything after it.
        rest = match after_begin.find(PEM_END) {
            Some(end) => {
                let tail = &after_begin[end + PEM_END.len()..];
                tail.find("-----").map_or("", |close| &tail[close + 5..])
            }
            None => "",
        };
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_formats_are_hidden() {
        let text = "use sk-abcdefghijklmnopqrstuv and ghp_0123456789abcdefghij, then AKIAABCDEFGHIJKLMNOP.";
        assert_eq!(
            redact(text),
            "use [redacted] and [redacted] then [redacted]"
        );
        assert_eq!(
            redact("the sk-8 wrench and eyJ are fine"),
            "the sk-8 wrench and eyJ are fine"
        );
    }

    #[test]
    fn assignments_and_bearer_tokens_are_hidden() {
        assert_eq!(
            redact("DB_PASSWORD=hunter2 ok"),
            "DB_PASSWORD=[redacted] ok"
        );
        assert_eq!(redact("token: abc123 next"), "token: [redacted] next");
        assert_eq!(
            redact("export API_KEY = abc123"),
            "export API_KEY = [redacted]"
        );
        assert_eq!(
            redact("--password hunter2 --verbose"),
            "--password [redacted] --verbose"
        );
        assert_eq!(
            redact("curl -H 'Authorization: Bearer abc.def' x"),
            "curl -H 'Authorization: Bearer [redacted] x"
        );
        assert_eq!(redact("reset your password now"), "reset your password now");
        assert_eq!(redact("tokenizer=fast"), "tokenizer=fast");
    }

    #[test]
    fn pem_blocks_are_hidden_even_when_cut_off() {
        let pem = "key:\n-----BEGIN RSA PRIVATE KEY-----\nMIIE\nabcd\n-----END RSA PRIVATE KEY-----\ndone";
        assert_eq!(redact(pem), "key:\n[redacted]\ndone");
        assert_eq!(
            redact("x -----BEGIN PRIVATE KEY-----\nMIIE cut"),
            "x [redacted]"
        );
    }

    #[test]
    fn ordinary_text_is_unchanged() {
        let text = "Run the tests and fix failures in src/sum.ts: 3 of 42 fail.";
        assert_eq!(redact(text), text);
        assert_eq!(redact(""), "");
        assert_eq!(redact("héllo  wörld\n"), "héllo  wörld\n");
    }
}
