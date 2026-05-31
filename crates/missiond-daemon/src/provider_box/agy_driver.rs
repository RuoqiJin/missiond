use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use missiond_core::agy_cli::{discover_sessions, parse_session, AgySession, AgyStep};
use missiond_core::pty::{recognize_screen, PtyCanonicalState};
use missiond_core::types::CliEngine;
use missiond_core::{PTYManager, PTYSlot, PTYSpawnOptions, SessionState};
use regex::Regex;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use super::driver::{ProviderDriver, ProviderDriverCapabilities};
use super::types::{
    ModelSwitchResult, ModelSwitchStatus, ProviderBoxDiagnostic, ProviderBoxResult,
    ProviderBoxStatus, ProviderControlAction, ProviderInteractionRequest, ProviderModelCatalog,
    ProviderModelCatalogEntry, ProviderModelUsage, ProviderRouterExport, ProviderUsageSnapshot,
    ProviderUsageStatus, PtyObservation, PtyStepAction, PtyStepRecord, PtyStepVerificationStatus,
    TimeoutCancelPolicy, DIAG_MODEL_SWITCH_UNVERIFIED, DIAG_PROVIDER_BOX_INVALID_REQUEST,
    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE, DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
    DIAG_PROVIDER_DURABLE_FINAL_MISSING, DIAG_PROVIDER_STATUS_UNAVAILABLE,
    DIAG_PROVIDER_TEXT_ONLY_VIOLATION, DIAG_PROVIDER_TURN_STALLED,
    DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED, DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED,
    DIAG_USAGE_UNKNOWN,
};

const DEFAULT_AGY_SLOT: &str = "slot-agy-provider-box";
const AGY_CTRL_D: &str = "\x1b[100;5u";
const AGY_EXIT_COMMAND: &str = "/exit";
const MODEL_PICKER_MAX_DOWN: usize = 96;
const MODEL_CATALOG_MAX_DOWN: usize = 128;
const OBSERVE_SETTLE_MS: u64 = 220;

#[derive(Clone)]
pub(crate) struct AgyProviderDriver {
    pty: Arc<PTYManager>,
    slot_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    agy_home: PathBuf,
}

#[derive(Debug, Clone)]
struct AgyObservation {
    lines: Vec<String>,
    text: String,
    snapshot: missiond_core::pty::PtyRecognitionSnapshot,
}

#[derive(Debug, Clone)]
struct AgyTurnFinal {
    session_id: String,
    transcript_path: String,
    final_text: String,
}

#[derive(Debug, Clone)]
enum AgyTranscriptOutcome {
    Pending,
    Completed(AgyTurnFinal),
    Violation {
        code: String,
        message: String,
        details: Value,
    },
}

#[derive(Debug)]
enum AgyMonitorOutcome {
    Completed(AgyTurnFinal),
    CancelledForRetry,
    Failed(ProviderBoxResult),
}

#[derive(Debug, Clone)]
struct ModelPickerEntry {
    model: String,
    selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelPickerNavigationPlan {
    direction: &'static str,
    key: &'static str,
    expected_models: Vec<String>,
}

impl AgyProviderDriver {
    pub(crate) fn new(pty: Arc<PTYManager>) -> Self {
        Self {
            pty,
            slot_locks: Arc::new(Mutex::new(HashMap::new())),
            agy_home: default_agy_home(),
        }
    }

    async fn slot_lock(&self, slot_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.slot_locks.lock().await;
        locks
            .entry(slot_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn request_slot_id(request: &ProviderInteractionRequest) -> String {
        request
            .slot_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_AGY_SLOT.to_string())
    }

    fn request_spawn_if_missing(request: &ProviderInteractionRequest) -> bool {
        request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("spawn_if_missing")
                    .or_else(|| worker.get("spawn"))
                    .and_then(Value::as_bool)
            })
            .unwrap_or(false)
    }

