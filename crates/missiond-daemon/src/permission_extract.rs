//! Permission extraction — parse Claude Code "don't ask again" dialog option text.
//!
//! Shared single source of truth used by:
//! - `handlers::compute::pty::mission_pty_confirm` — manual/explicit confirm
//! - `workers::local::pty_event_worker::handle_confirm_required` — auto-approve path
//!
//! Both call sites must produce the same learned entries so the YAML + injected
//! `.claude/settings.local.json` never diverge.

/// Result of parsing the Option(2) text of a Claude Code confirmation dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedConfirm {
    /// The pattern the user/auto-approve chose to allowlist, e.g. `python3:*` or `Read(/etc/passwd)`.
    pub pattern: String,
    /// Optional project path hint from "commands in <path>" — carries the project scope so
    /// downstream code can resolve it to a project_id via `ProjectRegistry::resolve`.
    pub project_path: Option<String>,
}

/// Parse the "Yes, and don't ask again for: <pattern>" / "…for X commands in <path>" option text
/// from a Claude Code confirmation dialog.
///
/// Shapes supported (case insensitive, curly quotes normalized):
///
///   "Yes, and don't ask again for: python3:*"
///   "Yes, and don't ask again for python3 commands in /Users/jinchen/Projects"
///   "Yes, and don't ask again for Read(/etc/passwd)"
///   "是，不再询问 python3:*"
///
/// Returns `None` when no recognizable "don't ask again" marker is present.
pub fn extract_confirm(option_text: &str) -> Option<ExtractedConfirm> {
    // Claude Code TUI sometimes renders the apostrophe as U+2018 / U+2019.
    // Normalize first so substring matching is stable.
    let normalized: String = option_text
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            other => other,
        })
        .collect();
    let lower = normalized.to_lowercase();

    // Match longest first so "don't ask again for:" wins over "don't ask again for".
    let marker = if lower.contains("don't ask again for:") {
        "don't ask again for:"
    } else if lower.contains("dont ask again for:") {
        "dont ask again for:"
    } else if lower.contains("don't ask again for") {
        "don't ask again for"
    } else if lower.contains("dont ask again for") {
        "dont ask again for"
    } else if lower.contains("不再询问") {
        "不再询问"
    } else {
        return None;
    };

    let idx = lower.find(marker)?;
    let after = normalized[idx + marker.len()..].trim();
    // Strip a leading ':' that might follow the marker.
    let after = after.trim_start_matches(':').trim();
    if after.is_empty() {
        return None;
    }

    // Try to split off " commands in <path>" tail (case-insensitive match on lowercased copy).
    let after_lower = after.to_lowercase();
    let (pattern, project_path) = if let Some(cin_idx) = after_lower.find(" commands in ") {
        let pattern = after[..cin_idx].trim().to_string();
        let path = after[cin_idx + " commands in ".len()..].trim().to_string();
        let project_path = if path.is_empty() { None } else { Some(path) };
        (pattern, project_path)
    } else if let Some(in_idx) = find_word(&after_lower, " in ") {
        // Also handle the bare "<pattern> in <path>" shape.
        let pattern = after[..in_idx].trim().to_string();
        let path = after[in_idx + " in ".len()..].trim().to_string();
        // Only accept as a path when it actually looks like a filesystem path
        // (absolute path, ~, or drive-letter) — avoid eating trailing English.
        if looks_like_path(&path) {
            (pattern, Some(path))
        } else {
            (after.to_string(), None)
        }
    } else {
        (after.to_string(), None)
    };

    if pattern.is_empty() {
        return None;
    }

    Some(ExtractedConfirm {
        pattern,
        project_path,
    })
}

/// Case-insensitive find of a whole-word needle, returns index in the haystack.
fn find_word(haystack: &str, needle: &str) -> Option<usize> {
    haystack.find(needle)
}

/// Heuristic: does the trailing string look like a filesystem path?
fn looks_like_path(s: &str) -> bool {
    s.starts_with('/')
        || s.starts_with('~')
        || s.starts_with("./")
        || s.starts_with("../")
        || s.chars().nth(1) == Some(':') // windows drive letter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_with_project_path() {
        let got = extract_confirm(
            "Yes, and don't ask again for python3 commands in /Users/jinchen/Projects",
        );
        assert_eq!(
            got,
            Some(ExtractedConfirm {
                pattern: "python3".to_string(),
                project_path: Some("/Users/jinchen/Projects".to_string()),
            })
        );
    }

    #[test]
    fn english_with_colon_no_project() {
        let got = extract_confirm("Yes, and don't ask again for: python3:*");
        assert_eq!(
            got,
            Some(ExtractedConfirm {
                pattern: "python3:*".to_string(),
                project_path: None,
            })
        );
    }

    #[test]
    fn english_with_colon_tool_param() {
        let got = extract_confirm("Yes, and don't ask again for: Read(/etc/passwd)");
        assert_eq!(
            got,
            Some(ExtractedConfirm {
                pattern: "Read(/etc/passwd)".to_string(),
                project_path: None,
            })
        );
    }

    #[test]
    fn curly_quote_normalized() {
        let got = extract_confirm("Yes, and don\u{2019}t ask again for: python3:*");
        assert_eq!(
            got,
            Some(ExtractedConfirm {
                pattern: "python3:*".to_string(),
                project_path: None,
            })
        );
    }

    #[test]
    fn chinese() {
        let got = extract_confirm("是，不再询问 python3:*");
        assert_eq!(
            got,
            Some(ExtractedConfirm {
                pattern: "python3:*".to_string(),
                project_path: None,
            })
        );
    }

    #[test]
    fn no_match() {
        assert_eq!(extract_confirm("Yes"), None);
        assert_eq!(extract_confirm("No"), None);
        assert_eq!(extract_confirm(""), None);
    }

    #[test]
    fn english_with_project_path_and_colon_pattern() {
        let got = extract_confirm(
            "Yes, and don't ask again for python3:* commands in /Users/jinchen/Projects/missiond",
        );
        assert_eq!(
            got,
            Some(ExtractedConfirm {
                pattern: "python3:*".to_string(),
                project_path: Some("/Users/jinchen/Projects/missiond".to_string()),
            })
        );
    }

    #[test]
    fn english_with_dont_apostrophe_variant() {
        let got = extract_confirm("Yes, and dont ask again for: python3:*");
        assert_eq!(
            got,
            Some(ExtractedConfirm {
                pattern: "python3:*".to_string(),
                project_path: None,
            })
        );
    }

    #[test]
    fn in_followed_by_non_path_is_not_project() {
        // Regression guard: "in this session" should not be parsed as a project path.
        let got = extract_confirm("Yes, and don't ask again for python3 in this session");
        assert_eq!(
            got,
            Some(ExtractedConfirm {
                pattern: "python3 in this session".to_string(),
                project_path: None,
            })
        );
    }
}
