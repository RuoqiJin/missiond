//! Database error types — anti-corruption layer between sqlx and business logic.
//!
//! PostgreSQL (sqlx) errors convert to `DbError`, so callers only ever see `DbResult<T>`.

/// Database-specific error type that hides the underlying driver.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[cfg(feature = "postgres")]
    #[error("sqlx: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("not found: {entity} {id}")]
    NotFound { entity: &'static str, id: String },

    #[error("constraint: {0}")]
    Constraint(String),

    #[error("EVIDENCE_REQUIRED: task_id={task_id}: BoardTask cannot be marked done until a canonical completed task-result artifact exists")]
    EvidenceRequired { task_id: String },

    #[error("COMPLETION_ARTIFACT_INVALID: {reason}")]
    CompletionArtifactInvalid {
        task_id: Option<String>,
        reason: String,
    },

    #[error("CLAIM_CONFLICT: {scope_kind}:{scope_key} is already held")]
    ClaimConflict {
        scope_kind: String,
        scope_key: String,
        holder: Option<String>,
        lease_expires_at: Option<String>,
    },

    #[error("CAPABILITY_DENIED: operation={operation} scope={scope_kind}:{scope_key}")]
    CapabilityDenied {
        operation: String,
        scope_kind: String,
        scope_key: String,
        reason: String,
    },

    #[error("RUNTIME_METADATA_REQUIRED: task_id={task_id}: runtime_metadata is required for control-plane decisions")]
    RuntimeMetadataRequired { task_id: String },

    #[error("TASK_CONTRACT_REQUIRED: task_id={task_id}: task_contracts row is required for control-plane decisions")]
    TaskContractRequired { task_id: String },

    #[error("SANDBOX_POLICY_UNSUPPORTED: {reason}")]
    SandboxPolicyUnsupported { reason: String },

    #[error("WRITE_SCOPE_VIOLATION: task_id={task_id}: {reason}")]
    WriteScopeViolation { task_id: String, reason: String },

    #[error("FEATURE_DISABLED: {feature}")]
    FeatureDisabled { feature: String },

    #[error("{0}")]
    Other(String),
}

/// Result alias used by all DB methods.
pub type DbResult<T> = Result<T, DbError>;
