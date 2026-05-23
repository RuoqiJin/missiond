use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use missiond_core::db::traits::ArtifactCommitOutboxInput;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::state::AppState;

use super::{atomic_write_artifact, read_existing_metadata, WriteOutcome};

#[derive(Debug, Clone)]
pub(crate) struct ArtifactCommitEnvelopeInput {
    pub(crate) operation_key: String,
    pub(crate) surface: String,
    pub(crate) request_id: Option<String>,
    pub(crate) project_id: Option<String>,
    pub(crate) artifact_kind: String,
    pub(crate) artifact_path: PathBuf,
    pub(crate) content: String,
    pub(crate) overwrite: bool,
    pub(crate) db_table: Option<String>,
    pub(crate) db_row_id: Option<String>,
    pub(crate) event_id: Option<String>,
    pub(crate) event_seq: Option<i64>,
    pub(crate) payload: Value,
}

pub(crate) struct ArtifactCommitEnvelope;

impl ArtifactCommitEnvelope {
    pub(crate) async fn commit_text(
        state: &AppState,
        input: ArtifactCommitEnvelopeInput,
    ) -> Result<WriteOutcome> {
        let artifact_sha256 = sha256_hex(&input.content);
        let payload = payload_with_artifact_content(&input.payload, &input.content);
        let outbox_input = ArtifactCommitOutboxInput {
            operation_key: input.operation_key.clone(),
            surface: input.surface.clone(),
            request_id: input.request_id.clone(),
            project_id: input.project_id.clone(),
            artifact_kind: input.artifact_kind.clone(),
            artifact_path: input.artifact_path.to_string_lossy().into_owned(),
            artifact_sha256: Some(artifact_sha256.clone()),
            db_table: input.db_table.clone(),
            db_row_id: input.db_row_id.clone(),
            event_id: input.event_id.clone(),
            event_seq: input.event_seq,
            payload,
        };
        let row = state
            .store
            .artifact_commit_outbox_upsert_pending(&outbox_input)
            .await
            .map_err(|e| anyhow!("artifact commit outbox upsert failed: {e}"))?;
        if row.status == "complete" {
            return complete_row_outcome(&input.artifact_path, &artifact_sha256).with_context(
                || {
                    format!(
                        "artifact commit {} is complete but invalid",
                        row.operation_key
                    )
                },
            );
        }

        let write =
            match write_or_reuse_artifact(&input.artifact_path, &input.content, input.overwrite) {
                Ok(write) => write,
                Err(err) => {
                    let _ = state
                        .store
                        .artifact_commit_outbox_mark_failed(
                            &input.operation_key,
                            &format!("{err:#}"),
                        )
                        .await;
                    return Err(err);
                }
            };
        if write.sha256 != artifact_sha256 {
            let err = anyhow!(
                "artifact sha mismatch after write: expected {}, got {} for {}",
                artifact_sha256,
                write.sha256,
                write.path.display()
            );
            let _ = state
                .store
                .artifact_commit_outbox_mark_failed(&input.operation_key, &format!("{err:#}"))
                .await;
            return Err(err);
        }

        let complete_payload = json!({
            "artifact_bytes": write.bytes,
            "artifact_created": write.created,
            "artifact_overwritten": write.overwritten,
        });
        state
            .store
            .artifact_commit_outbox_mark_complete(
                &input.operation_key,
                &write.sha256,
                &complete_payload,
            )
            .await
            .map_err(|e| anyhow!("artifact commit outbox complete failed: {e}"))?;
        Ok(write)
    }
}

pub(crate) async fn recover_artifact_commit_outbox(state: &AppState, limit: i64) -> Result<usize> {
    let rows = state
        .store
        .artifact_commit_outbox_claim_recoverable(limit)
        .await
        .map_err(|e| anyhow!("claim artifact commit outbox failed: {e}"))?;
    let mut recovered = 0usize;
    for row in rows {
        let operation_key = row.operation_key.clone();
        match recover_one(
            &row.artifact_path,
            row.artifact_sha256.as_deref(),
            &row.payload,
        ) {
            Ok(write) => {
                let payload = json!({
                    "artifact_bytes": write.bytes,
                    "artifact_recovered": true,
                    "artifact_created": write.created,
                    "artifact_overwritten": write.overwritten,
                });
                state
                    .store
                    .artifact_commit_outbox_mark_complete(&operation_key, &write.sha256, &payload)
                    .await
                    .map_err(|e| anyhow!("complete recovered artifact commit failed: {e}"))?;
                recovered += 1;
            }
            Err(err) => {
                let _ = state
                    .store
                    .artifact_commit_outbox_mark_failed(&operation_key, &format!("{err:#}"))
                    .await;
            }
        }
    }
    Ok(recovered)
}