    fn request_submit_input(request: &ProviderInteractionRequest) -> bool {
        request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("submit")
                    .or_else(|| worker.get("enter"))
                    .or_else(|| worker.get("append_enter"))
                    .and_then(Value::as_bool)
            })
            .unwrap_or(false)
    }

    async fn ensure_slot(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<String> {
        let slot_id = Self::request_slot_id(request);
        if let Some(status) = self.pty.get_status(&slot_id).await {
            if status.engine != CliEngine::Agy {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Requested slot is not an AGY slot",
                    json!({
                        "slot_id": slot_id,
                        "engine": status.engine.to_string(),
                    }),
                ));
                return None;
            }
            if !matches!(status.state, SessionState::Exited | SessionState::Error) {
                return Some(slot_id);
            }
        }

        let cwd = request
            .cwd
            .as_ref()
            .or(request.project_root.as_ref())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
        let slot = PTYSlot {
            id: slot_id.clone(),
            role: "provider-box-agy".to_string(),
            cwd: Some(cwd),
            engine: CliEngine::Agy,
        };
        self.pty.init_slot(&slot).await;

        let options = PTYSpawnOptions {
            auto_restart: true,
            wait_for_idle: true,
            timeout_secs: Some(90),
            ..Default::default()
        };
        match self.pty.spawn(&slot, options).await {
            Ok(_) => Some(slot_id),
            Err(err) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "AGY PTY slot could not be spawned",
                    json!({
                        "slot_id": slot_id,
                        "error": err.to_string(),
                    }),
                ));
                None
            }
        }
    }

    async fn attach_status_observation(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        expected_change: impl Into<Option<String>>,
    ) -> AgyObservation {
        let observation = self.observe(slot_id).await;
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        let pty_observation = Self::pty_observation(slot_id, &observation);
        result.record_step(PtyStepRecord::new(
            pty_observation.clone(),
            PtyStepAction::key("observe"),
            pty_observation,
            expected_change.into(),
            PtyStepVerificationStatus::Skipped,
        ));
        observation
    }

    async fn observe(&self, slot_id: &str) -> AgyObservation {
        let lines = self
            .pty
            .get_last_lines(slot_id, 180)
            .await
            .unwrap_or_else(|_| Vec::new());
        let status = self.pty.get_status(slot_id).await;
        let state = status
            .as_ref()
            .map(|info| info.state)
            .unwrap_or(SessionState::Idle);
        let snapshot = recognize_screen(CliEngine::Agy, &lines, state);
        let text = lines.join("\n");
        AgyObservation {
            lines,
            text,
            snapshot,
        }
    }

    fn pty_observation(slot_id: &str, observation: &AgyObservation) -> PtyObservation {
        PtyObservation::structured(
            format!("pty:{slot_id}"),
            observation.text.clone(),
            serde_json::to_value(&observation.snapshot).unwrap_or_else(|_| json!({})),
        )
    }

    async fn write_step(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        action: PtyStepAction,
        bytes: &str,
        expected_change: impl Into<Option<String>>,
    ) -> AgyObservation {
        let before = self.observe(slot_id).await;
        let write_result = self.pty.write(slot_id, bytes).await;
        tokio::time::sleep(Duration::from_millis(OBSERVE_SETTLE_MS)).await;
        let after = self.observe(slot_id).await;
        let status = if write_result.is_err() {
            PtyStepVerificationStatus::Failed
        } else if before.text != after.text {
            PtyStepVerificationStatus::Verified
        } else {
            PtyStepVerificationStatus::Unchanged
        };
        let mut step = PtyStepRecord::new(
            Self::pty_observation(slot_id, &before),
            action,
            Self::pty_observation(slot_id, &after),
            expected_change.into(),
            status,
        );
        if let Err(err) = write_result {
            step.diagnostics.push(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "PTY write failed",
                json!({
                    "slot_id": slot_id,
                    "error": err.to_string(),
                }),
            ));
        }
        result.record_step(step);
        after
    }

    async fn wait_until<F>(
        &self,
        slot_id: &str,
        timeout: Duration,
        mut predicate: F,
    ) -> AgyObservation
    where
        F: FnMut(&AgyObservation) -> bool,
    {
        let started = Instant::now();
        loop {
            let observation = self.observe(slot_id).await;
            if predicate(&observation) || started.elapsed() >= timeout {
                return observation;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn ensure_composer_ready(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> Option<AgyObservation> {
        let mut observation = self.observe(slot_id).await;
        if is_overlay_screen(&observation) {
            let _ = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("escape"),
                    "\x1b",
                    Some("close AGY overlay".to_string()),
                )
                .await;
            observation = self
                .wait_until(slot_id, Duration::from_secs(5), |obs| {
                    is_ready_for_text(obs)
                })
                .await;
        }

        if is_ready_for_text(&observation) {
            return Some(observation);
        }

        if observation.snapshot.state == PtyCanonicalState::Running {
            observation = self
                .wait_until(slot_id, Duration::from_secs(2), |obs| {
                    is_ready_for_text(obs)
                })
                .await;
            if is_ready_for_text(&observation) {
                return Some(observation);
            }
        }

        result.status = ProviderBoxStatus::Blocked;
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
            "AGY slot is not ready for text input",
            json!({
                "slot_id": slot_id,
                "state": observation.snapshot.state,
                "reason": observation.snapshot.reason,
                "blocked_kind": observation.snapshot.blocked_kind,
            }),
        ));
        None
    }

    async fn open_model_picker(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> Option<AgyObservation> {
        self.ensure_composer_ready(result, slot_id).await?;
        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/model"),
                "/model",
                Some("type AGY /model command".to_string()),
            )
            .await;
        let after = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute AGY /model command".to_string()),
            )
            .await;
        let observation = if is_model_picker(&after) {
            after
        } else {
            self.wait_until(slot_id, Duration::from_secs(4), is_model_picker)
                .await
        };
        if is_model_picker(&observation) {
            Some(observation)
        } else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_MODEL_SWITCH_UNVERIFIED,
                "AGY /model picker did not render",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                }),
            ));
            None
        }
    }

    async fn switch_model_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        target_model: &str,
    ) -> bool {
        let current = self
            .observe(slot_id)
            .await
            .snapshot
            .screen_identity
            .as_ref()
            .and_then(|identity| identity.current_model.clone());
        if model_eq(current.as_deref(), target_model) {
            result.model_switch_result = Some(ModelSwitchResult {
                status: ModelSwitchStatus::Verified,
                requested_model: Some(target_model.to_string()),
                requested_model_profile: request.model_profile.clone(),
                verified_model: current,
                verification_source: Some("agy:screen_identity".to_string()),
            });
            return true;
        }

        let mut observation = match self.open_model_picker(result, slot_id).await {
            Some(observation) => observation,
            None => return false,
        };

        if let Some(ok) = self
            .select_current_picker_model(request, result, slot_id, target_model, &observation)
            .await
        {
            return ok;
        }

        if let Some(plan) = plan_model_picker_navigation(&observation, target_model) {
            let (planned_result, planned_observation) = self
                .execute_model_picker_navigation_plan(
                    request,
                    result,
                    slot_id,
                    target_model,
                    &observation,
                    &plan,
                )
                .await;
            observation = planned_observation;
            if let Some(ok) = planned_result {
                return ok;
            }
        }

        let mut all_seen_selected = HashSet::new();
        for (direction, key, label) in [
            ("up", "\x1b[A", "move AGY model picker selection up"),
            ("down", "\x1b[B", "move AGY model picker selection down"),
        ] {
            let mut phase_seen_selected = HashSet::new();
            for _ in 0..MODEL_PICKER_MAX_DOWN {
                if let Some(ok) = self
                    .select_current_picker_model(
                        request,
                        result,
                        slot_id,
                        target_model,
                        &observation,
                    )
                    .await
                {
                    return ok;
                }

                if let Some(value) = observation
                    .snapshot
                    .screen_identity
                    .as_ref()
                    .and_then(|identity| identity.selected_model.clone())
                {
                    let normalized = normalize_model(&value);
                    all_seen_selected.insert(normalized.clone());
                    if !phase_seen_selected.insert(normalized) {
                        break;
                    }
                }
                observation = self
                    .write_step(
                        result,
                        slot_id,
                        PtyStepAction::key(direction),
                        key,
                        Some(label.to_string()),
                    )
                    .await;
            }
        }

        result.status = ProviderBoxStatus::Unverified;
        result.model_switch_result = Some(ModelSwitchResult {
            status: ModelSwitchStatus::Unverified,
            requested_model: Some(target_model.to_string()),
            requested_model_profile: request.model_profile.clone(),
            verified_model: observation
                .snapshot
                .screen_identity
                .as_ref()
                .and_then(|identity| identity.current_model.clone()),
            verification_source: Some("agy:model_picker".to_string()),
        });
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            DIAG_MODEL_SWITCH_UNVERIFIED,
            "Target AGY model was not found by bounded arrow-key navigation",
            json!({
                "target_model": target_model,
                "slot_id": slot_id,
                "seen_selected_count": all_seen_selected.len(),
                "scan_directions": ["up", "down"],
            }),
        ));
        false
    }

    async fn execute_model_picker_navigation_plan(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        target_model: &str,
        initial_observation: &AgyObservation,
        plan: &ModelPickerNavigationPlan,
    ) -> (Option<bool>, AgyObservation) {
        let mut observation = initial_observation.clone();
        for expected_model in &plan.expected_models {
            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key(plan.direction),
                    plan.key,
                    Some(format!(
                        "move AGY model picker selection {} to {}",
                        plan.direction, expected_model
                    )),
                )
                .await;
            if !is_model_picker(&observation) {
                result.status = ProviderBoxStatus::Unverified;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_MODEL_SWITCH_UNVERIFIED,
                    "AGY model picker closed during planned navigation",
                    json!({
                        "slot_id": slot_id,
                        "target_model": target_model,
                        "expected_model": expected_model,
                        "direction": plan.direction,
                        "reason": observation.snapshot.reason,
                    }),
                ));
                return (Some(false), observation);
            }
            let selected = observation
                .snapshot
                .screen_identity
                .as_ref()
                .and_then(|identity| identity.selected_model.clone());
            if !model_eq(selected.as_deref(), expected_model) {
                result.add_diagnostic(ProviderBoxDiagnostic::warning(
                    DIAG_MODEL_SWITCH_UNVERIFIED,
                    "AGY planned model-picker navigation did not land on the expected row; falling back to bounded scan",
                    json!({
                        "slot_id": slot_id,
                        "target_model": target_model,
                        "expected_model": expected_model,
                        "selected_model": selected,
                        "direction": plan.direction,
                    }),
                ));
                return (None, observation);
            }
        }

        if let Some(ok) = self
            .select_current_picker_model(request, result, slot_id, target_model, &observation)
            .await
        {
            return (Some(ok), observation);
        }

        result.add_diagnostic(ProviderBoxDiagnostic::warning(
            DIAG_MODEL_SWITCH_UNVERIFIED,
            "AGY planned model-picker navigation reached the expected rows but target was not selected; falling back to bounded scan",
            json!({
                "slot_id": slot_id,
                "target_model": target_model,
                "direction": plan.direction,
                "planned_steps": plan.expected_models.len(),
            }),
        ));
        (None, observation)
    }

    async fn select_current_picker_model(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        target_model: &str,
        observation: &AgyObservation,
    ) -> Option<bool> {
        let selected = observation
            .snapshot
            .screen_identity
            .as_ref()
            .and_then(|identity| identity.selected_model.clone());
        if !model_eq(selected.as_deref(), target_model) {
            return None;
        }

        let after_enter = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("select AGY model".to_string()),
            )
            .await;
        let verified = if current_model_eq(&after_enter, target_model) {
            after_enter
        } else {
            self.wait_until(slot_id, Duration::from_secs(8), |obs| {
                current_model_eq(obs, target_model) && !is_model_picker(obs)
            })
            .await
        };
        let verified_model = verified
            .snapshot
            .screen_identity
            .as_ref()
            .and_then(|identity| identity.current_model.clone());
        let ok = model_eq(verified_model.as_deref(), target_model);
        result.model_switch_result = Some(ModelSwitchResult {
            status: if ok {
                ModelSwitchStatus::Verified
            } else {
                ModelSwitchStatus::Unverified
            },
            requested_model: Some(target_model.to_string()),
            requested_model_profile: request.model_profile.clone(),
            verified_model,
            verification_source: Some("agy:screen_identity".to_string()),
        });
        if !ok {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_MODEL_SWITCH_UNVERIFIED,
                "AGY model switch could not be verified from header/footer model lines",
                json!({
                    "target_model": target_model,
                    "slot_id": slot_id,
                    "reason": verified.snapshot.reason,
                }),
            ));
        }
        Some(ok)
    }

    async fn verify_pinned_model_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        target_model: &str,
    ) -> bool {
        let observation = self.observe(slot_id).await;
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        let current = observation
            .snapshot
            .screen_identity
            .as_ref()
            .and_then(|identity| identity.current_model.clone());
        let ok = model_eq(current.as_deref(), target_model);
        result.model_switch_result = Some(ModelSwitchResult {
            status: if ok {
                ModelSwitchStatus::Verified
            } else {
                ModelSwitchStatus::Unverified
            },
            requested_model: Some(target_model.to_string()),
            requested_model_profile: request.model_profile.clone(),
            verified_model: current.clone(),
            verification_source: Some("agy:pinned-slot-screen_identity".to_string()),
        });
        if !ok {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_MODEL_SWITCH_UNVERIFIED,
                "AGY pinned text-only slot is not on the requested model; hot-path model switching is disabled",
                json!({
                    "slot_id": slot_id,
                    "requested_model": target_model,
                    "verified_model": current,
                    "reason": observation.snapshot.reason,
                    "switch_policy": "pinned_slot_verify_only",
                }),
            ));
        }
        ok
    }

    async fn submit_prompt_step(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        prompt: &str,
    ) -> bool {
        let before = self.observe(slot_id).await;
        let send_result = self.pty.send_fire_and_forget(slot_id, prompt).await;
        tokio::time::sleep(Duration::from_millis(OBSERVE_SETTLE_MS)).await;
        let after = self.observe(slot_id).await;
        let mut action = PtyStepAction::text("<pure text prompt paste then enter>");
        action.redacted = true;
        let status = if send_result.is_err() {
            PtyStepVerificationStatus::Failed
        } else if after.snapshot.state == PtyCanonicalState::Running || before.text != after.text {
            PtyStepVerificationStatus::Verified
        } else {
            PtyStepVerificationStatus::Ambiguous
        };
        let mut step = PtyStepRecord::new(
            Self::pty_observation(slot_id, &before),
            action,
            Self::pty_observation(slot_id, &after),
            Some("AGY accepts prompt and enters running state".to_string()),
            status,
        );
        if let Err(err) = send_result {
            step.diagnostics.push(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "AGY prompt submission failed",
                json!({
                    "slot_id": slot_id,
                    "error": err.to_string(),
                }),
            ));
        }
        let ok = step.verification_status != PtyStepVerificationStatus::Failed;
        result.record_step(step);
        ok
    }

    async fn monitor_turn(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        policy: &TimeoutCancelPolicy,
        attempt: u8,
    ) -> AgyMonitorOutcome {
        let timeout_secs = request.timeout_secs.unwrap_or(policy.running_timeout_secs);
        let started = Instant::now();
        let mut last_screen_hash: Option<String> = None;
        let mut last_progress = Instant::now();
        let mut idle_seen_at: Option<Instant> = None;

        loop {
            match self
                .extract_turn_from_transcripts(&request.correlation_id)
                .await
            {
                AgyTranscriptOutcome::Completed(final_turn) => {
                    return AgyMonitorOutcome::Completed(final_turn);
                }
                AgyTranscriptOutcome::Violation {
                    code,
                    message,
                    details,
                } => {
                    let mut failed = ProviderBoxResult::base(request, ProviderBoxStatus::Failed);
                    failed.slot_id = Some(slot_id.to_string());
                    failed.step_records = result.step_records.clone();
                    failed.add_diagnostic(ProviderBoxDiagnostic::error(code, message, details));
                    return AgyMonitorOutcome::Failed(failed);
                }
                AgyTranscriptOutcome::Pending => {}
            }

            let observation = self.observe(slot_id).await;
            let screen_hash = PtyObservation::text("pty-screen", &observation.text).screen_hash;
            if screen_hash != last_screen_hash {
                last_screen_hash = screen_hash;
                last_progress = Instant::now();
            }

            if is_hard_blocked(&observation) {
                let mut failed = ProviderBoxResult::base(request, ProviderBoxStatus::Blocked);
                failed.slot_id = Some(slot_id.to_string());
                failed.step_records = result.step_records.clone();
                failed.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_TEXT_ONLY_VIOLATION,
                    "AGY entered a blocked tool/file/approval surface during a text-only turn",
                    json!({
                        "slot_id": slot_id,
                        "blocked_kind": observation.snapshot.blocked_kind,
                        "reason": observation.snapshot.reason,
                    }),
                ));
                return AgyMonitorOutcome::Failed(failed);
            }

            if is_ready_for_text(&observation) {
                if let Some(seen_at) = idle_seen_at {
                    if seen_at.elapsed() >= Duration::from_secs(2) {
                        let mut failed =
                            ProviderBoxResult::base(request, ProviderBoxStatus::Failed);
                        failed.slot_id = Some(slot_id.to_string());
                        failed.step_records = result.step_records.clone();
                        failed.add_diagnostic(ProviderBoxDiagnostic::error(
                            DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                            "AGY returned to input but no matching durable JSONL final was found",
                            json!({
                                "slot_id": slot_id,
                                "correlation_id": request.correlation_id,
                                "agy_home": self.agy_home.display().to_string(),
                            }),
                        ));
                        return AgyMonitorOutcome::Failed(failed);
                    }
                } else {
                    idle_seen_at = Some(Instant::now());
                }
            } else {
                idle_seen_at = None;
            }

            let timeout_hit = started.elapsed() >= Duration::from_secs(timeout_secs);
            let no_progress_hit = last_progress.elapsed()
                >= Duration::from_secs(policy.no_progress_grace_secs)
                && observation.snapshot.state == PtyCanonicalState::Running;
            if timeout_hit || no_progress_hit {
                result.add_diagnostic(ProviderBoxDiagnostic::warning(
                    DIAG_PROVIDER_TURN_STALLED,
                    "AGY turn exceeded timeout/progress budget",
                    json!({
                        "slot_id": slot_id,
                        "attempt": attempt,
                        "timeout_hit": timeout_hit,
                        "no_progress_hit": no_progress_hit,
                        "elapsed_secs": started.elapsed().as_secs(),
                        "no_progress_secs": last_progress.elapsed().as_secs(),
                    }),
                ));
                if self.cancel_active_turn(result, slot_id, policy).await {
                    return AgyMonitorOutcome::CancelledForRetry;
                }

                let mut failed = ProviderBoxResult::base(request, ProviderBoxStatus::Failed);
                failed.slot_id = Some(slot_id.to_string());
                failed.step_records = result.step_records.clone();
                failed.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED,
                    "AGY turn timed out and cancel did not return to a ready surface",
                    json!({
                        "slot_id": slot_id,
                        "attempt": attempt,
                    }),
                ));
                return AgyMonitorOutcome::Failed(failed);
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn cancel_active_turn(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        policy: &TimeoutCancelPolicy,
    ) -> bool {
        for attempt in 0..policy.max_cancel_attempts.max(1) {
            let key = if policy.cancel_key.eq_ignore_ascii_case("escape") {
                "\x1b"
            } else {
                "\x03"
            };
            self.write_step(
                result,
                slot_id,
                PtyStepAction::key(policy.cancel_key.clone()),
                key,
                Some("cancel stalled AGY turn".to_string()),
            )
            .await;
            let ready = self
                .wait_until(
                    slot_id,
                    Duration::from_secs(policy.cancel_grace_secs.max(1)),
                    |obs| is_ready_for_text(obs),
                )
                .await;
            if is_ready_for_text(&ready) {
                result.add_diagnostic(ProviderBoxDiagnostic::warning(
                    DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED,
                    "AGY stalled turn was cancelled and returned to ready state",
                    json!({
                        "slot_id": slot_id,
                        "cancel_attempt": attempt + 1,
                        "reason": ready.snapshot.reason,
                    }),
                ));
                return true;
            }
        }
        false
    }

    async fn extract_turn_from_transcripts(&self, correlation_id: &str) -> AgyTranscriptOutcome {
        let brain = self.agy_home.join("brain");
        let sessions = discover_sessions(&brain).await;
        for (session_id, transcript) in sessions {
            let Some(session) = parse_session(&transcript).await else {
                continue;
            };
            if session.session_id != session_id {
                debug!(
                    session_id,
                    "AGY parsed session id differs from directory id"
                );
            }
            if let Some(outcome) = extract_correlated_turn(&session, correlation_id) {
                return match outcome {
                    AgyTranscriptOutcome::Completed(mut final_turn) => {
                        final_turn.transcript_path = transcript.display().to_string();
                        AgyTranscriptOutcome::Completed(final_turn)
                    }
                    other => other,
                };
            }
        }
        AgyTranscriptOutcome::Pending
    }

    fn validate_pure_text(
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> bool {
        let prompt_ok = request
            .prompt
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
        let output_ok = request.output_contract.as_ref().map_or(true, |contract| {
            contract
                .get("media_type")
                .and_then(Value::as_str)
                .map_or(true, |value| value == "text/plain")
                && contract
                    .get("single_turn")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
        });
        let ok = prompt_ok
            && output_ok
            && request.attachments.is_empty()
            && request.no_tools
            && request.no_mcp
            && request.no_shell
            && request.no_file_access
            && request.single_turn_policy.as_ref().map_or(true, |policy| {
                policy.require_plain_text
                    && policy.no_tools
                    && policy.no_mcp
                    && policy.no_shell
                    && policy.no_file_access
            });
        if !ok {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "AGY text-only turn requires prompt, text/plain output, no attachments, and all no-tool guards",
                json!({
                    "prompt_present": prompt_ok,
                    "attachments": request.attachments.len(),
                    "no_tools": request.no_tools,
                    "no_mcp": request.no_mcp,
                    "no_shell": request.no_shell,
                    "no_file_access": request.no_file_access,
                    "output_ok": output_ok,
                }),
            ));
        }
        ok
    }

    async fn refresh_usage_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let first = self.observe(slot_id).await;
        if is_usage_screen(&first) {
            self.write_step(
                result,
                slot_id,
                PtyStepAction::key("escape"),
                "\x1b",
                Some("close current AGY usage screen before refresh".to_string()),
            )
            .await;
            let _ = self
                .wait_until(slot_id, Duration::from_secs(5), is_ready_for_text)
                .await;
        }
        if self.ensure_composer_ready(result, slot_id).await.is_none() {
            return;
        }
        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/usage"),
                "/usage",
                Some("type AGY /usage command".to_string()),
            )
            .await;
        let after = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute AGY /usage command".to_string()),
            )
            .await;
        let usage = if usage_screen_value(&after).is_some() {
            after
        } else {
            self.wait_until(slot_id, Duration::from_secs(6), |obs| {
                usage_screen_value(obs).is_some()
            })
            .await
        };

        if let Some(screen_usage) = usage_screen_value(&usage) {
            let quotas = screen_usage
                .get("modelQuotas")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    Some(ProviderModelUsage {
                        model: entry.get("model")?.as_str()?.to_string(),
                        percent: entry
                            .get("percent")
                            .and_then(Value::as_u64)
                            .map(|value| value.min(100) as u8),
                        status: entry
                            .get("status")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                })
                .collect::<Vec<_>>();
            result.status = ProviderBoxStatus::Completed;
            result.usage_snapshot = Some(ProviderUsageSnapshot {
                schema: "missiond.provider-usage-snapshot.v1".to_string(),
                snapshot_id: format!("usage-{}", uuid::Uuid::new_v4().simple()),
                provider: request
                    .provider
                    .clone()
                    .or_else(|| Some("agy_cli".to_string())),
                engine: CliEngine::Agy,
                slot_id: Some(slot_id.to_string()),
                account_ref: usage
                    .snapshot
                    .screen_identity
                    .as_ref()
                    .and_then(|identity| identity.account.clone()),
                model: usage
                    .snapshot
                    .screen_identity
                    .as_ref()
                    .and_then(|identity| identity.current_model.clone())
                    .or_else(|| request.model.clone()),
                observed_at: chrono::Utc::now().to_rfc3339(),
                status: ProviderUsageStatus::Exact,
                remaining: None,
                limit: None,
                reset_at: None,
                source: Some("agy:/usage".to_string()),
                confidence: usage.snapshot.confidence as f32,
                block_kind: None,
                model_quotas: quotas,
                diagnostics: Vec::new(),
            });
        } else {
            result.status = ProviderBoxStatus::Unknown;
            result.usage_snapshot = Some(ProviderUsageSnapshot::unknown(request));
            result.add_diagnostic(ProviderBoxDiagnostic::warning(
                DIAG_USAGE_UNKNOWN,
                "AGY /usage screen did not expose structured model quotas",
                json!({
                    "slot_id": slot_id,
                    "reason": usage.snapshot.reason,
                }),
            ));
        }
    }

    async fn clear_screen_locked(&self, result: &mut ProviderBoxResult, slot_id: &str) {
        if self.ensure_composer_ready(result, slot_id).await.is_none() {
            return;
        }

        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/"),
                "/",
                Some("open AGY slash command menu".to_string()),
            )
            .await;
        if !is_slash_command_surface(&observation) {
            observation = self
                .wait_until(slot_id, Duration::from_secs(3), is_slash_command_surface)
                .await;
        }
        if !is_slash_command_surface(&observation) {
            mark_control_unverified(
                result,
                slot_id,
                "AGY slash command menu did not open before clear",
                &observation,
            );
            return;
        }

        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("c"),
                "c",
                Some("filter AGY slash commands by c".to_string()),
            )
            .await;
        if !observation.text.contains("/clear") {
            observation = self
                .wait_until(slot_id, Duration::from_secs(3), |obs| {
                    is_slash_command_surface(obs) && obs.text.contains("/clear")
                })
                .await;
        }
        if !observation.text.contains("/clear") {
            mark_control_unverified(
                result,
                slot_id,
                "AGY /clear command was not visible after /c filter",
                &observation,
            );
            return;
        }

        let observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("down"),
                "\x1b[B",
                Some("select AGY /clear command".to_string()),
            )
            .await;
        if !selected_clear_command(&observation) {
            mark_control_unverified(
                result,
                slot_id,
                "AGY /clear command was not selected after one down arrow",
                &observation,
            );
            return;
        }

        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("complete AGY /clear into composer".to_string()),
            )
            .await;
        if !is_pending_clear_command(&observation) {
            observation = self
                .wait_until(slot_id, Duration::from_secs(3), is_pending_clear_command)
                .await;
        }
        if !is_pending_clear_command(&observation) {
            mark_control_unverified(
                result,
                slot_id,
                "AGY first enter did not complete /clear into the composer",
                &observation,
            );
            return;
        }

        observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute AGY /clear command".to_string()),
            )
            .await;
        if !is_home_identity_ready(&observation) {
            observation = self
                .wait_until(slot_id, Duration::from_secs(5), is_home_identity_ready)
                .await;
        }
        if is_home_identity_ready(&observation) {
            result.status = ProviderBoxStatus::Completed;
        } else {
            mark_control_unverified(
                result,
                slot_id,
                "AGY /clear execution did not return to the home identity screen",
                &observation,
            );
        }
    }

    async fn exit_locked(&self, result: &mut ProviderBoxResult, slot_id: &str) {
        let initial = self.observe(slot_id).await;
        if is_shell_prompt_after_exit(&initial) {
            result.status = ProviderBoxStatus::Completed;
            let status = self.pty.get_status(slot_id).await;
            result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &initial));
            return;
        }
        if is_exit_confirm_pending(&initial) {
            self.confirm_ctrl_d_exit_locked(result, slot_id).await;
            return;
        }

        if self.ensure_composer_ready(result, slot_id).await.is_none() {
            return;
        }

        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/exit"),
                AGY_EXIT_COMMAND,
                Some("type AGY /exit command".to_string()),
            )
            .await;
        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute AGY /exit command".to_string()),
            )
            .await;
        if !is_shell_prompt_after_exit(&observation) {
            observation = self
                .wait_until(slot_id, Duration::from_secs(5), is_shell_prompt_after_exit)
                .await;
        }
        if is_shell_prompt_after_exit(&observation) {
            result.status = ProviderBoxStatus::Completed;
            return;
        }

        if is_exit_confirm_pending(&observation) || is_ready_for_text(&observation) {
            self.confirm_ctrl_d_exit_locked(result, slot_id).await;
        } else {
            mark_control_unverified(
                result,
                slot_id,
                "AGY /exit command did not return to shell prompt",
                &observation,
            );
        }
    }

    async fn confirm_ctrl_d_exit_locked(&self, result: &mut ProviderBoxResult, slot_id: &str) {
        let mut observation = self.observe(slot_id).await;
        if !is_exit_confirm_pending(&observation) {
            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("ctrl+d"),
                    AGY_CTRL_D,
                    Some("request AGY Ctrl+D exit confirmation fallback".to_string()),
                )
                .await;
            if !is_exit_confirm_pending(&observation) {
                observation = self
                    .wait_until(slot_id, Duration::from_secs(3), is_exit_confirm_pending)
                    .await;
            }
            if !is_exit_confirm_pending(&observation) {
                mark_control_unverified(
                    result,
                    slot_id,
                    "AGY /exit failed and Ctrl+D fallback did not show exit confirmation",
                    &observation,
                );
                return;
            }
        }

        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("ctrl+d"),
                AGY_CTRL_D,
                Some("confirm AGY Ctrl+D exit fallback".to_string()),
            )
            .await;
        if !is_shell_prompt_after_exit(&observation) {
            observation = self
                .wait_until(slot_id, Duration::from_secs(5), is_shell_prompt_after_exit)
                .await;
        }
        if is_shell_prompt_after_exit(&observation) {
            result.status = ProviderBoxStatus::Completed;
        } else {
            mark_control_unverified(
                result,
                slot_id,
                "AGY Ctrl+D exit fallback did not return to shell prompt",
                &observation,
            );
        }
    }

    async fn input_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let Some(input) = request
            .prompt
            .as_ref()
            .map(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "AGY input control action requires prompt or text",
                json!({
                    "slot_id": slot_id,
                    "control_action": "input",
                }),
            ));
            return;
        };

        if self.ensure_composer_ready(result, slot_id).await.is_none() {
            return;
        }

        let submit = Self::request_submit_input(request);
        let mut action = PtyStepAction::text("<input text>");
        action.redacted = true;
        let mut after = self
            .write_step(
                result,
                slot_id,
                action,
                input,
                Some("write text into AGY composer".to_string()),
            )
            .await;
        if submit {
            after = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("enter"),
                    "\r",
                    Some("press Enter to submit AGY input".to_string()),
                )
                .await;
        }
        let failed = result
            .step_records
            .last()
            .is_some_and(|step| step.verification_status == PtyStepVerificationStatus::Failed);
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &after));
        result.status = if failed {
            ProviderBoxStatus::Failed
        } else {
            ProviderBoxStatus::Completed
        };
    }
}

