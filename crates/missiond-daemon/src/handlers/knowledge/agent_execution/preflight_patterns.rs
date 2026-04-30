/// wave-20 / task 03 — repo-relative path-vs-pattern matcher used by the
/// task-contract scope projection in preflight. Mirrors the JS helper in
/// `scripts/lib/missiond_lisp.mjs::pathMatchesPattern` so daemon-side
/// preflight, the post-commit guard (`scripts/task-scope-guard.mjs`), and
/// the verifier (`scripts/verify-task-contract.mjs`) all key off the same
/// glob semantics.
pub(super) fn pattern_matches_path(file_path: &str, pattern: &str) -> bool {
    if file_path.is_empty() || pattern.is_empty() {
        return false;
    }
    let norm_path = normalize_repo_relative(file_path);
    let pat = normalize_repo_relative(pattern);
    if !pat.contains('*') && !pat.contains('?') {
        if norm_path == pat {
            return true;
        }
        let prefix = if pat.ends_with('/') {
            pat.clone()
        } else {
            format!("{}/", pat)
        };
        return norm_path.starts_with(&prefix);
    }
    glob_to_regex(&pat).is_match(&norm_path)
}

/// Normalise a path or pattern to a repo-relative form: backslash → slash,
/// strip a single leading `./`, and any leading `/` so absolute-style
/// patterns (rare in our contracts) still match repo-relative entries.
fn normalize_repo_relative(input: &str) -> String {
    let mut s = input.replace('\\', "/");
    if let Some(stripped) = s.strip_prefix("./") {
        s = stripped.to_string();
    }
    while let Some(stripped) = s.strip_prefix('/') {
        s = stripped.to_string();
    }
    s
}

/// Compile a glob pattern into a regex anchored on both ends. Mirrors the
/// JS `globToRegExp` in `scripts/lib/missiond_lisp.mjs` so the JS guard
/// and the daemon-side preflight stay in lock-step.
fn glob_to_regex(pattern: &str) -> regex::Regex {
    let mut out = String::with_capacity(pattern.len() + 4);
    out.push('^');
    let bytes: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '*' {
            if i + 1 < bytes.len() && bytes[i + 1] == '*' {
                out.push_str(".*");
                i += 2;
            } else {
                out.push_str("[^/]*");
                i += 1;
            }
        } else if c == '?' {
            out.push_str("[^/]");
            i += 1;
        } else if matches!(
            c,
            '.' | '+' | '^' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\'
        ) {
            out.push('\\');
            out.push(c);
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out.push('$');
    regex::Regex::new(&out).unwrap_or_else(|_| regex::Regex::new("$.^").unwrap())
}