fn recover_one(path: &str, expected_sha256: Option<&str>, payload: &Value) -> Result<WriteOutcome> {
    let path = PathBuf::from(path);
    if let Some(existing) = read_existing_metadata(&path)? {
        if let Some(expected) = expected_sha256 {
            if existing.sha256 != expected {
                return Err(anyhow!(
                    "artifact sha mismatch during recovery: expected {}, got {} for {}",
                    expected,
                    existing.sha256,
                    path.display()
                ));
            }
        }
        return Ok(WriteOutcome {
            path,
            created: false,
            overwritten: false,
            sha256: existing.sha256,
            bytes: existing.bytes,
        });
    }

    let content = payload
        .get("artifact_content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("artifact missing and outbox payload has no artifact_content"))?;
    let content_sha = sha256_hex(content);
    if let Some(expected) = expected_sha256 {
        if content_sha != expected {
            return Err(anyhow!(
                "outbox payload sha mismatch during recovery: expected {}, got {}",
                expected,
                content_sha
            ));
        }
    }
    atomic_write_artifact(&path, content, false)
}

fn complete_row_outcome(path: &Path, expected_sha256: &str) -> Result<WriteOutcome> {
    let existing = read_existing_metadata(path)?.ok_or_else(|| {
        anyhow!(
            "complete outbox row has no artifact on disk: {}",
            path.display()
        )
    })?;
    if existing.sha256 != expected_sha256 {
        return Err(anyhow!(
            "complete outbox row artifact sha mismatch: expected {}, got {} for {}",
            expected_sha256,
            existing.sha256,
            path.display()
        ));
    }
    Ok(WriteOutcome {
        path: path.to_path_buf(),
        created: false,
        overwritten: false,
        sha256: existing.sha256,
        bytes: existing.bytes,
    })
}

fn write_or_reuse_artifact(path: &Path, content: &str, overwrite: bool) -> Result<WriteOutcome> {
    let expected_sha = sha256_hex(content);
    if let Some(existing) = read_existing_metadata(path)? {
        if existing.sha256 == expected_sha {
            return Ok(WriteOutcome {
                path: path.to_path_buf(),
                created: false,
                overwritten: false,
                sha256: existing.sha256,
                bytes: existing.bytes,
            });
        }
        if !overwrite {
            return Err(anyhow!(
                "artifact already exists with different sha256 at {}; expected {}, got {}",
                path.display(),
                expected_sha,
                existing.sha256
            ));
        }
    }
    atomic_write_artifact(path, content, overwrite)
}

fn payload_with_artifact_content(payload: &Value, content: &str) -> Value {
    let mut payload = payload.as_object().cloned().unwrap_or_default();
    payload.insert(
        "artifact_content".to_string(),
        Value::String(content.to_string()),
    );
    payload.insert(
        "artifact_content_sha256".to_string(),
        Value::String(sha256_hex(content)),
    );
    Value::Object(payload)
}

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut s, "{:02x}", byte);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recover_one_writes_missing_artifact_from_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("events/000002.event.lisp");
        let content = "(lifecycle-event \"evt-1\")";
        let payload = payload_with_artifact_content(&json!({ "kind": "event" }), content);
        let outcome = recover_one(path.to_str().unwrap(), Some(&sha256_hex(content)), &payload)
            .expect("recover");

        assert!(outcome.created);
        assert_eq!(outcome.sha256, sha256_hex(content));
        assert_eq!(std::fs::read_to_string(path).unwrap(), content);
    }

    #[test]
    fn recover_one_reuses_existing_matching_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("plan.lisp");
        let content = "(plan :id \"p1\")";
        atomic_write_artifact(&path, content, false).unwrap();
        let payload = payload_with_artifact_content(&json!({}), content);
        let outcome = recover_one(path.to_str().unwrap(), Some(&sha256_hex(content)), &payload)
            .expect("recover");

        assert!(!outcome.created);
        assert!(!outcome.overwritten);
        assert_eq!(outcome.sha256, sha256_hex(content));
    }

    #[test]
    fn recover_one_reports_existing_sha_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("plan.lisp");
        atomic_write_artifact(&path, "old", false).unwrap();
        let payload = payload_with_artifact_content(&json!({}), "new");
        let err = recover_one(path.to_str().unwrap(), Some(&sha256_hex("new")), &payload)
            .expect_err("mismatch");

        assert!(format!("{err:#}").contains("artifact sha mismatch during recovery"));
    }

    #[test]
    fn write_or_reuse_artifact_is_idempotent_for_duplicate_operation_payload() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("request.lisp");
        let first = write_or_reuse_artifact(&path, "(request)", false).unwrap();
        let second = write_or_reuse_artifact(&path, "(request)", false).unwrap();

        assert!(first.created);
        assert!(!second.created);
        assert!(!second.overwritten);
        assert_eq!(first.sha256, second.sha256);
    }
}
