use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::distill::paren_balanced_ignoring_strings;
use super::{COMPILER_VERSION, GENERATED_FLOWS_DIR, WORKFLOWS_DIR};

// ───────────────────────────────────────────────────────────────────────
// helpers — methodology compiler v0 (pure, covered by unit tests)
// ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MethodologyStep {
    pub(super) id: String,
    pub(super) body: String,
}

/// One of the higher-order methodology forms the v0 lifter recognises but
/// never converts into an executable node. The compiler stays conservative:
/// the form's raw body is preserved verbatim under
/// `methodology_metadata` in the generated YAML so downstream readers
/// (manual reviewer, future forge compiler, audit trace) can recover the
/// original semantics. Only `(step …)` forms turn into nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MethodologyForm {
    /// Form keyword as it appears in source, e.g. `principle`, `anti-pattern`.
    pub(super) kind: String,
    /// First whitespace-delimited token after the keyword, treated as an
    /// optional id (e.g. `(principle no-fallback …)` → Some("no-fallback")).
    /// Forms without a leading identifier (or a malformed one) keep `None`
    /// — we never invent ids the source did not author.
    pub(super) id: Option<String>,
    /// Verbatim source slice of the form, parens included. Multi-line bodies
    /// preserve their original whitespace so reviewers see the methodology
    /// exactly as authored.
    pub(super) body: String,
    /// 0-based line at which the opening `(` was emitted in the source.
    pub(super) start_line: usize,
}

/// A `(phase …)` form with the steps the v0 lifter found nested under it.
/// Steps inside a phase are STILL emitted as top-level executable nodes by
/// the YAML builder, but each carries `methodology_metadata.phase_id` so a
/// manual reviewer can rejoin the narrative with the executable plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MethodologyPhase {
    /// Phase id (token after `(phase `). Anonymous phases keep `None` and
    /// surface in metadata as `phase_<line>` so YAML keys stay distinct.
    pub(super) id: Option<String>,
    /// Verbatim source slice including parens.
    pub(super) body: String,
    /// Inclusive 0-based line range covered by the phase form. Used to
    /// associate inner steps without requiring a recursive parser.
    pub(super) start_line: usize,
    pub(super) end_line: usize,
}

/// Aggregate result of the v0 semantic lifter — produced by
/// [`extract_methodology_lifted`] and consumed by [`build_generated_yaml`].
/// All vectors preserve source order so the generated YAML reads top-to-
/// bottom against the methodology Lisp.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct MethodologyLifted {
    pub(super) phases: Vec<MethodologyPhase>,
    pub(super) principles: Vec<MethodologyForm>,
    pub(super) anti_patterns: Vec<MethodologyForm>,
    pub(super) gates: Vec<MethodologyForm>,
    pub(super) artifacts: Vec<MethodologyForm>,
    pub(super) authorities: Vec<MethodologyForm>,
}

impl MethodologyLifted {
    pub(super) fn is_empty(&self) -> bool {
        self.phases.is_empty()
            && self.principles.is_empty()
            && self.anti_patterns.is_empty()
            && self.gates.is_empty()
            && self.artifacts.is_empty()
            && self.authorities.is_empty()
    }

    /// Total count of all lifted forms across every category — used by the
    /// dry-run preview and the deterministic-mode payload to surface a single
    /// `lifted_form_count` figure for callers.
    pub(super) fn total_count(&self) -> usize {
        self.phases.len()
            + self.principles.len()
            + self.anti_patterns.len()
            + self.gates.len()
            + self.artifacts.len()
            + self.authorities.len()
    }
}

/// Step keyed by its 0-based starting line, used internally by
/// [`build_generated_yaml`] to attach `phase_id` metadata when a step's line
/// falls inside a phase form's `start_line..=end_line` range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocatedStep {
    pub(super) step: MethodologyStep,
    pub(super) start_line: usize,
}

