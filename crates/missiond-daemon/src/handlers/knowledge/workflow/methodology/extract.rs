use super::*;

/// Extract `(step <id> <body…>)` forms from a methodology Lisp source. Multi-line
/// bodies are accumulated using paren depth tracking that ignores string contents.
/// Pure-fn for testing.
pub(in crate::handlers::knowledge::workflow) fn extract_steps(
    content: &str,
) -> Vec<MethodologyStep> {
    let mut steps = Vec::new();
    let mut buffer: Option<(String, String, i32, bool, bool)> = None;
    // (id, body, depth, in_string, escaped)

    for line in content.lines() {
        if let Some((mut id, mut body, mut depth, mut in_string, mut escaped)) = buffer.take() {
            body.push('\n');
            for ch in line.chars() {
                body.push(ch);
                advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
                if depth == 0 {
                    steps.push(MethodologyStep {
                        id: std::mem::take(&mut id),
                        body: std::mem::take(&mut body),
                    });
                    buffer = None;
                    break;
                }
            }
            if depth > 0 {
                buffer = Some((id, body, depth, in_string, escaped));
            }
            continue;
        }

        let leading = line.chars().take_while(|c| c.is_whitespace()).count();
        let rest = &line[leading..];
        if !rest.starts_with("(step") {
            continue;
        }
        let after_step = &rest["(step".len()..];
        if !after_step.starts_with(|c: char| c.is_whitespace()) {
            continue; // e.g. (steps … shouldn't match
        }
        let after_ws = after_step.trim_start();
        let id_end = after_ws
            .find(|c: char| c.is_whitespace() || c == ')')
            .unwrap_or(after_ws.len());
        let id = after_ws[..id_end].trim().to_string();
        if id.is_empty() {
            continue;
        }

        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escaped = false;
        let mut body = String::new();
        let mut closed = false;
        for ch in rest.chars() {
            body.push(ch);
            advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
            if depth == 0 && body.ends_with(')') {
                steps.push(MethodologyStep {
                    id: id.clone(),
                    body: body.clone(),
                });
                closed = true;
                break;
            }
        }
        if !closed && depth > 0 {
            buffer = Some((id, body, depth, in_string, escaped));
        }
    }

    steps
}

/// Variant of [`extract_steps`] that also records each step's 0-based source
/// line. Used by [`build_generated_yaml`] to assign `phase_id` metadata when
/// a step's line falls inside a `(phase …)` form's range. The matching rules
/// are identical to `extract_steps` so the back-compat tests still cover the
/// recognition surface.
pub(in crate::handlers::knowledge::workflow) fn extract_steps_with_lines(
    content: &str,
) -> Vec<LocatedStep> {
    let mut out: Vec<LocatedStep> = Vec::new();
    let mut buffer: Option<(LocatedStep, i32, bool, bool)> = None;
    // (located_step, depth, in_string, escaped)

    for (line_idx, line) in content.lines().enumerate() {
        if let Some((mut ls, mut depth, mut in_string, mut escaped)) = buffer.take() {
            ls.step.body.push('\n');
            for ch in line.chars() {
                ls.step.body.push(ch);
                advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
                if depth == 0 {
                    out.push(LocatedStep {
                        step: MethodologyStep {
                            id: std::mem::take(&mut ls.step.id),
                            body: std::mem::take(&mut ls.step.body),
                        },
                        start_line: ls.start_line,
                    });
                    buffer = None;
                    break;
                }
            }
            if depth > 0 {
                buffer = Some((ls, depth, in_string, escaped));
            }
            continue;
        }

        let leading = line.chars().take_while(|c| c.is_whitespace()).count();
        let rest = &line[leading..];
        if !rest.starts_with("(step") {
            continue;
        }
        let after_step = &rest["(step".len()..];
        if !after_step.starts_with(|c: char| c.is_whitespace()) {
            continue; // e.g. (steps … shouldn't match
        }
        let after_ws = after_step.trim_start();
        let id_end = after_ws
            .find(|c: char| c.is_whitespace() || c == ')')
            .unwrap_or(after_ws.len());
        let id = after_ws[..id_end].trim().to_string();
        if id.is_empty() {
            continue;
        }

        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escaped = false;
        let mut body = String::new();
        let mut closed = false;
        for ch in rest.chars() {
            body.push(ch);
            advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
            if depth == 0 && body.ends_with(')') {
                out.push(LocatedStep {
                    step: MethodologyStep {
                        id: id.clone(),
                        body: body.clone(),
                    },
                    start_line: line_idx,
                });
                closed = true;
                break;
            }
        }
        if !closed && depth > 0 {
            buffer = Some((
                LocatedStep {
                    step: MethodologyStep { id, body },
                    start_line: line_idx,
                },
                depth,
                in_string,
                escaped,
            ));
        }
    }

    out
}

