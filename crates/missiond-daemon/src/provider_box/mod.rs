//! Interactive provider box runtime skeleton.
//!
//! This module is the single receiving framework for provider CLI operations.
//! Provider-specific terminal key sequences and menu behavior are intentionally
//! left behind `ProviderDriver`; until a driver is taught, methods return typed
//! unsupported/unknown diagnostics.

#![allow(dead_code, unused_imports)]

mod agy_driver;
mod artifact;
mod codex_driver;
mod driver;
mod http_adapter;
mod runtime;
mod types;

pub(crate) use agy_driver::AgyProviderDriver;
pub(crate) use artifact::ProviderBoxArtifactWriter;
pub(crate) use codex_driver::CodexProviderDriver;
pub(crate) use driver::{ProviderDriver, ProviderDriverCapabilities, UnsupportedProviderDriver};
pub(crate) use http_adapter::ProviderBoxHttpAdapter;
pub(crate) use runtime::ProviderInteractionBox;
pub(crate) use types::{
    BoxCommand, ModelSwitchPolicy, ModelSwitchResult, ModelSwitchStatus, ProviderAttachmentRef,
    ProviderBoxDiagnostic, ProviderBoxResult, ProviderBoxStatus, ProviderControlAction,
    ProviderInteractionRequest, ProviderModelCatalog, ProviderModelCatalogEntry,
    ProviderModelUsage, ProviderRouterExport, ProviderUsageSnapshot, ProviderUsageStatus,
    PtyObservation, PtyStepAction, PtyStepRecord, PtyStepVerificationStatus, SingleTurnPolicy,
    TimeoutCancelPolicy, DIAG_AGY_MODEL_CATALOG_UNSUPPORTED, DIAG_MODEL_SWITCH_UNSUPPORTED,
    DIAG_MODEL_SWITCH_UNVERIFIED, DIAG_PROVIDER_BOX_AUTH_REQUIRED,
    DIAG_PROVIDER_BOX_INVALID_REQUEST, DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
    DIAG_PROVIDER_DURABLE_FINAL_MISSING, DIAG_PROVIDER_STATUS_UNAVAILABLE,
    DIAG_PROVIDER_TEXT_ONLY_VIOLATION, DIAG_PROVIDER_TURN_STALLED,
    DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED, DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED,
    DIAG_PURE_TEXT_GUARD_UNSUPPORTED, DIAG_SUBMIT_TURN_UNSUPPORTED, DIAG_USAGE_UNKNOWN,
};