#[async_trait]
impl ProviderDriver for AgyProviderDriver {
    fn engine(&self) -> CliEngine {
        CliEngine::Agy
    }

    fn capabilities(&self) -> ProviderDriverCapabilities {
        ProviderDriverCapabilities {
            submit_turn: true,
            switch_model: true,
            usage_probe: true,
            model_catalog: true,
            pure_text_guard: true,
            control_action: true,
            status: true,
        }
    }

    async fn status(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let slot_id = Self::request_slot_id(request);
        result.slot_id = Some(slot_id.clone());

        if Self::request_spawn_if_missing(request) {
            let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
                return result;
            };
            result.slot_id = Some(slot_id.clone());
        } else {
            let Some(status) = self.pty.get_status(&slot_id).await else {
                result.status = ProviderBoxStatus::Unknown;
                result.add_diagnostic(ProviderBoxDiagnostic::warning(
                    DIAG_PROVIDER_STATUS_UNAVAILABLE,
                    "AGY slot status is unavailable",
                    json!({
                        "slot_id": slot_id,
                        "spawn_if_missing": false,
                    }),
                ));
                return result;
            };
            if status.engine != CliEngine::Agy {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Requested slot is not an AGY slot",
                    json!({
                        "slot_id": slot_id,
                        "engine": status.engine.to_string(),
                    }),
                ));
                return result;
            }
        }

        self.attach_status_observation(
            &mut result,
            &slot_id,
            Some("observe current AGY CLI state".to_string()),
        )
        .await;
        result.status = ProviderBoxStatus::Completed;
        result
    }

    async fn switch_model(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;
        let policy = request.model_switch_policy.clone().unwrap_or_default();
        let target = policy
            .target_model
            .or_else(|| request.model.clone())
            .unwrap_or_default();
        if target.trim().is_empty() {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "AGY model switch requires target model",
                json!({"slot_id": slot_id}),
            ));
            return result;
        }

        if self
            .switch_model_locked(request, &mut result, &slot_id, &target)
            .await
        {
            result.status = ProviderBoxStatus::Completed;
        }
        result
    }

    async fn probe_usage(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;
        self.refresh_usage_locked(request, &mut result, &slot_id)
            .await;
        result
    }

    async fn discover_models(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;

        let mut observation = match self.open_model_picker(&mut result, &slot_id).await {
            Some(observation) => observation,
            None => return result,
        };
        let mut models = Vec::<String>::new();
        let mut seen_models = HashSet::<String>::new();
        let mut seen_selected = HashSet::<String>::new();
        let mut stagnant_steps = 0usize;

        for _ in 0..MODEL_CATALOG_MAX_DOWN {
            let before_len = seen_models.len();
            for model in extract_models_from_screen(&observation.text) {
                let normalized = normalize_model(&model);
                if seen_models.insert(normalized) {
                    models.push(model);
                }
            }
            if let Some(selected) = observation
                .snapshot
                .screen_identity
                .as_ref()
                .and_then(|identity| identity.selected_model.clone())
            {
                let normalized = normalize_model(&selected);
                if !seen_selected.insert(normalized) && seen_selected.len() > 1 {
                    break;
                }
            }
            if seen_models.len() == before_len {
                stagnant_steps += 1;
            } else {
                stagnant_steps = 0;
            }
            if stagnant_steps >= 12 && seen_models.len() > 1 {
                break;
            }
            observation = self
                .write_step(
                    &mut result,
                    &slot_id,
                    PtyStepAction::key("down"),
                    "\x1b[B",
                    Some("scroll AGY model picker for catalog export".to_string()),
                )
                .await;
            if !is_model_picker(&observation) {
                break;
            }
        }

        let _ = self
            .write_step(
                &mut result,
                &slot_id,
                PtyStepAction::key("escape"),
                "\x1b",
                Some("close AGY model picker after catalog export".to_string()),
            )
            .await;

        models.sort_by_key(|model| normalize_model(model));
        models.dedup_by(|a, b| normalize_model(a) == normalize_model(b));
        let entries = models
            .iter()
            .map(|model| ProviderModelCatalogEntry {
                provider_model_id: format!("agy:{}", slug_model(model)),
                display_name: model.clone(),
                family: model.split_whitespace().next().map(str::to_string),
                routeable_default: true,
                switch_capability: "interactive_model_picker".to_string(),
                usage_probe_capability: "interactive_usage_screen".to_string(),
                confidence: 0.82,
            })
            .collect::<Vec<_>>();

        let catalog = ProviderModelCatalog {
            schema: "missiond.provider-model-catalog.v1".to_string(),
            catalog_id: format!("catalog-{}", uuid::Uuid::new_v4().simple()),
            provider: request
                .provider
                .clone()
                .or_else(|| Some("agy_cli".to_string())),
            engine: CliEngine::Agy,
            account_ref: observation
                .snapshot
                .screen_identity
                .as_ref()
                .and_then(|identity| identity.account.clone()),
            discovered_at: chrono::Utc::now().to_rfc3339(),
            source: Some("agy:/model".to_string()),
            entries,
            diagnostics: Vec::new(),
        };
        let router_export = build_router_export(request, &catalog);
        result.status = ProviderBoxStatus::Completed;
        result.router_export = Some(router_export);
        result.model_catalog = Some(catalog);
        result
    }

    async fn pure_text_single_turn(
        &self,
        request: &ProviderInteractionRequest,
    ) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        if !Self::validate_pure_text(request, &mut result) {
            return result;
        }
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;

        if let Some(target_model) = request.model.as_deref() {
            if !target_model.trim().is_empty() {
                let model_policy = request.model_switch_policy.clone().unwrap_or_default();
                let model_ready = if model_policy.allow_respawn {
                    self.switch_model_locked(request, &mut result, &slot_id, target_model)
                        .await
                } else {
                    self.verify_pinned_model_locked(request, &mut result, &slot_id, target_model)
                        .await
                };
                if !model_ready {
                    return result;
                }
            }
        }

        if self
            .ensure_composer_ready(&mut result, &slot_id)
            .await
            .is_none()
        {
            return result;
        }

        let policy = request.timeout_cancel_policy.clone().unwrap_or_default();
        let prompt = request.prompt.clone().unwrap_or_default();
        let max_attempts = policy.max_retries.saturating_add(1).max(1);
        for attempt in 0..max_attempts {
            if !self
                .submit_prompt_step(&mut result, &slot_id, &prompt)
                .await
            {
                result.status = ProviderBoxStatus::Failed;
                return result;
            }
            match self
                .monitor_turn(request, &mut result, &slot_id, &policy, attempt)
                .await
            {
                AgyMonitorOutcome::Completed(final_turn) => {
                    result.status = ProviderBoxStatus::Completed;
                    result.provider_conversation_id = Some(final_turn.session_id);
                    result.durable_source = Some(final_turn.transcript_path);
                    result.final_text = Some(final_turn.final_text);
                    return result;
                }
                AgyMonitorOutcome::CancelledForRetry
                    if policy.retry_after_cancel && attempt + 1 < max_attempts =>
                {
                    if self
                        .ensure_composer_ready(&mut result, &slot_id)
                        .await
                        .is_none()
                    {
                        return result;
                    }
                }
                AgyMonitorOutcome::CancelledForRetry => {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED,
                        "AGY turn was cancelled and retry budget was exhausted",
                        json!({
                            "slot_id": slot_id,
                            "attempt": attempt,
                        }),
                    ));
                    return result;
                }
                AgyMonitorOutcome::Failed(mut failed) => {
                    failed.diagnostics.extend(result.diagnostics.clone());
                    return failed;
                }
            }
        }

        result.status = ProviderBoxStatus::Failed;
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_DURABLE_FINAL_MISSING,
            "AGY text-only turn exhausted retry budget without durable final",
            json!({
                "slot_id": slot_id,
                "correlation_id": request.correlation_id,
            }),
        ));
        result
    }

    async fn control_action(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(action) = request.control_action else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "AGY control action request requires control_action",
                json!({
                    "slot_id": request.slot_id,
                    "command": request.command,
                }),
            ));
            return result;
        };
        if matches!(action, ProviderControlAction::Exit) {
            let slot_id = Self::request_slot_id(request);
            result.slot_id = Some(slot_id.clone());
            let Some(status) = self.pty.get_status(&slot_id).await else {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Cannot exit an unavailable AGY slot",
                    json!({
                        "slot_id": slot_id,
                    }),
                ));
                return result;
            };
            if status.engine != CliEngine::Agy {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Requested slot is not an AGY slot",
                    json!({
                        "slot_id": slot_id,
                        "engine": status.engine.to_string(),
                    }),
                ));
                return result;
            }
            if status.state == SessionState::Exited {
                let observation = self.observe(&slot_id).await;
                result.slot_status = Some(slot_status_value(&slot_id, Some(&status), &observation));
                result.status = ProviderBoxStatus::Completed;
                return result;
            }

            let lock = self.slot_lock(&slot_id).await;
            let _guard = lock.lock().await;
            self.exit_locked(&mut result, &slot_id).await;
            if result.slot_status.is_none() {
                self.attach_status_observation(
                    &mut result,
                    &slot_id,
                    Some("observe AGY state after exit control action".to_string()),
                )
                .await;
            }
            return result;
        }

        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;

        match action {
            ProviderControlAction::Input => {
                self.input_locked(request, &mut result, &slot_id).await;
            }
            ProviderControlAction::ClearScreen => {
                self.clear_screen_locked(&mut result, &slot_id).await;
            }
            ProviderControlAction::Exit => {
                self.exit_locked(&mut result, &slot_id).await;
            }
        }
        if result.slot_status.is_none() {
            self.attach_status_observation(
                &mut result,
                &slot_id,
                Some("observe AGY state after control action".to_string()),
            )
            .await;
        }
        result
    }
}