#[derive(Debug, Clone)]
pub(super) struct GeneratedMeta {
    pub(super) flow_id: String,
    pub(super) name: String,
    pub(super) source_path: String,
    pub(super) source_hash: String,
    pub(super) generated_at: String,
    pub(super) compiler_status: String,
}

#[derive(Debug)]
pub(super) enum CompiledFlowError {
    MissingArgs,
    Missing { flow_id: String, expected: PathBuf },
}

#[derive(Debug, Clone)]
pub(super) struct CompiledFlow {
    pub(super) path: PathBuf,
}

pub(super) fn resolve_methodology_path(
    project_root: &Path,
    name: Option<&str>,
    workflow_path: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(p) = workflow_path.filter(|s| !s.is_empty()) {
        let candidate = PathBuf::from(p);
        return Ok(if candidate.is_absolute() {
            candidate
        } else {
            project_root.join(candidate)
        });
    }
    if let Some(name) = name.filter(|s| !s.is_empty()) {
        let mut p = project_root.join(WORKFLOWS_DIR).join(name);
        if p.extension().is_none() {
            p.set_extension("lisp");
        }
        return Ok(p);
    }
    Err("compile_methodology requires `workflow_path` or `name`".to_string())
}

pub(super) fn validate_methodology_source(content: &str) -> Result<(), String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("methodology source is empty".to_string());
    }
    if !paren_balanced_ignoring_strings(content) {
        return Err("methodology source has unbalanced parentheses".to_string());
    }
    if !content.chars().any(|c| c == '(') {
        return Err("methodology source has no top-level form".to_string());
    }
    Ok(())
}

/// SHA-256 hex of the source bytes — stable across runs for identical input.
pub(super) fn source_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for byte in digest {
        s.push_str(&format!("{:02x}", byte));
    }
    s
}

pub(super) fn derive_flow_id(stem: &str, output_flow_id: Option<&str>) -> String {
    if let Some(explicit) = output_flow_id.filter(|s| !s.is_empty()) {
        return explicit.to_string();
    }
    let safe = sanitize_id_token(stem);
    if safe.is_empty() {
        "methodology-anonymous-v0".to_string()
    } else {
        format!("methodology-{}-v0", safe)
    }
}

