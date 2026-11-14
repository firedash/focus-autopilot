// SPDX-License-Identifier: Apache-2.0
//! Folder patterns for `ignore.cwd_globs` (backlog G5). `*` matches within one path
//! segment, `**` across segments, `?` one character. A pattern that matches a folder also
//! covers everything inside it. `\` and `/` are treated alike, so one pattern style works
//! on both OSes. Expanding `~` is the shell's job.

/// Patterns and paths longer than this never match; it bounds the matching work.
const MAX_LEN: usize = 1_024;

pub(crate) fn matches_any(patterns: &[String], path: &str) -> bool {
    patterns.iter().any(|pattern| matches(pattern, path))
}

pub(crate) fn matches(pattern: &str, path: &str) -> bool {
    let normalize = |text: &str| -> Vec<char> {
        text.trim_end_matches(['/', '\\'])
            .chars()
            .map(|c| if c == '\\' { '/' } else { c })
            .collect()
    };
    let (pattern, path) = (normalize(pattern), normalize(path));
    if pattern.is_empty() || pattern.len() > MAX_LEN || path.len() > MAX_LEN {
        return false;
    }
    // reachable[i]: the pattern consumed so far can match the first i characters of the path.
    let mut reachable = vec![false; path.len() + 1];
    reachable[0] = true;
    let mut rest = pattern.as_slice();
    while let Some((&c, tail)) = rest.split_first() {
        let double_star = c == '*' && tail.first() == Some(&'*');
        rest = if double_star { &tail[1..] } else { tail };
        reachable = step(&reachable, &path, c, double_star);
    }
    // The whole path, or one of its parent folders.
    reachable[path.len()] || (1..path.len()).any(|i| reachable[i] && path[i] == '/')
}

fn step(reachable: &[bool], path: &[char], c: char, double_star: bool) -> Vec<bool> {
    let mut next = vec![false; reachable.len()];
    match c {
        '*' => {
            let mut open = false;
            for i in 0..reachable.len() {
                open = (open && (double_star || path[i - 1] != '/')) || reachable[i];
                next[i] = open;
            }
        }
        _ => {
            for i in 0..path.len() {
                let fits = if c == '?' {
                    path[i] != '/'
                } else {
                    path[i] == c
                };
                next[i + 1] = reachable[i] && fits;
            }
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_and_question_marks_follow_path_segments() {
        assert!(matches("/work/secret", "/work/secret"));
        assert!(matches("/work/secret/", "/work/secret"));
        assert!(matches("/work/secret", "/work/secret/sub"));
        assert!(!matches("/work/secret", "/work/secrets"));
        assert!(matches("/work/*", "/work/app"));
        assert!(!matches("/work/*", "/work"));
        assert!(!matches("/w*/sub", "/work/app/sub"));
        assert!(matches("/w**/sub", "/work/app/sub"));
        assert!(matches("**/client-*", "/home/me/code/client-acme"));
        assert!(matches("**/client-*", "/home/me/code/client-acme/web"));
        assert!(!matches("**/client-*", "/home/me/code/server"));
        assert!(matches("/work/app?", "/work/app1"));
        assert!(!matches("/work/app?", "/work/app"));
        assert!(!matches("/work/a?b", "/work/a/b"));
        assert!(!matches("", "/work"));
        assert!(matches("*", "work"));
    }

    #[test]
    fn windows_separators_are_treated_like_slashes() {
        assert!(matches(r"C:\work\**", r"C:\work\app\src"));
        assert!(matches("C:/work/*", r"C:\work\app"));
        assert!(matches_any(
            &["/a".into(), r"C:\work\*".into()],
            r"C:\work\app"
        ));
        assert!(!matches_any(&[], "/work"));
    }

    #[test]
    fn oversized_input_never_matches() {
        let long = "a".repeat(MAX_LEN + 1);
        assert!(!matches("**", &long));
        assert!(!matches(&long, &long));
    }
}