fn is_model_picker(observation: &AgyObservation) -> bool {
    observation.snapshot.blocked_kind.as_deref() == Some("model_picker")
        || observation.snapshot.reason == "agy:model_picker"
}

fn is_usage_screen(observation: &AgyObservation) -> bool {
    observation.snapshot.reason == "agy:usage_meter"
        || observation.snapshot.screen_usage.is_some()
        || observation
            .text
            .to_ascii_lowercase()
            .contains("model quota")
}

fn is_overlay_screen(observation: &AgyObservation) -> bool {
    is_model_picker(observation)
        || is_usage_screen(observation)
        || is_slash_command_surface(observation)
}

fn is_ready_for_text(observation: &AgyObservation) -> bool {
    matches!(
        observation.snapshot.state,
        PtyCanonicalState::Idle | PtyCanonicalState::Complete
    ) && !is_model_picker(observation)
        && !is_usage_screen(observation)
        && !is_slash_command_surface(observation)
        && !is_exit_confirm_pending(observation)
        && !is_shell_prompt_after_exit(observation)
}

fn is_hard_blocked(observation: &AgyObservation) -> bool {
    observation.snapshot.state == PtyCanonicalState::Blocked
        && !is_model_picker(observation)
        && !is_slash_command_surface(observation)
}

