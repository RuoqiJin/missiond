/// Parsed envelope of a wave-14 deterministic review-question id. Layout:
///   `review:<scope>:<artifact_id>:v<version>:<action>[:<topic-hash>]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedReviewQuestionId {
    pub(crate) scope: String,
    pub(crate) artifact_id: String,
    pub(crate) version: i32,
    pub(crate) action: String,
    /// Optional 16-hex-char topic hash. Wave-14 layout suffix; wave-11
    /// layout has no suffix and this field stays None.
    pub(crate) topic_hash: Option<String>,
}

/// Errors returned by [`parse_review_question_id_struct`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReviewIdParseError {
    /// id does not start with the literal `review:` prefix.
    MissingPrefix,
    /// id is missing one of the mandatory `<scope>:<id>:v<version>:<action>`
    /// segments.
    InsufficientSegments,
    /// Mandatory segment is empty (e.g. `review::abc:v1:compile`).
    EmptySegment(&'static str),
    /// `v<version>` segment is malformed (missing `v` prefix or non-numeric
    /// version).
    BadVersion(String),
}

impl ReviewIdParseError {
    pub(crate) fn message(&self) -> String {
        match self {
            ReviewIdParseError::MissingPrefix => {
                "review_question_id must start with `review:` prefix".to_string()
            }
            ReviewIdParseError::InsufficientSegments => {
                "review_question_id must look like `review:<scope>:<id>:v<version>:<action>[:<topic-hash>]`"
                    .to_string()
            }
            ReviewIdParseError::EmptySegment(seg) => {
                format!("review_question_id has empty `{}` segment", seg)
            }
            ReviewIdParseError::BadVersion(raw) => format!(
                "review_question_id version segment `{}` must be `v<int>` (e.g. `v1`)",
                raw
            ),
        }
    }
}

/// Parse a wave-14 deterministic review-question id back into its parts.
/// Pure / side-effect free; does not consult the DB.
///
/// Recognised shapes:
///   `review:<scope>:<artifact_id>:v<version>:<action>`
///   `review:<scope>:<artifact_id>:v<version>:<action>:<topic-hash>`
pub(crate) fn parse_review_question_id_struct(
    qid: &str,
) -> Result<ParsedReviewQuestionId, ReviewIdParseError> {
    let trimmed = qid.trim();
    let body = trimmed
        .strip_prefix("review:")
        .ok_or(ReviewIdParseError::MissingPrefix)?;
    let segs: Vec<&str> = body.split(':').collect();
    if segs.len() < 4 {
        return Err(ReviewIdParseError::InsufficientSegments);
    }
    if segs.len() > 5 {
        // We don't accept `v<v>:<action>:<hash>:<extra>` — extra colons inside
        // the action / hash were never produced by wave-14.
        return Err(ReviewIdParseError::InsufficientSegments);
    }
    let scope = segs[0].trim();
    if scope.is_empty() {
        return Err(ReviewIdParseError::EmptySegment("scope"));
    }
    let artifact_id = segs[1].trim();
    if artifact_id.is_empty() {
        return Err(ReviewIdParseError::EmptySegment("artifact_id"));
    }
    let version_seg = segs[2].trim();
    let version_num = version_seg
        .strip_prefix('v')
        .ok_or_else(|| ReviewIdParseError::BadVersion(version_seg.to_string()))?;
    let version: i32 = version_num
        .parse()
        .map_err(|_| ReviewIdParseError::BadVersion(version_seg.to_string()))?;
    let action = segs[3].trim();
    if action.is_empty() {
        return Err(ReviewIdParseError::EmptySegment("action"));
    }
    let topic_hash = if segs.len() == 5 {
        let hash = segs[4].trim();
        if hash.is_empty() {
            return Err(ReviewIdParseError::EmptySegment("topic_hash"));
        }
        Some(hash.to_string())
    } else {
        None
    };
    Ok(ParsedReviewQuestionId {
        scope: scope.to_string(),
        artifact_id: artifact_id.to_string(),
        version,
        action: action.to_ascii_lowercase(),
        topic_hash,
    })
}

