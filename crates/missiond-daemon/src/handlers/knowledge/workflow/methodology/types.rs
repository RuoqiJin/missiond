use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::knowledge::workflow) struct MethodologyStep {
    pub(in crate::handlers::knowledge::workflow) id: String,
    pub(in crate::handlers::knowledge::workflow) body: String,
}

/// One of the higher-order methodology forms the v0 lifter recognises but
/// never converts into an executable node. The compiler stays conservative:
/// the form's raw body is preserved verbatim under
/// `methodology_metadata` in the generated YAML so downstream readers
/// (manual reviewer, future forge compiler, audit trace) can recover the
/// original semantics. Only `(step …)` forms turn into nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::knowledge::workflow) struct MethodologyForm {
    /// Form keyword as it appears in source, e.g. `principle`, `anti-pattern`.
    pub(in crate::handlers::knowledge::workflow) kind: String,
    /// First whitespace-delimited token after the keyword, treated as an
    /// optional id (e.g. `(principle no-fallback …)` → Some("no-fallback")).
    /// Forms without a leading identifier (or a malformed one) keep `None`
    /// — we never invent ids the source did not author.
    pub(in crate::handlers::knowledge::workflow) id: Option<String>,
    /// Verbatim source slice of the form, parens included. Multi-line bodies
    /// preserve their original whitespace so reviewers see the methodology
    /// exactly as authored.
    pub(in crate::handlers::knowledge::workflow) body: String,
    /// 0-based line at which the opening `(` was emitted in the source.
    pub(in crate::handlers::knowledge::workflow) start_line: usize,
}

/// A `(phase …)` form with the steps the v0 lifter found nested under it.
/// Steps inside a phase are STILL emitted as top-level executable nodes by
/// the YAML builder, but each carries `methodology_metadata.phase_id` so a
/// manual reviewer can rejoin the narrative with the executable plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::knowledge::workflow) struct MethodologyPhase {
    /// Phase id (token after `(phase `). Anonymous phases keep `None` and
    /// surface in metadata as `phase_<line>` so YAML keys stay distinct.
    pub(in crate::handlers::knowledge::workflow) id: Option<String>,
    /// Verbatim source slice including parens.
    pub(in crate::handlers::knowledge::workflow) body: String,
    /// Inclusive 0-based line range covered by the phase form. Used to
    /// associate inner steps without requiring a recursive parser.
    pub(in crate::handlers::knowledge::workflow) start_line: usize,
    pub(in crate::handlers::knowledge::workflow) end_line: usize,
}

/// Aggregate result of the v0 semantic lifter — produced by
/// [`extract_methodology_lifted`] and consumed by [`build_generated_yaml`].
/// All vectors preserve source order so the generated YAML reads top-to-
/// bottom against the methodology Lisp.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::handlers::knowledge::workflow) struct MethodologyLifted {
    pub(in crate::handlers::knowledge::workflow) phases: Vec<MethodologyPhase>,
    pub(in crate::handlers::knowledge::workflow) principles: Vec<MethodologyForm>,
    pub(in crate::handlers::knowledge::workflow) anti_patterns: Vec<MethodologyForm>,
    pub(in crate::handlers::knowledge::workflow) gates: Vec<MethodologyForm>,
    pub(in crate::handlers::knowledge::workflow) artifacts: Vec<MethodologyForm>,
    pub(in crate::handlers::knowledge::workflow) authorities: Vec<MethodologyForm>,
}

impl MethodologyLifted {
    pub(in crate::handlers::knowledge::workflow) fn is_empty(&self) -> bool {
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
    pub(in crate::handlers::knowledge::workflow) fn total_count(&self) -> usize {
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
pub(in crate::handlers::knowledge::workflow) struct LocatedStep {
    pub(in crate::handlers::knowledge::workflow) step: MethodologyStep,
    pub(in crate::handlers::knowledge::workflow) start_line: usize,
}

#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::workflow) struct GeneratedMeta {
    pub(in crate::handlers::knowledge::workflow) flow_id: String,
    pub(in crate::handlers::knowledge::workflow) name: String,
    pub(in crate::handlers::knowledge::workflow) source_path: String,
    pub(in crate::handlers::knowledge::workflow) source_hash: String,
    pub(in crate::handlers::knowledge::workflow) generated_at: String,
    pub(in crate::handlers::knowledge::workflow) compiler_status: String,
}

#[derive(Debug)]
pub(in crate::handlers::knowledge::workflow) enum CompiledFlowError {
    MissingArgs,
    Missing { flow_id: String, expected: PathBuf },
}

#[derive(Debug, Clone)]
pub(in crate::handlers::knowledge::workflow) struct CompiledFlow {
    pub(in crate::handlers::knowledge::workflow) path: PathBuf,
}