fn is_slash_command_surface(observation: &AgyObservation) -> bool {
    matches!(
        observation.snapshot.blocked_kind.as_deref(),
        Some("slash_command_menu" | "slash_command_input")
    ) || matches!(
        observation.snapshot.reason.as_str(),
        "agy:slash_command_menu" | "agy:slash_command_pending"
    )
}

fn is_exit_confirm_pending(observation: &AgyObservation) -> bool {
    observation.snapshot.reason == "agy:exit_confirm_pending"
        || observation.snapshot.blocked_kind.as_deref() == Some("exit_confirmation")
}

fn is_shell_prompt_after_exit(observation: &AgyObservation) -> bool {
    observation.snapshot.reason == "agy:shell_prompt_after_exit"
}

fn selected_clear_command(observation: &AgyObservation) -> bool {
    is_slash_command_surface(observation)
        && observation.text.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("> /clear")
                || trimmed.starts_with("› /clear")
                || trimmed.starts_with("❯ /clear")
        })
}

fn is_pending_clear_command(observation: &AgyObservation) -> bool {
    observation.snapshot.reason == "agy:slash_command_pending"
        && observation.text.lines().any(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("> /clear")
                || trimmed.starts_with("› /clear")
                || trimmed.starts_with("❯ /clear")
        })
}