/// Errors returned by [`validate_review_resolution_envelope`]. Each
/// variant maps to an MCP `ToolError` code via [`Self::code`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolutionValidationError {
    /// The parsed id's scope does not match the manager surface (e.g. a
    /// `directive` id submitted to the plan handler).
    ScopeMismatch {
        expected: &'static str,
        actual: String,
    },
    /// The parsed id's artifact_id does not match the action's artifact
    /// (e.g. `directive_id=abc` but qid encodes `xyz`).
    ArtifactIdMismatch { expected: String, actual: String },
    /// The parsed id's version does not match the artifact's current
    /// version (or the version the caller passed for transition).
    StaleVersion {
        expected: i32,
        actual_in_id: i32,
        artifact_id: String,
    },
    /// The parsed id's action is not in the action whitelist for this
    /// manager surface (e.g. `:supersede` arrived at the directive
    /// handler).
    UnsupportedAction {
        scope: &'static str,
        action: String,
        allowed: &'static [&'static str],
    },
    /// The parsed id's scope is not one of the wave-14 surfaces (i.e. not
    /// directive/plan/workflow). Defensive — shouldn't happen for ids we
    /// produced, but keeps malformed third-party ids loud.
    UnsupportedScope { scope: String },
}

impl ResolutionValidationError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            ResolutionValidationError::StaleVersion { .. } => "STALE_REVIEW_VERSION",
            ResolutionValidationError::ScopeMismatch { .. } => "REVIEW_SCOPE_MISMATCH",
            ResolutionValidationError::ArtifactIdMismatch { .. } => "REVIEW_ARTIFACT_MISMATCH",
            ResolutionValidationError::UnsupportedAction { .. } => "REVIEW_ACTION_UNSUPPORTED",
            ResolutionValidationError::UnsupportedScope { .. } => "REVIEW_SCOPE_UNSUPPORTED",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            ResolutionValidationError::ScopeMismatch { expected, actual } => format!(
                "review_question_id scope `{}` does not match manager surface `{}`",
                actual, expected
            ),
            ResolutionValidationError::ArtifactIdMismatch { expected, actual } => format!(
                "review_question_id artifact `{}` does not match request artifact `{}`",
                actual, expected
            ),
            ResolutionValidationError::StaleVersion {
                expected,
                actual_in_id,
                artifact_id,
            } => format!(
                "review_question_id targets version `v{}` but artifact `{}` is at version `v{}`",
                actual_in_id, artifact_id, expected
            ),
            ResolutionValidationError::UnsupportedAction {
                scope,
                action,
                allowed,
            } => format!(
                "review_question_id action `{}` is not allowed on scope `{}` (valid: {})",
                action,
                scope,
                allowed.join("|")
            ),
            ResolutionValidationError::UnsupportedScope { scope } => format!(
                "review_question_id scope `{}` is not supported (valid: directive|plan|workflow)",
                scope
            ),
        }
    }
}

/// All wave-14 surfaces that we accept review-question ids for. Defensive
/// check — wave-15 handler-side validators repeat the per-surface match
/// (so they can pin the action whitelist), but this guards against
/// third-party ids whose scope is not one we ever produced.
pub(crate) const WAVE14_SUPPORTED_SCOPES: &[&str] = &["directive", "plan", "workflow"];

/// Validate a parsed review-question id against the manager surface that
/// received it. Pure / side-effect free; does not consult the DB. The
/// caller is responsible for sourcing `current_artifact_version` (e.g.
/// from `directive_get_version_chain` head) before invoking this.
///
/// `allowed_actions` is the manager-side action whitelist (e.g.
/// `&["compile", "approve", "archive"]` for the directive surface). The
/// id's `action` must be in this list — we refuse to resolve a directive
/// surface against a `:supersede` qid even if other validators pass.
pub(crate) fn validate_review_resolution_envelope(
    parsed: &ParsedReviewQuestionId,
    expected_scope: &'static str,
    expected_artifact_id: &str,
    current_artifact_version: i32,
    allowed_actions: &'static [&'static str],
) -> Result<(), ResolutionValidationError> {
    if !WAVE14_SUPPORTED_SCOPES.contains(&parsed.scope.as_str()) {
        return Err(ResolutionValidationError::UnsupportedScope {
            scope: parsed.scope.clone(),
        });
    }
    if parsed.scope != expected_scope {
        return Err(ResolutionValidationError::ScopeMismatch {
            expected: expected_scope,
            actual: parsed.scope.clone(),
        });
    }
    if parsed.artifact_id != expected_artifact_id {
        return Err(ResolutionValidationError::ArtifactIdMismatch {
            expected: expected_artifact_id.to_string(),
            actual: parsed.artifact_id.clone(),
        });
    }
    if parsed.version != current_artifact_version {
        return Err(ResolutionValidationError::StaleVersion {
            expected: current_artifact_version,
            actual_in_id: parsed.version,
            artifact_id: parsed.artifact_id.clone(),
        });
    }
    if !allowed_actions.contains(&parsed.action.as_str()) {
        return Err(ResolutionValidationError::UnsupportedAction {
            scope: expected_scope,
            action: parsed.action.clone(),
            allowed: allowed_actions,
        });
    }
    Ok(())
}