/// Conservative semantic lifter for the methodology compiler v0.
///
/// Recognises six higher-order forms — `(phase …)`, `(principle …)`,
/// `(anti-pattern …)`, `(gate …)`, `(artifact …)`, `(authority …)` — when
/// they appear as standalone forms whose opening paren sits at the start of
/// a (whitespace-trimmed) line. This matches the convention used by
/// [`extract_steps`] and by every methodology Lisp shipped under
/// `.missiond/workflows/`. Forms appearing only as inner tokens of another
/// expression are deliberately ignored — the lifter never tries to be a
/// real sexp parser, and never speculates about meaning the source did not
/// declare.
///
/// The lifter NEVER converts these forms into executable nodes. They live in
/// `methodology_metadata` on the generated YAML so the deterministic
/// compiler's contract — "v0 only emits nodes for `(step …)`" — stays
/// intact (intent-flow.lisp :: F-methodology-to-executable-compile :: s2
/// `phases / gates / anti-patterns / authority lifting` is no longer
/// pending; semantic execution remains a future forge concern).
pub(in crate::handlers::knowledge::workflow) fn extract_methodology_lifted(
    content: &str,
) -> MethodologyLifted {
    const KEYWORDS: &[&str] = &[
        "phase",
        "principle",
        "anti-pattern",
        "gate",
        "artifact",
        "authority",
    ];

    let mut lifted = MethodologyLifted::default();
    // (kind, id, body, depth, in_string, escaped, start_line)
    let mut buffer: Option<(String, Option<String>, String, i32, bool, bool, usize)> = None;

    for (line_idx, line) in content.lines().enumerate() {
        if let Some((kind, id, mut body, mut depth, mut in_string, mut escaped, start_line)) =
            buffer.take()
        {
            body.push('\n');
            let mut closed = false;
            for ch in line.chars() {
                body.push(ch);
                advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
                if depth == 0 {
                    push_lifted_form(
                        &mut lifted,
                        &kind,
                        id.clone(),
                        std::mem::take(&mut body),
                        start_line,
                        line_idx,
                    );
                    closed = true;
                    break;
                }
            }
            if !closed && depth > 0 {
                buffer = Some((kind, id, body, depth, in_string, escaped, start_line));
            }
            continue;
        }

        let leading = line.chars().take_while(|c| c.is_whitespace()).count();
        let rest = &line[leading..];
        let Some((kind, after_kind)) = match_form_keyword(rest, KEYWORDS) else {
            continue;
        };
        let after_ws = after_kind.trim_start();
        // Optional id: first non-whitespace, non-paren token. We only treat a
        // bare identifier (no leading `:` or `"`) as an id; keyword args and
        // string payloads stay anonymous so we never accidentally promote
        // `:goal` or `"summary"` into an id slot.
        let id = parse_optional_form_id(after_ws);

        let mut depth: i32 = 0;
        let mut in_string = false;
        let mut escaped = false;
        let mut body = String::new();
        let mut closed = false;
        for ch in rest.chars() {
            body.push(ch);
            advance_paren_state(ch, &mut depth, &mut in_string, &mut escaped);
            if depth == 0 && body.ends_with(')') {
                push_lifted_form(
                    &mut lifted,
                    kind,
                    id.clone(),
                    body.clone(),
                    line_idx,
                    line_idx,
                );
                closed = true;
                break;
            }
        }
        if !closed && depth > 0 {
            buffer = Some((
                kind.to_string(),
                id,
                body,
                depth,
                in_string,
                escaped,
                line_idx,
            ));
        }
    }

    lifted
}