fn is_home_identity_ready(observation: &AgyObservation) -> bool {
    if !is_ready_for_text(observation) {
        return false;
    }
    let Some(identity) = observation.snapshot.screen_identity.as_ref() else {
        return false;
    };
    identity.cli_version.is_some()
        && identity.account.is_some()
        && identity.current_model.is_some()
        && identity.cwd.is_some()
}

fn slot_status_value(
    slot_id: &str,
    status: Option<&missiond_core::PTYAgentInfo>,
    observation: &AgyObservation,
) -> Value {
    json!({
        "slot_id": slot_id,
        "engine": status.map(|info| info.engine.to_string()),
        "session_state": status.map(|info| info.state),
        "running": status
            .map(|info| !matches!(info.state, SessionState::Exited | SessionState::Error))
            .unwrap_or(false),
        "pid": status.and_then(|info| info.pid),
        "started_at": status.and_then(|info| info.started_at),
        "status_text": status.and_then(|info| info.status_text.clone()),
        "current_task_id": status.and_then(|info| info.current_task_id.clone()),
        "log_file": status.map(|info| info.log_file.display().to_string()),
        "pty_canonical_state": observation.snapshot.state,
        "reason": observation.snapshot.reason.clone(),
        "phase": observation.snapshot.phase.clone(),
        "blocked_kind": observation.snapshot.blocked_kind.clone(),
        "screen_identity": observation.snapshot.screen_identity.clone(),
        "screen_usage": observation.snapshot.screen_usage.clone(),
        "screen_hash": PtyObservation::text("pty-screen", &observation.text).screen_hash,
    })
}

fn mark_control_unverified(
    result: &mut ProviderBoxResult,
    slot_id: &str,
    message: &'static str,
    observation: &AgyObservation,
) {
    result.status = ProviderBoxStatus::Unverified;
    result.add_diagnostic(ProviderBoxDiagnostic::error(
        DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
        message,
        json!({
            "slot_id": slot_id,
            "reason": observation.snapshot.reason,
            "state": observation.snapshot.state,
            "blocked_kind": observation.snapshot.blocked_kind,
        }),
    ));
}

fn current_model_eq(observation: &AgyObservation, target: &str) -> bool {
    let current = observation
        .snapshot
        .screen_identity
        .as_ref()
        .and_then(|identity| identity.current_model.as_deref());
    model_eq(current, target)
}

fn model_eq(observed: Option<&str>, target: &str) -> bool {
    observed
        .map(normalize_model)
        .is_some_and(|observed| observed == normalize_model(target))
}

fn normalize_model(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
}

fn slug_model(value: &str) -> String {
    normalize_model(value)
        .replace("(", "")
        .replace(")", "")
        .replace("/", "-")
        .replace('.', "")
        .replace('+', "plus")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .filter(|value| value.is_ascii_alphanumeric() || *value == '-')
        .collect()
}

fn agy_slot_pool_id(model: &str) -> String {
    format!("slot-pool-agy-{}", slug_model(model))
}

fn extract_model_from_agy_line(line: &str) -> Option<String> {
    let regex = Regex::new(
        r"\b((?:Gemini|Claude|GPT(?:-OSS)?|OpenAI|Grok|Llama|Mistral|Qwen|DeepSeek)[A-Za-z0-9 ._/\-+]*(?:\([^)]+\))?)",
    )
    .expect("valid AGY model regex");
    let cleaned = clean_agy_line(line);
    let value = regex
        .captures(&cleaned)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().trim())?;
    let model = value
        .replace("(current)", "")
        .replace("[current]", "")
        .trim()
        .to_string();
    if model.is_empty() {
        None
    } else {
        Some(model)
    }
}

fn extract_models_from_screen(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for line in text.lines() {
        let cleaned = clean_agy_line(line);
        let lower = cleaned.to_ascii_lowercase();
        if lower.contains("antigravity cli") || lower.contains("switch model") {
            continue;
        }
        if let Some(model) = extract_model_from_agy_line(&cleaned) {
            let normalized = normalize_model(&model);
            if !model.is_empty() && seen.insert(normalized) {
                out.push(model);
            }
        }
    }
    out
}

fn extract_model_picker_entries_from_screen(text: &str) -> Vec<ModelPickerEntry> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut in_latest_picker = false;
    for line in text.lines() {
        let cleaned = clean_agy_line(line);
        let lower = cleaned.to_ascii_lowercase();
        if lower == "switch model" {
            entries.clear();
            seen.clear();
            in_latest_picker = true;
            continue;
        }
        if !in_latest_picker {
            continue;
        }
        if lower.contains("keyboard:")
            || lower.contains("navigate enter select")
            || lower.contains("esc to cancel")
        {
            break;
        }
        let Some(model) = extract_model_from_agy_line(line) else {
            continue;
        };
        let normalized = normalize_model(&model);
        if seen.insert(normalized) {
            entries.push(ModelPickerEntry {
                model,
                selected: picker_line_selected(line),
            });
        }
    }
    entries
}

fn picker_line_selected(line: &str) -> bool {
    line.trim_start()
        .trim_start_matches(|c: char| matches!(c, '│' | '┃' | '║' | '┆' | '┊'))
        .trim_start()
        .chars()
        .next()
        .map(|value| matches!(value, '>' | '›' | '❯' | '▸' | '▶' | '➜' | '→'))
        .unwrap_or(false)
}

fn plan_model_picker_navigation(
    observation: &AgyObservation,
    target_model: &str,
) -> Option<ModelPickerNavigationPlan> {
    let entries = extract_model_picker_entries_from_screen(&observation.text);
    let selected_model = observation
        .snapshot
        .screen_identity
        .as_ref()
        .and_then(|identity| identity.selected_model.as_deref());
    plan_model_picker_navigation_from_entries(&entries, selected_model, target_model)
}

fn plan_model_picker_navigation_from_entries(
    entries: &[ModelPickerEntry],
    selected_model: Option<&str>,
    target_model: &str,
) -> Option<ModelPickerNavigationPlan> {
    let selected_index = selected_model
        .and_then(|selected| {
            entries
                .iter()
                .position(|entry| model_eq(Some(entry.model.as_str()), selected))
        })
        .or_else(|| entries.iter().position(|entry| entry.selected))?;
    let target_index = entries
        .iter()
        .position(|entry| model_eq(Some(entry.model.as_str()), target_model))?;

    if selected_index == target_index {
        return Some(ModelPickerNavigationPlan {
            direction: "down",
            key: "\x1b[B",
            expected_models: Vec::new(),
        });
    }

    if target_index > selected_index {
        Some(ModelPickerNavigationPlan {
            direction: "down",
            key: "\x1b[B",
            expected_models: entries[selected_index + 1..=target_index]
                .iter()
                .map(|entry| entry.model.clone())
                .collect(),
        })
    } else {
        Some(ModelPickerNavigationPlan {
            direction: "up",
            key: "\x1b[A",
            expected_models: entries[target_index..selected_index]
                .iter()
                .rev()
                .map(|entry| entry.model.clone())
                .collect(),
        })
    }
}

