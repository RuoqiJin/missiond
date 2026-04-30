use super::*;

/// Topic segment used when sanitization yields an empty string.
///
/// Mirrors the spirit of `workflow.rs`'s `methodology-anonymous-v0` fallback:
/// we never want a writer to silently land on `.missiond/alignment//…`.
pub(crate) const ANONYMOUS_TOPIC: &str = "anonymous";

/// Three artifact kinds covered by intent-memory.lisp directive-layer
/// file-first-artifacts. Anything outside these three must use a different
/// helper — this module deliberately enumerates them so a typo cannot pick the
/// wrong directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ArtifactKind {
    /// `.missiond/alignment/<topic>/intent-alignment.lisp` — directive /
    /// alignment file-first SSOT (intent-memory.lisp ::
    /// `intent-alignment-artifact`).
    IntentAlignment,
    /// `.missiond/plans/<topic>/PLAN.lisp` — plan file-first SSOT
    /// (intent-memory.lisp :: `plan-file`).
    Plan,
    /// `.missiond/workflows/<topic>.lisp` — workflow methodology file-first
    /// SSOT (intent-memory.lisp :: `workflow-methodology-file`).
    Workflow,
}

impl ArtifactKind {
    /// Human-readable label for diagnostics and logs.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            ArtifactKind::IntentAlignment => "intent-alignment",
            ArtifactKind::Plan => "plan",
            ArtifactKind::Workflow => "workflow",
        }
    }
}

impl fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Caller-facing description of one artifact write request.
///
/// `file_name` is reserved for kinds whose default file name does not apply
/// (currently none — alignment/plan use a fixed file under a per-topic dir;
/// workflow puts the topic itself in the file name). It is wired through so
/// future topics (e.g. multi-version `intent-alignment-v2.lisp`) can opt in
/// without touching `artifact_path` callers.
///
/// Today the directive / plan / workflow writers all construct artifact paths
/// directly via [`artifact_path`]; the spec form remains as a foundation API
/// so future multi-version writers (e.g. `intent-alignment-v2.lisp`) can opt
/// into [`artifact_path_from_spec`] without touching the existing call sites.
#[derive(Debug, Clone)]
#[allow(dead_code)] // foundation API; future multi-version writers may opt into spec form
pub(crate) struct ArtifactSpec {
    pub kind: ArtifactKind,
    pub topic: String,
    pub project_root: PathBuf,
    pub file_name: Option<String>,
}

/// Metadata returned after a successful atomic write.
///
/// `created` and `overwritten` are mutually exclusive:
///   - `created=true,  overwritten=false` — fresh file written.
///   - `created=false, overwritten=true ` — pre-existing file replaced.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct WriteOutcome {
    pub path: PathBuf,
    pub created: bool,
    pub overwritten: bool,
    pub sha256: String,
    pub bytes: u64,
}

/// Metadata snapshot of an existing artifact on disk. Only used for read-only
/// inspection (e.g. drift detection vs DB mirror).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ArtifactMetadata {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

/// Sanitize a topic into a single safe path segment.
///
/// Rules (aligned with `workflow::sanitize_id_token`):
///   - keep ASCII alphanumerics, `_`, `-`.
///   - replace any other run of characters with a single `-`.
///   - trim leading / trailing `-`.
///   - if the result is empty, fall back to [`ANONYMOUS_TOPIC`].
///
/// Stability is contractual: future writers compare topics across runs to
/// detect rewrites of the same file-first SSOT, so the function must remain
/// idempotent (`sanitize(sanitize(x)) == sanitize(x)`).
pub(crate) fn sanitize_topic_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_hyphen = false;
    for ch in input.chars() {
        let allowed = ch.is_ascii_alphanumeric() || ch == '_' || ch == '-';
        if allowed {
            out.push(ch);
            prev_hyphen = ch == '-';
        } else if !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        ANONYMOUS_TOPIC.to_string()
    } else {
        trimmed
    }
}

/// Resolve the canonical artifact path for `(kind, topic)` under
/// `project_root`. The topic is sanitized internally; callers do not need to
/// pre-sanitize. Path layout matches intent-memory.lisp directive-layer
/// file-first-artifacts and is the single authority across the daemon.
pub(crate) fn artifact_path(project_root: &Path, kind: ArtifactKind, topic: &str) -> PathBuf {
    let safe_topic = sanitize_topic_segment(topic);
    match kind {
        ArtifactKind::IntentAlignment => project_root
            .join(".missiond")
            .join("alignment")
            .join(&safe_topic)
            .join("intent-alignment.lisp"),
        ArtifactKind::Plan => project_root
            .join(".missiond")
            .join("plans")
            .join(&safe_topic)
            .join("PLAN.lisp"),
        ArtifactKind::Workflow => project_root
            .join(".missiond")
            .join("workflows")
            .join(format!("{}.lisp", safe_topic)),
    }
}

/// Resolve an artifact path from an [`ArtifactSpec`], honoring an explicit
/// `file_name` override when supplied. Today only workflow can sensibly use
/// the override (its file name embeds the topic); for alignment/plan the
/// override replaces the default leaf file name within the per-topic dir.
#[allow(dead_code)] // foundation API
pub(crate) fn artifact_path_from_spec(spec: &ArtifactSpec) -> PathBuf {
    let default = artifact_path(&spec.project_root, spec.kind, &spec.topic);
    match (&spec.file_name, spec.kind) {
        (Some(name), _) if !name.is_empty() => match default.parent() {
            Some(parent) => parent.join(name),
            None => PathBuf::from(name),
        },
        _ => default,
    }
}