/// Match a known form keyword at the start of a (whitespace-trimmed) line.
/// Returns `Some((keyword, remainder))` only when the keyword is followed by
/// whitespace or `)` — i.e. `(phase` matches, but `(phases` and `(phaseA`
/// do not. This is the same disambiguation rule [`extract_steps`] uses for
/// `(step` vs `(steps`.
pub(in crate::handlers::knowledge::workflow) fn match_form_keyword<'a>(
    rest: &'a str,
    keywords: &[&'static str],
) -> Option<(&'static str, &'a str)> {
    if !rest.starts_with('(') {
        return None;
    }
    for kw in keywords {
        let prefix = format!("({}", kw);
        if !rest.starts_with(&prefix) {
            continue;
        }
        let after = &rest[prefix.len()..];
        let next = after.chars().next();
        match next {
            None => return Some((*kw, after)),
            Some(c) if c.is_whitespace() || c == ')' => return Some((*kw, after)),
            _ => continue,
        }
    }
    None
}

/// Treat the first whitespace/paren-delimited token as the form id, but only
/// when it looks like a bare identifier (no leading `:` keyword arg, no
/// leading quote, and at least one ASCII alphanumeric / `-` / `_` char).
/// Anything else stays anonymous — we'd rather lose an id than fabricate
/// one from a string payload or keyword arg.
pub(in crate::handlers::knowledge::workflow) fn parse_optional_form_id(
    after_ws: &str,
) -> Option<String> {
    let token_end = after_ws
        .find(|c: char| c.is_whitespace() || c == ')')
        .unwrap_or(after_ws.len());
    let token = after_ws[..token_end].trim();
    if token.is_empty() {
        return None;
    }
    let first = token.chars().next()?;
    if first == ':' || first == '"' || first == '(' {
        return None;
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/' || c == '.')
    {
        return None;
    }
    Some(token.to_string())
}

fn push_lifted_form(
    lifted: &mut MethodologyLifted,
    kind: &str,
    id: Option<String>,
    body: String,
    start_line: usize,
    end_line: usize,
) {
    match kind {
        "phase" => lifted.phases.push(MethodologyPhase {
            id,
            body,
            start_line,
            end_line,
        }),
        "principle" => lifted.principles.push(MethodologyForm {
            kind: kind.to_string(),
            id,
            body,
            start_line,
        }),
        "anti-pattern" => lifted.anti_patterns.push(MethodologyForm {
            kind: kind.to_string(),
            id,
            body,
            start_line,
        }),
        "gate" => lifted.gates.push(MethodologyForm {
            kind: kind.to_string(),
            id,
            body,
            start_line,
        }),
        "artifact" => lifted.artifacts.push(MethodologyForm {
            kind: kind.to_string(),
            id,
            body,
            start_line,
        }),
        "authority" => lifted.authorities.push(MethodologyForm {
            kind: kind.to_string(),
            id,
            body,
            start_line,
        }),
        _ => {} // unknown keyword: silently ignore (defensive — kept for forward-compat)
    }
}

/// Resolve which phase (if any) a step's line falls inside. Returns the
/// phase's effective id — explicit when authored, else a stable
/// `phase_<line>` token so YAML keys stay distinct. `None` means the step
/// lives outside any recognised phase form.
pub(in crate::handlers::knowledge::workflow) fn phase_id_for_step(
    phases: &[MethodologyPhase],
    step_line: usize,
) -> Option<String> {
    for ph in phases {
        if step_line >= ph.start_line && step_line <= ph.end_line {
            return Some(
                ph.id
                    .clone()
                    .unwrap_or_else(|| format!("phase_{}", ph.start_line)),
            );
        }
    }
    None
}

fn advance_paren_state(ch: char, depth: &mut i32, in_string: &mut bool, escaped: &mut bool) {
    if *in_string {
        if *escaped {
            *escaped = false;
        } else if ch == '\\' {
            *escaped = true;
        } else if ch == '"' {
            *in_string = false;
        }
        return;
    }
    match ch {
        '"' => *in_string = true,
        '(' => *depth += 1,
        ')' => *depth -= 1,
        _ => {}
    }
}