fn clean_agy_line(line: &str) -> String {
    line.trim()
        .trim_matches(|c: char| matches!(c, '│' | '┃' | '║' | '┆' | '┊'))
        .trim_start_matches(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '>' | '›' | '❯' | '▸' | '▶' | '➜' | '→' | '*' | '-' | '•' | '●' | '○'
                )
        })
        .trim()
        .to_string()
}

fn usage_screen_value(observation: &AgyObservation) -> Option<Value> {
    observation
        .snapshot
        .screen_usage
        .as_ref()
        .and_then(|usage| serde_json::to_value(usage).ok())
}

fn extract_correlated_turn(
    session: &AgySession,
    correlation_id: &str,
) -> Option<AgyTranscriptOutcome> {
    let start = session
        .steps
        .iter()
        .position(|step| step.step_type == "USER_INPUT" && step_contains(step, correlation_id))?;

    let mut final_text = None;
    for step in session.steps.iter().skip(start + 1) {
        if step.step_type == "USER_INPUT" {
            break;
        }
        if let Some(violation) = pure_text_violation(step) {
            return Some(violation);
        }
        if step.step_type == "PLANNER_RESPONSE" {
            if let Some(content) = &step.content {
                let text = value_text(content);
                if !text.trim().is_empty() {
                    final_text = Some(text);
                }
            }
        }
    }

    final_text.map(|text| {
        AgyTranscriptOutcome::Completed(AgyTurnFinal {
            session_id: session.session_id.clone(),
            transcript_path: String::new(),
            final_text: text,
        })
    })
}

fn pure_text_violation(step: &AgyStep) -> Option<AgyTranscriptOutcome> {
    if step
        .tool_calls
        .as_ref()
        .is_some_and(|calls| !calls.is_empty())
    {
        let tools = step
            .tool_calls
            .as_ref()
            .into_iter()
            .flatten()
            .map(|call| call.name.clone())
            .collect::<Vec<_>>();
        return Some(AgyTranscriptOutcome::Violation {
            code: DIAG_PROVIDER_TEXT_ONLY_VIOLATION.to_string(),
            message: "AGY transcript contains planner tool calls during text-only turn".to_string(),
            details: json!({
                "step_index": step.step_index,
                "step_type": step.step_type,
                "tools": tools,
            }),
        });
    }

    if matches!(
        step.step_type.as_str(),
        "PLANNER_RESPONSE" | "CONVERSATION_HISTORY"
    ) {
        return None;
    }

    let upper = step.step_type.to_ascii_uppercase();
    let class = if upper.contains("RUN") || upper.contains("COMMAND") || upper.contains("SHELL") {
        "shell"
    } else if upper.contains("FILE")
        || upper.contains("DIRECTORY")
        || upper.contains("GREP")
        || upper.contains("SEARCH")
        || upper.contains("READ")
        || upper.contains("WRITE")
        || upper.contains("EDIT")
        || upper.contains("LIST")
        || upper.contains("VIEW")
    {
        "file_access"
    } else if upper.contains("MCP") || upper.contains("APPROVAL") {
        "tool_or_approval"
    } else {
        "non_text_step"
    };
    Some(AgyTranscriptOutcome::Violation {
        code: DIAG_PROVIDER_TEXT_ONLY_VIOLATION.to_string(),
        message: "AGY transcript contains a non-text step during text-only turn".to_string(),
        details: json!({
            "step_index": step.step_index,
            "step_type": step.step_type,
            "class": class,
        }),
    })
}

