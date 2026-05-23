//! file_artifacts — file-first SSOT writer for directive / plan / workflow
//! artifacts.
//!
//! Lisp authority:
//!   - intent-flow.lisp ::
//!       F-intent-alignment-plan-execution-loop :: :file-vs-db-contract
//!   - intent-memory.lisp :: directive-layer :: file-first-artifacts
//!   - intent-intent-layer.lisp :: unified-entry-pipeline :: :file-first-ssot
//!
//! Path convention (per intent-memory.lisp directive-layer file-first-artifacts):
//!   - alignment : <project_root>/.missiond/alignment/<topic>/intent-alignment.lisp
//!   - plan      : <project_root>/.missiond/plans/<topic>/PLAN.lisp
//!   - workflow  : <project_root>/.missiond/workflows/<topic>.lisp
//!
//! Layered surface:
//!   - Pure helpers (`artifact_path`, `atomic_write_artifact`,
//!     `unique_temp_path_in_dir`, `read_existing_metadata`,
//!     `sanitize_topic_segment`) — no `AppState`, no DB, no events. Foundation
//!     wave-11 introduced these so other writers can compose them.
//!   - `WriterContext` + `attempt_artifact_write` — the manager-side helper
//!     that resolves `project_root` through the canonical
//!     `slot_orchestrator::project_root::resolve_target_project_root` resolver,
//!     enforces the no-process-cwd-fallback contract
//!     (intent-worker.lisp :: project-root-spawn-cwd), and routes overwrite
//!     policy. Compiler actors (`directive`, `plan`, `workflow`) call this
//!     after their DB mirror commit so the file-first SSOT and the row stay
//!     in sync, with `partial`/`error` semantics surfaced to the caller.
//!
//! `attempt_artifact_write` deliberately does NOT touch DB rows and does NOT
//! publish events — that contract belongs to the calling action so the file
//! write can fail without dragging down the row, and so this module stays
//! easy to unit-test end-to-end against `tempfile`.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::slot_orchestrator::project_root::{resolve_target_project_root, ResolutionError};
use missiond_core::types::SharedProjectRegistry;

mod attempt;
mod commit;
mod kind;
mod write;

#[cfg(test)]
pub(crate) use attempt::AttemptOutcome;
pub(crate) use attempt::{attempt_artifact_write, resolve_writer_project_root, WriterContext};
pub(crate) use commit::{
    recover_artifact_commit_outbox, ArtifactCommitEnvelope, ArtifactCommitEnvelopeInput,
};
pub(crate) use kind::{artifact_path, sanitize_topic_segment, ArtifactKind, WriteOutcome};
#[cfg(test)]
pub(crate) use kind::{artifact_path_from_spec, ArtifactSpec, ANONYMOUS_TOPIC};
#[cfg(test)]
pub(crate) use write::unique_temp_path_in_dir;
pub(crate) use write::{atomic_write_artifact, read_existing_metadata};

#[cfg(test)]
mod tests;