pub(super) fn sanitize_id_token(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_hyphen = false;
    for ch in raw.chars() {
        let allowed = ch.is_ascii_alphanumeric() || ch == '_' || ch == '-';
        if allowed {
            out.push(ch);
            prev_hyphen = ch == '-';
        } else if !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub(super) fn source_path_for_yaml(project_root: &Path, path: &Path) -> String {
    match path.strip_prefix(project_root) {
        Ok(rel) => rel.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

pub(super) fn generated_yaml_path(project_root: &Path, flow_id: &str) -> PathBuf {
    project_root
        .join(GENERATED_FLOWS_DIR)
        .join(format!("{}.yaml", flow_id))
}

/// Extract `(step <id> <body…>)` forms from a methodology Lisp source. Multi-line
/// bodies are accumulated using paren depth tracking that ignores string contents.
/// Pure-fn for testing.
pub(super) fn extract_steps(content: &str) -> Vec<MethodologyStep> {
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
pub(super) fn extract_steps_with_lines(content: &str) -> Vec<LocatedStep> {
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
pub(super) fn extract_methodology_lifted(content: &str) -> MethodologyLifted {
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
pub(super) fn match_form_keyword<'a>(
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
pub(super) fn parse_optional_form_id(after_ws: &str) -> Option<String> {
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
pub(super) fn phase_id_for_step(phases: &[MethodologyPhase], step_line: usize) -> Option<String> {
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

pub(super) fn build_generated_yaml(
    meta: &GeneratedMeta,
    steps: &[LocatedStep],
    lifted: &MethodologyLifted,
    review_required: bool,
) -> Result<String, serde_yaml::Error> {
    use serde_yaml::{Mapping, Value as Yaml};

    let mut root = Mapping::new();
    root.insert(Yaml::from("id"), Yaml::from(meta.flow_id.clone()));
    root.insert(Yaml::from("name"), Yaml::from(meta.name.clone()));
    root.insert(Yaml::from("source_kind"), Yaml::from("methodology_lisp"));
    root.insert(
        Yaml::from("source_path"),
        Yaml::from(meta.source_path.clone()),
    );
    root.insert(
        Yaml::from("source_hash"),
        Yaml::from(meta.source_hash.clone()),
    );
    root.insert(Yaml::from("generated_by"), Yaml::from(COMPILER_VERSION));
    root.insert(
        Yaml::from("generated_at"),
        Yaml::from(meta.generated_at.clone()),
    );
    root.insert(
        Yaml::from("compiler_status"),
        Yaml::from(meta.compiler_status.clone()),
    );
    root.insert(Yaml::from("review_required"), Yaml::from(review_required));

    // Lifted higher-order semantics — emitted under a top-level
    // `methodology_metadata` mapping. `FlowDefinition` does NOT declare this
    // field, so serde_yaml ignores it during loader deserialisation while
    // the raw YAML still preserves it for human reviewers and the future
    // forge compiler. Keeping this strictly out-of-band is what lets the
    // v0 lifter stay conservative — no execution semantics change.
    if !lifted.is_empty() {
        root.insert(
            Yaml::from("methodology_metadata"),
            Yaml::Mapping(build_methodology_metadata_yaml(lifted)),
        );
    }

    let mut nodes_seq: Vec<Yaml> = Vec::new();
    if steps.is_empty() {
        let mut node = Mapping::new();
        node.insert(Yaml::from("id"), Yaml::from("manual_review"));
        node.insert(Yaml::from("type"), Yaml::from("slot_task"));
        node.insert(Yaml::from("model"), Yaml::from("opus"));
        node.insert(
            Yaml::from("prompt"),
            Yaml::from(build_manual_review_prompt(meta, lifted)),
        );
        // Mirror the lifted metadata onto the manual_review node itself so
        // the reviewer sees it without having to walk back to the YAML
        // root. The flattened FlowNode/NodeType serde shape ignores
        // unknown keys, so this is a pure documentation channel.
        if !lifted.is_empty() {
            node.insert(
                Yaml::from("methodology_metadata"),
                Yaml::Mapping(build_methodology_metadata_yaml(lifted)),
            );
        }
        nodes_seq.push(Yaml::Mapping(node));
    } else {
        for step in steps {
            let safe_id = sanitize_id_token(&step.step.id);
            let node_id = if safe_id.is_empty() {
                "step".to_string()
            } else {
                format!("step_{}", safe_id)
            };
            let mut node = Mapping::new();
            node.insert(Yaml::from("id"), Yaml::from(node_id.clone()));
            node.insert(Yaml::from("type"), Yaml::from("slot_task"));
            node.insert(Yaml::from("model"), Yaml::from("opus"));
            node.insert(Yaml::from("prompt"), Yaml::from(step.step.body.clone()));
            node.insert(
                Yaml::from("save_as"),
                Yaml::from(format!("{}_result", node_id)),
            );
            // Per-node `methodology_metadata.phase_id` carries the v0
            // lifter's phase association. FlowNode flattens NodeType (which
            // has `tag = "type"`); serde_yaml's default unknown-field
            // tolerance lets us attach this without affecting the
            // executable shape — verified by the YAML round-trip test.
            if let Some(phase_id) = phase_id_for_step(&lifted.phases, step.start_line) {
                let mut node_meta = Mapping::new();
                node_meta.insert(Yaml::from("phase_id"), Yaml::from(phase_id));
                node.insert(Yaml::from("methodology_metadata"), Yaml::Mapping(node_meta));
            }
            nodes_seq.push(Yaml::Mapping(node));
        }
    }
    root.insert(Yaml::from("nodes"), Yaml::Sequence(nodes_seq));
    serde_yaml::to_string(&Yaml::Mapping(root))
}

/// Build the prompt body for the `manual_review` fallback node. When the
/// v0 lifter recovered higher-order forms, surface them in the prompt so
/// the reviewer immediately sees what the methodology declared even before
/// touching the metadata mapping.
pub(super) fn build_manual_review_prompt(
    meta: &GeneratedMeta,
    lifted: &MethodologyLifted,
) -> String {
    let base = format!(
        "Manually review compiled methodology '{flow}' before running.\n\
         Source: {src}\n\
         Source hash: {hash}\n\
         The deterministic compiler v0 could not auto-extract executable (step …) forms.\n\
         Edit this YAML or augment the source Lisp before dispatching.",
        flow = meta.flow_id,
        src = meta.source_path,
        hash = meta.source_hash,
    );
    if lifted.is_empty() {
        return base;
    }
    let mut out = base;
    out.push_str("\n\nLifted methodology semantics (v0 recognised, NOT executable):");
    if !lifted.phases.is_empty() {
        out.push_str(&format!("\n  - phases: {}", lifted.phases.len()));
    }
    if !lifted.principles.is_empty() {
        out.push_str(&format!("\n  - principles: {}", lifted.principles.len()));
    }
    if !lifted.anti_patterns.is_empty() {
        out.push_str(&format!(
            "\n  - anti-patterns: {}",
            lifted.anti_patterns.len()
        ));
    }
    if !lifted.gates.is_empty() {
        out.push_str(&format!("\n  - gates: {}", lifted.gates.len()));
    }
    if !lifted.artifacts.is_empty() {
        out.push_str(&format!("\n  - artifacts: {}", lifted.artifacts.len()));
    }
    if !lifted.authorities.is_empty() {
        out.push_str(&format!("\n  - authorities: {}", lifted.authorities.len()));
    }
    out.push_str("\nSee the `methodology_metadata` mapping at the YAML root for raw bodies.");
    out
}

/// Produce the YAML representation of the lifted methodology forms.
/// Each category is a sequence of `{kind, id?, body, start_line}` entries
/// (or `{id?, body, start_line, end_line}` for phases). Bodies are kept
/// verbatim so reviewers and the future forge compiler can recover the
/// exact source spelling.
fn build_methodology_metadata_yaml(lifted: &MethodologyLifted) -> serde_yaml::Mapping {
    use serde_yaml::{Mapping, Value as Yaml};

    fn form_to_yaml(form: &MethodologyForm) -> Yaml {
        let mut m = Mapping::new();
        m.insert(Yaml::from("kind"), Yaml::from(form.kind.clone()));
        if let Some(id) = &form.id {
            m.insert(Yaml::from("id"), Yaml::from(id.clone()));
        }
        m.insert(Yaml::from("body"), Yaml::from(form.body.clone()));
        m.insert(Yaml::from("start_line"), Yaml::from(form.start_line as u64));
        Yaml::Mapping(m)
    }

    let mut root = Mapping::new();
    if !lifted.phases.is_empty() {
        let phases_seq: Vec<Yaml> = lifted
            .phases
            .iter()
            .map(|ph| {
                let mut m = Mapping::new();
                m.insert(Yaml::from("kind"), Yaml::from("phase"));
                if let Some(id) = &ph.id {
                    m.insert(Yaml::from("id"), Yaml::from(id.clone()));
                }
                m.insert(Yaml::from("body"), Yaml::from(ph.body.clone()));
                m.insert(Yaml::from("start_line"), Yaml::from(ph.start_line as u64));
                m.insert(Yaml::from("end_line"), Yaml::from(ph.end_line as u64));
                Yaml::Mapping(m)
            })
            .collect();
        root.insert(Yaml::from("phases"), Yaml::Sequence(phases_seq));
    }
    if !lifted.principles.is_empty() {
        root.insert(
            Yaml::from("principles"),
            Yaml::Sequence(lifted.principles.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.anti_patterns.is_empty() {
        root.insert(
            Yaml::from("anti_patterns"),
            Yaml::Sequence(lifted.anti_patterns.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.gates.is_empty() {
        root.insert(
            Yaml::from("gates"),
            Yaml::Sequence(lifted.gates.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.artifacts.is_empty() {
        root.insert(
            Yaml::from("artifacts"),
            Yaml::Sequence(lifted.artifacts.iter().map(form_to_yaml).collect()),
        );
    }
    if !lifted.authorities.is_empty() {
        root.insert(
            Yaml::from("authorities"),
            Yaml::Sequence(lifted.authorities.iter().map(form_to_yaml).collect()),
        );
    }
    root
}

/// Per-process monotonic counter feeding [`unique_generated_yaml_temp_path`]
/// so two writers landing on the same generated YAML inside the same nanosecond
/// (or on a coarse-clock filesystem) still receive distinct temp file names.
static GENERATED_YAML_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Build a per-attempt temp file path that lives in the same directory as
/// `target` so the subsequent `rename` stays atomic on POSIX (same-FS).
///
/// Layout: `<leaf>.tmp.<pid>.<unix_nanos>.<seq>`. The fixed legacy extension
/// (a literal kept only as a regression marker — see the `static_temp` test
/// suffix below) is deliberately avoided because two concurrent
/// compile_methodology writers on the same `flow_id` would otherwise share
/// one temp path and corrupt each other's output before the rename.
///
/// This is a workflow.rs-local mirror of the unique-temp helper that
/// `file_artifacts` will eventually expose; we keep it private here until that
/// foundation crate publishes a stable surface (referenced by Task 4b — once
/// the shared `unique_temp_path_in_dir(target: &Path) -> PathBuf` lands, both
/// callers should converge on it).
pub(super) fn unique_generated_yaml_temp_path(target: &Path) -> PathBuf {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let leaf = target
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "anonymous".to_string());
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = GENERATED_YAML_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    parent.join(format!("{leaf}.tmp.{pid}.{nanos}.{seq}"))
}

/// Atomic write for the methodology compiler's generated YAML target.
///
/// Behavior:
///   - Auto-creates parent directories.
///   - Writes to a per-attempt unique temp file in the same directory so
///     concurrent compile_methodology calls on the same `flow_id` cannot
///     trample each other's temp file (rename remains same-FS atomic).
///   - On either write or rename failure, removes ONLY this attempt's temp
///     file (path-specific cleanup) and propagates the underlying IO error.
///     The cleanup is `let _ =` because the propagated error is the real
///     signal — silent retries would mask the root cause.
pub(super) fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = unique_generated_yaml_temp_path(path);
    if let Err(e) = std::fs::write(&tmp, content) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

pub(super) fn resolve_compiled_flow(
    project_root: &Path,
    flow_id: Option<&str>,
    flow_path: Option<&str>,
    name: Option<&str>,
) -> Result<CompiledFlow, CompiledFlowError> {
    if let Some(p) = flow_path.filter(|s| !s.is_empty()) {
        let candidate = PathBuf::from(p);
        let resolved = if candidate.is_absolute() {
            candidate
        } else {
            project_root.join(candidate)
        };
        if resolved.exists() {
            return Ok(CompiledFlow { path: resolved });
        }
        let id_for_msg = flow_id.map(|s| s.to_string()).unwrap_or_else(|| {
            resolved
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string()
        });
        return Err(CompiledFlowError::Missing {
            flow_id: id_for_msg,
            expected: resolved,
        });
    }

    let id = if let Some(id) = flow_id.filter(|s| !s.is_empty()) {
        id.to_string()
    } else if let Some(n) = name.filter(|s| !s.is_empty()) {
        derive_flow_id(n, None)
    } else {
        return Err(CompiledFlowError::MissingArgs);
    };
    let expected = generated_yaml_path(project_root, &id);
    if expected.exists() {
        Ok(CompiledFlow { path: expected })
    } else {
        Err(CompiledFlowError::Missing {
            flow_id: id,
            expected,
        })
    }
}