fn step_contains(step: &AgyStep, needle: &str) -> bool {
    step.content
        .as_ref()
        .map(value_text)
        .unwrap_or_default()
        .contains(needle)
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| item.as_str().map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn build_router_export(
    request: &ProviderInteractionRequest,
    catalog: &ProviderModelCatalog,
) -> ProviderRouterExport {
    let base_url = request
        .router_export_policy
        .as_ref()
        .and_then(|policy| {
            policy
                .get("provider_box_base_url")
                .and_then(Value::as_str)
                .or_else(|| policy.get("managed_proxy_base_url").and_then(Value::as_str))
        })
        .map(str::to_string)
        .or_else(|| std::env::var("MISSIOND_PROVIDER_BOX_PROXY_BASE_URL").ok())
        .or_else(|| std::env::var("MISSIOND_AGY_PROVIDER_BOX_BASE_URL").ok());

    let mut routeable_entries = Vec::new();
    let mut blocked_entries = Vec::new();
    for entry in &catalog.entries {
        let slug = slug_model(&entry.display_name);
        let slot_pool_id = agy_slot_pool_id(&entry.display_name);
        let route = json!({
            "model_id": format!("agy-{slug}"),
            "primary": {
                "provider": "MissionDAgy",
                "provider_model_id": base_url.clone().unwrap_or_default(),
                "billing_id": format!("missiond/agy/{slug}"),
                "timeouts_ms": 300000,
                "capabilities": {
                    "text": true,
                    "tools": false,
                    "vision": false,
                    "files": false,
                    "mcp": false,
                    "shell": false
                },
                "extra": {
                    "provider": "agy_cli",
                    "model": entry.display_name,
                    "slot_pool_id": slot_pool_id,
                    "slot_policy": {
                        "kind": "provider_box_managed_pool",
                        "public_max_concurrent": 1,
                        "replicas_hidden": true,
                        "queue_owner": "provider-box"
                    },
                    "pure_text": true,
                    "allow_model_switch": false,
                    "requires_current_model_verification": true,
                    "completion_endpoint": "/provider-box/v1/text-only/completions"
                }
            }
        });
        if base_url.is_some() {
            routeable_entries.push(route);
        } else {
            blocked_entries.push(json!({
                "entry": route,
                "reason": "managed proxy provider-box base URL missing"
            }));
        }
    }

    let diagnostics = if base_url.is_none() {
        vec![ProviderBoxDiagnostic::warning(
            "PROVIDER_ROUTER_EXPORT_PROXY_URL_MISSING",
            "AGY router export requires MissionD provider-box URL from the self-built proxy deployment program",
            json!({
                "env": [
                    "MISSIOND_PROVIDER_BOX_PROXY_BASE_URL",
                    "MISSIOND_AGY_PROVIDER_BOX_BASE_URL"
                ]
            }),
        )]
    } else {
        Vec::new()
    };

    ProviderRouterExport {
        schema: "missiond.provider-router-export.v1".to_string(),
        export_id: format!("router-export-{}", uuid::Uuid::new_v4().simple()),
        catalog_id: Some(catalog.catalog_id.clone()),
        provider: catalog.provider.clone(),
        engine: CliEngine::Agy,
        router_backend_ids: vec!["xjp-router:MissionDAgy".to_string()],
        routeable_entries,
        blocked_entries,
        policy_ref: Some("interactive-provider-box/MissionDAgy/text-only".to_string()),
        diagnostics,
    }
}

fn default_agy_home() -> PathBuf {
    std::env::var_os("MISSIOND_AGY_CLI_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".gemini").join("antigravity-cli")))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use missiond_core::agy_cli::{AgySession, AgyStep, AgyToolCall};

    use super::*;

    fn observation(input: &[&str]) -> AgyObservation {
        let lines = input
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        let snapshot = recognize_screen(CliEngine::Agy, &lines, missiond_core::SessionState::Idle);
        let text = lines.join("\n");
        AgyObservation {
            lines,
            text,
            snapshot,
        }
    }

    fn step(index: i64, step_type: &str, content: Option<Value>) -> AgyStep {
        AgyStep {
            step_index: index,
            source: "MODEL".to_string(),
            step_type: step_type.to_string(),
            status: Some("DONE".to_string()),
            created_at: Some("2026-05-31T00:00:00Z".to_string()),
            content,
            thinking: None,
            tool_calls: None,
        }
    }

    #[test]
    fn agy_driver_does_not_treat_slash_menu_as_ready_text_surface() {
        let obs = observation(&[
            "────────────────────────────────────────",
            "> /c",
            "────────────────────────────────────────",
            "/changelog        Show release notes and changes",
            "> /clear          Clear conversation and start a new one",
            "/config           Open settings panel",
            "↑/↓ Navigate · enter Select · tab Complete",
            "esc to cancel                                                                                    Gemini 3.5 Flash (High)",
        ]);

        assert!(is_overlay_screen(&obs));
        assert!(is_slash_command_surface(&obs));
        assert!(!is_ready_for_text(&obs));
        assert!(!is_hard_blocked(&obs));
    }

    #[test]
    fn agy_driver_treats_completed_slash_command_as_pending_input() {
        let obs = observation(&[
            "────────────────────────────────────────",
            "> /clear",
            "────────────────────────────────────────",
            "? for shortcuts                                                                                  Gemini 3.5 Flash (High)",
        ]);

        assert!(is_overlay_screen(&obs));
        assert!(is_slash_command_surface(&obs));
        assert!(!is_ready_for_text(&obs));
    }

    #[test]
    fn agy_driver_verifies_clear_selection_and_home_identity() {
        let selected = observation(&[
            "────────────────────────────────────────",
            "> /c",
            "────────────────────────────────────────",
            "/changelog        Show release notes and changes",
            "> /clear          Clear conversation and start a new one",
            "/config           Open settings panel",
            "↑/↓ Navigate · enter Select · tab Complete",
            "esc to cancel                                                                                    Gemini 3.5 Flash (High)",
        ]);
        let home = observation(&[
            "Antigravity CLI 1.0.3",
            "jjrrqqq@gmail.com (Google AI Ultra)",
            "Gemini 3.5 Flash (High)",
            "~/Projects/missiond",
            "────────────────────────────────────────",
            ">",
            "────────────────────────────────────────",
            "? for shortcuts                                                                                  Gemini 3.5 Flash (High)",
        ]);

        assert!(selected_clear_command(&selected));
        assert!(is_home_identity_ready(&home));
    }

    #[test]
    fn agy_driver_prefers_exit_slash_command_bytes() {
        assert_eq!(AGY_EXIT_COMMAND, "/exit");
    }

    #[test]
    fn agy_driver_keeps_ctrl_d_confirmation_as_exit_fallback_state() {
        let obs = observation(&[
            "Antigravity CLI 1.0.3",
            "jjrrqqq@gmail.com (Google AI Ultra)",
            "Gemini 3.5 Flash (High)",
            "~/Projects/missiond",
            "────────────────────────────────────────",
            ">",
            "────────────────────────────────────────",
            "press ctrl+d again to exit                                                                        Gemini 3.5 Flash (High)",
        ]);

        assert!(is_exit_confirm_pending(&obs));
        assert!(!is_overlay_screen(&obs));
        assert!(!is_ready_for_text(&obs));
        assert!(is_hard_blocked(&obs));
    }

    #[test]
    fn agy_driver_recognizes_shell_prompt_after_exit_as_not_ready() {
        let obs = observation(&[
            "Resume with:",
            "  agy --conversation=917a5c67-e5b7-467a-8cfa-0d142faa474a",
            "  agy -c",
            "Resume: agy --conversation=917a5c67-e5b7-467a-8cfa-0d142faa474a (or -c)",
            "(base) jinchen@Mac missiond %",
        ]);

        assert!(is_shell_prompt_after_exit(&obs));
        assert!(!is_ready_for_text(&obs));
    }

    #[test]
    fn correlated_turn_extracts_final_after_matching_user() {
        let session = AgySession {
            session_id: "conv-1".to_string(),
            steps: vec![
                step(
                    1,
                    "USER_INPUT",
                    Some(json!("Correlation-ID: corr-1\nhello")),
                ),
                step(2, "PLANNER_RESPONSE", Some(json!("final answer"))),
            ],
        };

        let outcome = extract_correlated_turn(&session, "corr-1").expect("matched");

        match outcome {
            AgyTranscriptOutcome::Completed(turn) => {
                assert_eq!(turn.session_id, "conv-1");
                assert_eq!(turn.final_text, "final answer");
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    #[test]
    fn correlated_turn_rejects_tool_call_before_final() {
        let mut tool_step = step(2, "PLANNER_RESPONSE", None);
        tool_step.tool_calls = Some(vec![AgyToolCall {
            name: "run_command".to_string(),
            args: json!({"cmd": "pwd"}),
        }]);
        let session = AgySession {
            session_id: "conv-1".to_string(),
            steps: vec![
                step(1, "USER_INPUT", Some(json!("Correlation-ID: corr-2"))),
                tool_step,
            ],
        };

        let outcome = extract_correlated_turn(&session, "corr-2").expect("matched");

        assert!(matches!(outcome, AgyTranscriptOutcome::Violation { .. }));
    }

    #[test]
    fn model_catalog_extractor_reads_visible_agy_models() {
        let models = extract_models_from_screen(
            "Switch Model\n> Gemini 3.5 Flash (High)\n  Claude Sonnet 4.6 (Thinking)\nesc to cancel",
        );

        assert_eq!(
            models,
            vec![
                "Gemini 3.5 Flash (High)".to_string(),
                "Claude Sonnet 4.6 (Thinking)".to_string()
            ]
        );
    }

    #[test]
    fn model_picker_navigation_plan_counts_visible_down_steps() {
        let entries = extract_model_picker_entries_from_screen(
            "Antigravity CLI 1.0.3\n\
             Gemini 3.5 Flash (Medium)\n\
             Switch Model\n\
             > Gemini 3.5 Flash (Medium)    (current)\n\
               Gemini 3.5 Flash (High)\n\
               Gemini 3.5 Flash (Low)\n\
               Gemini 3.1 Pro (Low)\n\
               Gemini 3.1 Pro (High)\n\
               Claude Sonnet 4.6 (Thinking)\n\
               Claude Opus 4.6 (Thinking)\n\
               GPT-OSS 120B (Medium)\n\
             Keyboard: up/down\n\
             esc to cancel Gemini 3.5 Flash (Medium)",
        );

        let plan = plan_model_picker_navigation_from_entries(
            &entries,
            Some("Gemini 3.5 Flash (Medium)"),
            "Claude Opus 4.6 (Thinking)",
        )
        .expect("navigation plan");

        assert_eq!(plan.direction, "down");
        assert_eq!(plan.expected_models.len(), 6);
        assert_eq!(
            plan.expected_models.last().map(String::as_str),
            Some("Claude Opus 4.6 (Thinking)")
        );
    }

    #[test]
    fn model_picker_navigation_plan_counts_visible_up_steps() {
        let entries = extract_model_picker_entries_from_screen(
            "Switch Model\n\
               Gemini 3.5 Flash (Medium)\n\
               Gemini 3.5 Flash (High)\n\
               Gemini 3.5 Flash (Low)\n\
             > Gemini 3.1 Pro (Low)\n\
               Gemini 3.1 Pro (High)\n\
             Keyboard: up/down",
        );

        let plan = plan_model_picker_navigation_from_entries(
            &entries,
            Some("Gemini 3.1 Pro (Low)"),
            "Gemini 3.5 Flash (High)",
        )
        .expect("navigation plan");

        assert_eq!(plan.direction, "up");
        assert_eq!(
            plan.expected_models,
            vec![
                "Gemini 3.5 Flash (Low)".to_string(),
                "Gemini 3.5 Flash (High)".to_string()
            ]
        );
    }

    #[test]
    fn router_export_blocks_without_managed_proxy_base_url() {
        let mut request = ProviderInteractionRequest::new(
            super::super::types::BoxCommand::ModelCatalogExport,
            CliEngine::Agy,
        );
        request.router_export_policy = Some(json!({}));
        let catalog = ProviderModelCatalog {
            schema: "missiond.provider-model-catalog.v1".to_string(),
            catalog_id: "catalog-1".to_string(),
            provider: Some("agy_cli".to_string()),
            engine: CliEngine::Agy,
            account_ref: None,
            discovered_at: "2026-05-31T00:00:00Z".to_string(),
            source: Some("agy:/model".to_string()),
            entries: vec![ProviderModelCatalogEntry {
                provider_model_id: "agy:gemini-35-flash-high".to_string(),
                display_name: "Gemini 3.5 Flash (High)".to_string(),
                family: Some("Gemini".to_string()),
                routeable_default: true,
                switch_capability: "interactive_model_picker".to_string(),
                usage_probe_capability: "interactive_usage_screen".to_string(),
                confidence: 0.9,
            }],
            diagnostics: Vec::new(),
        };

        let export = build_router_export(&request, &catalog);

        assert!(export.routeable_entries.is_empty());
        assert_eq!(export.blocked_entries.len(), 1);
    }

    #[test]
    fn router_export_hides_text_only_replica_slots_for_opus() {
        let mut request = ProviderInteractionRequest::new(
            super::super::types::BoxCommand::ModelCatalogExport,
            CliEngine::Agy,
        );
        request.router_export_policy = Some(json!({
            "provider_box_base_url": "https://missiond.example/provider-box"
        }));
        let catalog = ProviderModelCatalog {
            schema: "missiond.provider-model-catalog.v1".to_string(),
            catalog_id: "catalog-1".to_string(),
            provider: Some("agy_cli".to_string()),
            engine: CliEngine::Agy,
            account_ref: None,
            discovered_at: "2026-05-31T00:00:00Z".to_string(),
            source: Some("agy:/model".to_string()),
            entries: vec![ProviderModelCatalogEntry {
                provider_model_id: "agy:claude-opus-46-thinking".to_string(),
                display_name: "Claude Opus 4.6 (Thinking)".to_string(),
                family: Some("Claude".to_string()),
                routeable_default: true,
                switch_capability: "interactive_model_picker".to_string(),
                usage_probe_capability: "interactive_usage_screen".to_string(),
                confidence: 0.9,
            }],
            diagnostics: Vec::new(),
        };

        let export = build_router_export(&request, &catalog);

        assert_eq!(export.routeable_entries.len(), 1);
        let extra = &export.routeable_entries[0]["primary"]["extra"];
        assert!(extra.get("slot_id").is_none());
        assert!(extra.get("slot_ids").is_none());
        assert_eq!(
            extra["slot_pool_id"],
            "slot-pool-agy-claude-opus-46-thinking"
        );
        assert_eq!(extra["slot_policy"]["replicas_hidden"], true);
        assert_eq!(extra["slot_policy"]["public_max_concurrent"], 1);
        assert_eq!(extra["slot_policy"]["queue_owner"], "provider-box");
        assert_eq!(extra["allow_model_switch"], false);
        assert_eq!(extra["requires_current_model_verification"], true);
    }
}
