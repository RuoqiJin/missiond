use super::super::types::ParsedDag;
use super::node_form::parse_node_form;

/// Top-level entry: parse plan.sexp_text for `(node ...)` forms only.
pub(in crate::handlers::knowledge::plan_dag) fn parse_plan_dag(sexp: &str) -> ParsedDag {
    let mut out = ParsedDag::default();
    for form in scan_top_level_forms(sexp) {
        let head = top_form_head(&form).unwrap_or_default();
        let head_lc = head.to_ascii_lowercase();
        if head_lc == "node" {
            if let Some(node) = parse_node_form(&form) {
                out.nodes.push(node);
            }
        } else if !head.is_empty() {
            // Non-node sibling — record verbatim so authors can see what the
            // scheduler skipped (e.g., :goal, :phases, :tasks, comments).
            out.unsupported_top_forms.push(form);
        }
    }
    out
}

/// Walk through the outer plan envelope and yield the s-expressions sitting at
/// "top level" inside it. We treat anything inside the outermost paren of the
/// plan envelope as a sibling to be considered. This is intentionally
/// shallow — we do NOT recurse into nested forms looking for `(node ...)`,
/// because that would silently consume nodes meant for sub-phases.
pub(super) fn scan_top_level_forms(sexp: &str) -> Vec<String> {
    let trimmed = sexp.trim();
    let bytes: Vec<char> = trimmed.chars().collect();
    let n = bytes.len();
    if n == 0 || bytes[0] != '(' {
        return Vec::new();
    }
    // Find the slice immediately inside the outermost paren.
    // Strategy: skip the head symbol of the outer envelope, then collect
    // sibling forms until we close the outer paren.
    let mut i = 1usize;
    // Skip whitespace
    while i < n && bytes[i].is_whitespace() {
        i += 1;
    }
    // Skip the head symbol (e.g. `plan`, `plan-draft`, `PLAN`).
    while i < n
        && !bytes[i].is_whitespace()
        && bytes[i] != '('
        && bytes[i] != ')'
        && bytes[i] != '"'
    {
        i += 1;
    }
    let mut forms: Vec<String> = Vec::new();
    let mut depth: i64 = 0;
    let mut in_string = false;
    let mut esc = false;
    let mut current_start: Option<usize> = None;
    while i < n {
        let c = bytes[i];
        if in_string {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c == '(' {
            if depth == 0 {
                current_start = Some(i);
            }
            depth += 1;
            i += 1;
            continue;
        }
        if c == ')' {
            depth -= 1;
            if depth == 0 {
                if let Some(start) = current_start.take() {
                    let form: String = bytes[start..=i].iter().collect();
                    forms.push(form);
                }
                i += 1;
                continue;
            }
            if depth < 0 {
                // Closing the outer envelope — stop.
                break;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    forms
}

/// Get the head symbol of a top-level form like `(node :id ...)` -> `node`.
pub(super) fn top_form_head(form: &str) -> Option<String> {
    let trimmed = form.trim_start();
    let inner = trimmed.strip_prefix('(')?.trim_start();
    let mut end = 0usize;
    for (idx, ch) in inner.char_indices() {
        if ch.is_whitespace() || ch == '(' || ch == ')' || ch == '"' {
            break;
        }
        end = idx + ch.len_utf8();
    }
    if end == 0 {
        None
    } else {
        Some(inner[..end].to_string())
    }
}
