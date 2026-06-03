use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use missiond_core::agy_cli::{discover_sessions, parse_session, AgySession, AgyStep, HistoryIndex};
use missiond_core::pty::{recognize_screen, PtyCanonicalState};
use missiond_core::types::CliEngine;
use missiond_core::{PTYManager, PTYSlot, PTYSpawnOptions, SessionState};
use regex::Regex;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use super::driver::{ProviderDriver, ProviderDriverCapabilities};
use super::types::{
    BoxCommand, ModelSwitchResult, ModelSwitchStatus, ProviderBoxDiagnostic, ProviderBoxResult,
    ProviderBoxStatus, ProviderControlAction, ProviderInteractionRequest, ProviderModelCatalog,
    ProviderModelCatalogEntry, ProviderModelUsage, ProviderRouterExport, ProviderUsageSnapshot,
    ProviderUsageStatus, PtyObservation, PtyStepAction, PtyStepRecord, PtyStepVerificationStatus,
    TimeoutCancelPolicy, DIAG_MODEL_SWITCH_UNVERIFIED, DIAG_PROVIDER_BOX_INVALID_REQUEST,
    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE, DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
    DIAG_PROVIDER_DURABLE_FINAL_MISSING, DIAG_PROVIDER_MCP_RECONNECT_UNVERIFIED,
    DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE, DIAG_PROVIDER_STATUS_UNAVAILABLE,
    DIAG_PROVIDER_TEXT_ONLY_VIOLATION, DIAG_PROVIDER_TURN_STALLED,
    DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED, DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED,
    DIAG_USAGE_UNKNOWN,
};

const DEFAULT_AGY_SLOT: &str = "slot-agy-provider-box";
const AGY_CTRL_D: &str = "\x1b[100;5u";
const AGY_EXIT_COMMAND: &str = "/exit";
const MODEL_PICKER_MAX_DOWN: usize = 96;
const MODEL_CATALOG_MAX_DOWN: usize = 128;
const OBSERVE_SETTLE_MS: u64 = 320;
const OBSERVE_STABLE_POLL_MS: u64 = 120;
const OBSERVE_STABLE_MAX_MS: u64 = 1_000;
const AGY_STARTUP_SIGN_IN_WAIT_SECS: u64 = 10;
const AGY_STARTUP_READY_WAIT_SECS: u64 = 30;
const AGY_DURABLE_FINAL_IDLE_GRACE_DEFAULT_SECS: u64 = 90;
const AGY_DURABLE_FINAL_IDLE_GRACE_MIN_SECS: u64 = 10;
const AGY_DURABLE_FINAL_IDLE_GRACE_MAX_SECS: u64 = 300;
const AGY_PROMPT_SEND_READY_WAIT_SECS: u64 = 8;
const AGY_MANUAL_TEXT_LIMIT: usize = 4096;
const AGY_USAGE_MAX_PAGES: usize = 8;
const AGY_TEXT_SLOT_IDLE_RECLAIM_SECS: u64 = 2 * 60 * 60;
const AGY_TEXT_SLOT_REAPER_INTERVAL_SECS: u64 = 10 * 60;
const AGY_MANUAL_KEY_NAMES: &[&str] = &[
    "enter",
    "escape",
    "up",
    "down",
    "left",
    "right",
    "tab",
    "backspace",
    "delete",
    "ctrl+c",
    "ctrl+d",
    "pageup",
    "pagedown",
    "home",
    "end",
];

#[derive(Clone)]
pub(crate) struct AgyProviderDriver {
    pty: Arc<PTYManager>,
    slot_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    slot_last_used: Arc<Mutex<HashMap<String, Instant>>>,
    text_turn_global_lock: Arc<Mutex<()>>,
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
struct AgyTranscriptCursor {
    workspace: Option<PathBuf>,
    sessions: HashMap<String, AgySessionCursor>,
}

#[derive(Debug, Clone)]
struct AgySessionCursor {
    transcript_path: PathBuf,
    step_count: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgyStartupOutcome {
    Ready,
    RestartRequired,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgyTrustSelection {
    Trust,
    Exit,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgyManualPtyStep {
    action_type: String,
    key: Option<String>,
    text: Option<String>,
    expected_change: Option<String>,
    redacted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgyMcpServerEntry {
    name: String,
    selected: bool,
    connected: bool,
    status: String,
    tools_summary: Option<String>,
    error: Option<String>,
}

impl AgyProviderDriver {
    pub(crate) fn new(pty: Arc<PTYManager>) -> Self {
        let slot_last_used = Arc::new(Mutex::new(HashMap::new()));
        start_agy_text_slot_reaper(Arc::clone(&pty), Arc::clone(&slot_last_used));
        Self {
            pty,
            slot_locks: Arc::new(Mutex::new(HashMap::new())),
            slot_last_used,
            text_turn_global_lock: Arc::new(Mutex::new(())),
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

    async fn mark_text_slot_used(&self, slot_id: &str) {
        if is_private_agy_text_slot_id(slot_id) {
            self.slot_last_used
                .lock()
                .await
                .insert(slot_id.to_string(), Instant::now());
        }
    }

    async fn reclaim_idle_text_slots(&self) {
        reclaim_idle_agy_text_slots(&self.pty, &self.slot_last_used).await;
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

    fn request_force_restart(request: &ProviderInteractionRequest) -> bool {
        request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("force_restart")
                    .or_else(|| worker.get("restart"))
                    .or_else(|| worker.get("respawn"))
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

    fn request_mcp_server(request: &ProviderInteractionRequest) -> String {
        request
            .desired_worker
            .as_ref()
            .and_then(|worker| {
                worker
                    .get("mcp_server")
                    .or_else(|| worker.get("server"))
                    .and_then(Value::as_str)
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("missiond")
            .to_string()
    }

    fn request_manual_pty_step(
        request: &ProviderInteractionRequest,
    ) -> Result<AgyManualPtyStep, ProviderBoxDiagnostic> {
        let step = request
            .desired_worker
            .as_ref()
            .and_then(|worker| worker.get("pty_step").or(Some(worker)))
            .ok_or_else(|| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "AGY PTY step requires desired_worker.pty_step",
                    json!({
                        "slot_id": request.slot_id,
                        "command": request.command,
                    }),
                )
            })?;
        let action_type = step
            .get("action_type")
            .or_else(|| step.get("type"))
            .and_then(Value::as_str)
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .or_else(|| {
                if step.get("key").is_some() {
                    Some("key".to_string())
                } else if step.get("text").is_some() {
                    Some("text".to_string())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "AGY PTY step requires action_type=key or action_type=text",
                    json!({
                        "slot_id": request.slot_id,
                        "allowed_action_types": ["key", "text"],
                    }),
                )
            })?;
        let redacted = step
            .get("redacted")
            .and_then(Value::as_bool)
            .unwrap_or(action_type == "text" || action_type == "paste");
        let expected_change = step
            .get("expected_change")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        match action_type.as_str() {
            "key" => {
                let key = step
                    .get("key")
                    .or_else(|| step.get("human_input"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        ProviderBoxDiagnostic::error(
                            DIAG_PROVIDER_BOX_INVALID_REQUEST,
                            "AGY key PTY step requires a key name",
                            json!({
                                "slot_id": request.slot_id,
                                "allowed_keys": AGY_MANUAL_KEY_NAMES,
                            }),
                        )
                    })?;
                if agy_manual_key_bytes(&key).is_none() {
                    return Err(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_INVALID_REQUEST,
                        "AGY key PTY step uses an unsupported key",
                        json!({
                            "slot_id": request.slot_id,
                            "key": key,
                            "allowed_keys": AGY_MANUAL_KEY_NAMES,
                        }),
                    ));
                }
                Ok(AgyManualPtyStep {
                    action_type,
                    key: Some(key),
                    text: None,
                    expected_change,
                    redacted: false,
                })
            }
            "text" | "paste" => {
                let text = step
                    .get("text")
                    .or_else(|| step.get("human_input"))
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| {
                        ProviderBoxDiagnostic::error(
                            DIAG_PROVIDER_BOX_INVALID_REQUEST,
                            "AGY text PTY step requires text",
                            json!({
                                "slot_id": request.slot_id,
                            }),
                        )
                    })?;
                validate_manual_text_step(request.slot_id.as_deref(), &text)?;
                Ok(AgyManualPtyStep {
                    action_type: "text".to_string(),
                    key: None,
                    text: Some(text),
                    expected_change,
                    redacted,
                })
            }
            _ => Err(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "AGY PTY step action_type is unsupported",
                json!({
                    "slot_id": request.slot_id,
                    "action_type": action_type,
                    "allowed_action_types": ["key", "text"],
                }),
            )),
        }
    }

    fn slot_for_request(request: &ProviderInteractionRequest, slot_id: &str) -> PTYSlot {
        let cwd = request
            .cwd
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| {
                if request.command == BoxCommand::PureTextSingleTurn
                    && is_private_agy_text_slot_id(slot_id)
                {
                    Some(agy_text_only_workspace(slot_id))
                } else {
                    request.project_root.as_ref().map(PathBuf::from)
                }
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
        if let Err(err) = std::fs::create_dir_all(&cwd) {
            warn!(
                slot_id = %slot_id,
                cwd = %cwd.display(),
                error = %err,
                "Failed to create AGY provider-box slot cwd before spawn"
            );
        }
        if request.command == BoxCommand::PureTextSingleTurn && is_private_agy_text_slot_id(slot_id)
        {
            if let Err(err) = prepare_text_only_workspace(&cwd, slot_id) {
                warn!(
                    slot_id = %slot_id,
                    cwd = %cwd.display(),
                    error = %err,
                    "Failed to prepare AGY text-only workspace policy evidence"
                );
            }
        }
        PTYSlot {
            id: slot_id.to_string(),
            role: "provider-box-agy".to_string(),
            cwd: Some(cwd),
            engine: CliEngine::Agy,
        }
    }

    fn spawn_options() -> PTYSpawnOptions {
        PTYSpawnOptions {
            auto_restart: true,
            wait_for_idle: false,
            timeout_secs: Some(90),
            ..Default::default()
        }
    }

    fn spawn_options_for_request(
        request: &ProviderInteractionRequest,
        slot_id: &str,
    ) -> PTYSpawnOptions {
        let mut options = Self::spawn_options();
        if request.command == BoxCommand::PureTextSingleTurn && is_private_agy_text_slot_id(slot_id)
        {
            options.sandbox = Some("text-only".to_string());
            options.extra_env.insert(
                "MISSIOND_PROVIDER_BOX_TEXT_ONLY".to_string(),
                "1".to_string(),
            );
            options.extra_env.insert(
                "MISSIOND_PROVIDER_BOX_SLOT_ID".to_string(),
                slot_id.to_string(),
            );
        }
        options
    }

    async fn ensure_slot(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<String> {
        let slot_id = Self::request_slot_id(request);
        let slot = Self::slot_for_request(request, &slot_id);
        let options = Self::spawn_options_for_request(request, &slot_id);
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
            if Self::request_force_restart(request) {
                result.add_diagnostic(ProviderBoxDiagnostic::warning(
                    DIAG_PROVIDER_TURN_STALLED,
                    "AGY slot force_restart requested; restarting slot before use",
                    json!({
                        "slot_id": slot_id,
                        "session_state": status.state,
                    }),
                ));
                if let Err(err) = self.pty.kill(&slot_id).await {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                        "AGY force_restart failed while killing the existing slot",
                        json!({
                            "slot_id": slot_id,
                            "error": err.to_string(),
                        }),
                    ));
                    return None;
                }
                return self.spawn_slot_with_bootstrap(&slot, options, result).await;
            }
            if !matches!(status.state, SessionState::Exited | SessionState::Error) {
                let observation = self.observe(&slot_id).await;
                if should_resolve_agy_startup_surface(status.state, &observation) {
                    let lock = self.slot_lock(&slot_id).await;
                    let guard = lock.lock().await;
                    match self.ensure_startup_ready_locked(result, &slot_id).await {
                        AgyStartupOutcome::Ready => {}
                        AgyStartupOutcome::RestartRequired => {
                            self.abort_startup_for_restart_locked(result, &slot_id)
                                .await;
                            drop(guard);
                            return self.spawn_slot_with_bootstrap(&slot, options, result).await;
                        }
                        AgyStartupOutcome::Failed => return None,
                    }
                }
                return Some(slot_id);
            }
        }

        self.spawn_slot_with_bootstrap(&slot, options, result).await
    }

    async fn spawn_slot_with_bootstrap(
        &self,
        slot: &PTYSlot,
        options: PTYSpawnOptions,
        result: &mut ProviderBoxResult,
    ) -> Option<String> {
        self.pty.init_slot(slot).await;
        for attempt in 0..2 {
            match self.pty.spawn(slot, options.clone()).await {
                Ok(_) => {}
                Err(err) => {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                        "AGY PTY slot could not be spawned",
                        json!({
                            "slot_id": slot.id,
                            "attempt": attempt,
                            "error": err.to_string(),
                        }),
                    ));
                    return None;
                }
            }

            let lock = self.slot_lock(&slot.id).await;
            let guard = lock.lock().await;
            match self.ensure_startup_ready_locked(result, &slot.id).await {
                AgyStartupOutcome::Ready => return Some(slot.id.clone()),
                AgyStartupOutcome::Failed => return None,
                AgyStartupOutcome::RestartRequired if attempt == 0 => {
                    self.abort_startup_for_restart_locked(result, &slot.id)
                        .await;
                    drop(guard);
                    continue;
                }
                AgyStartupOutcome::RestartRequired => {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                        "AGY startup sign-in did not complete after restart",
                        json!({
                            "slot_id": slot.id,
                            "attempt": attempt,
                        }),
                    ));
                    return None;
                }
            }
        }
        None
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

    fn observations_equivalent(left: &AgyObservation, right: &AgyObservation) -> bool {
        left.text == right.text
            && left.snapshot.state == right.snapshot.state
            && left.snapshot.reason == right.snapshot.reason
            && left.snapshot.blocked_kind == right.snapshot.blocked_kind
    }

    fn observations_changed(before: &AgyObservation, after: &AgyObservation) -> bool {
        !Self::observations_equivalent(before, after)
    }

    async fn observe_after_action(&self, slot_id: &str) -> AgyObservation {
        let started = Instant::now();
        tokio::time::sleep(Duration::from_millis(OBSERVE_SETTLE_MS)).await;
        let mut previous = self.observe(slot_id).await;

        loop {
            if started.elapsed() >= Duration::from_millis(OBSERVE_STABLE_MAX_MS) {
                return previous;
            }

            tokio::time::sleep(Duration::from_millis(OBSERVE_STABLE_POLL_MS)).await;
            let current = self.observe(slot_id).await;
            if Self::observations_equivalent(&previous, &current) {
                return current;
            }
            previous = current;
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
        let after = self.observe_after_action(slot_id).await;
        let status = if write_result.is_err() {
            PtyStepVerificationStatus::Failed
        } else if Self::observations_changed(&before, &after) {
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

    async fn wait_step_until<F>(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        timeout: Duration,
        expected_change: impl Into<Option<String>>,
        mut predicate: F,
    ) -> AgyObservation
    where
        F: FnMut(&AgyObservation) -> bool,
    {
        let before = self.observe(slot_id).await;
        let after = self
            .wait_until(slot_id, timeout, |obs| predicate(obs))
            .await;
        let status = if predicate(&after) {
            PtyStepVerificationStatus::Verified
        } else if before.text != after.text {
            PtyStepVerificationStatus::Ambiguous
        } else {
            PtyStepVerificationStatus::Unchanged
        };
        result.record_step(PtyStepRecord::new(
            Self::pty_observation(slot_id, &before),
            PtyStepAction {
                action_type: "wait".to_string(),
                human_input: format!("wait {}s", timeout.as_secs()),
                redacted: false,
            },
            Self::pty_observation(slot_id, &after),
            expected_change.into(),
            status,
        ));
        after
    }

    async fn ensure_startup_ready_locked(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> AgyStartupOutcome {
        let mut observation = self.observe(slot_id).await;
        if is_starting_or_unknown(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(AGY_STARTUP_READY_WAIT_SECS),
                    Some("wait for AGY startup surface".to_string()),
                    |obs| {
                        is_ready_for_text(obs)
                            || is_agy_startup_signing_in(obs)
                            || is_workspace_trust_prompt(obs)
                            || is_shell_prompt_after_exit(obs)
                            || obs.snapshot.state == PtyCanonicalState::Blocked
                    },
                )
                .await;
        }

        if is_agy_startup_signing_in(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(AGY_STARTUP_SIGN_IN_WAIT_SECS),
                    Some("wait for AGY sign-in bootstrap to leave signing-in screen".to_string()),
                    |obs| !is_agy_startup_signing_in(obs),
                )
                .await;
            if is_agy_startup_signing_in(&observation) {
                result.add_diagnostic(ProviderBoxDiagnostic::warning(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "AGY startup sign-in screen stayed active past the grace window; slot will be restarted",
                    json!({
                        "slot_id": slot_id,
                        "timeout_secs": AGY_STARTUP_SIGN_IN_WAIT_SECS,
                        "reason": observation.snapshot.reason,
                    }),
                ));
                return AgyStartupOutcome::RestartRequired;
            }
        }

        if is_workspace_trust_prompt(&observation) {
            observation = match self
                .accept_workspace_trust_locked(result, slot_id, observation)
                .await
            {
                Some(observation) => observation,
                None => return AgyStartupOutcome::Failed,
            };
        }

        if is_ready_for_text(&observation) {
            return AgyStartupOutcome::Ready;
        }

        if observation.snapshot.state == PtyCanonicalState::Running {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(3),
                    Some("wait for AGY startup processing to settle".to_string()),
                    |obs| is_ready_for_text(obs) || is_workspace_trust_prompt(obs),
                )
                .await;
            if is_workspace_trust_prompt(&observation) {
                observation = match self
                    .accept_workspace_trust_locked(result, slot_id, observation)
                    .await
                {
                    Some(observation) => observation,
                    None => return AgyStartupOutcome::Failed,
                };
            }
            if is_ready_for_text(&observation) {
                return AgyStartupOutcome::Ready;
            }
        }

        result.status = ProviderBoxStatus::Blocked;
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
            "AGY startup did not reach a ready composer",
            json!({
                "slot_id": slot_id,
                "state": observation.snapshot.state,
                "reason": observation.snapshot.reason,
                "blocked_kind": observation.snapshot.blocked_kind,
            }),
        ));
        AgyStartupOutcome::Failed
    }

    async fn accept_workspace_trust_locked(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        mut observation: AgyObservation,
    ) -> Option<AgyObservation> {
        match selected_workspace_trust_option(&observation) {
            AgyTrustSelection::Trust => {}
            AgyTrustSelection::Exit => {
                let mut selected_trust = false;
                for (key_name, bytes, label) in [
                    ("up", "\x1b[A", "move AGY workspace trust selection up"),
                    ("down", "\x1b[B", "move AGY workspace trust selection down"),
                ] {
                    observation = self
                        .write_step(
                            result,
                            slot_id,
                            PtyStepAction::key(key_name),
                            bytes,
                            Some(label.to_string()),
                        )
                        .await;
                    if selected_workspace_trust_option(&observation) == AgyTrustSelection::Trust {
                        selected_trust = true;
                        break;
                    }
                }
                if !selected_trust {
                    result.status = ProviderBoxStatus::Unverified;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                        "AGY workspace trust prompt could not be moved to the trust option",
                        json!({
                            "slot_id": slot_id,
                            "reason": observation.snapshot.reason,
                        }),
                    ));
                    return None;
                }
            }
            AgyTrustSelection::Unknown => {
                result.status = ProviderBoxStatus::Unverified;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                    "AGY workspace trust prompt selection was not recognizable",
                    json!({
                        "slot_id": slot_id,
                        "reason": observation.snapshot.reason,
                    }),
                ));
                return None;
            }
        }

        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("confirm AGY workspace trust selection".to_string()),
            )
            .await;
        if !is_ready_for_text(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(AGY_STARTUP_READY_WAIT_SECS),
                    Some("wait for AGY composer after workspace trust confirmation".to_string()),
                    is_ready_for_text,
                )
                .await;
        }
        if is_ready_for_text(&observation) {
            Some(observation)
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
                "AGY workspace trust confirmation did not reach the composer",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                    "blocked_kind": observation.snapshot.blocked_kind,
                }),
            ));
            None
        }
    }

    async fn abort_startup_for_restart_locked(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let mut observation = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("ctrl+c"),
                "\x03",
                Some("interrupt stalled AGY startup sign-in".to_string()),
            )
            .await;
        if !is_shell_prompt_after_exit(&observation) {
            observation = self
                .wait_step_until(
                    result,
                    slot_id,
                    Duration::from_secs(2),
                    Some("wait for AGY startup interrupt to settle".to_string()),
                    |obs| {
                        is_shell_prompt_after_exit(obs)
                            || obs.snapshot.state == PtyCanonicalState::Complete
                    },
                )
                .await;
        }
        if !is_shell_prompt_after_exit(&observation) {
            let _ = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("ctrl+d"),
                    AGY_CTRL_D,
                    Some("send Ctrl+D fallback for stalled AGY startup".to_string()),
                )
                .await;
        }
        if let Err(err) = self.pty.kill(slot_id).await {
            result.add_diagnostic(ProviderBoxDiagnostic::warning(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "AGY stalled startup slot did not close cleanly before restart",
                json!({
                    "slot_id": slot_id,
                    "error": err.to_string(),
                }),
            ));
        }
    }

    async fn ensure_composer_ready(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> Option<AgyObservation> {
        let mut observation = self.observe(slot_id).await;
        if should_resolve_agy_startup_surface(
            self.pty
                .get_status(slot_id)
                .await
                .as_ref()
                .map(|status| status.state)
                .unwrap_or(SessionState::Idle),
            &observation,
        ) {
            match self.ensure_startup_ready_locked(result, slot_id).await {
                AgyStartupOutcome::Ready => {
                    observation = self.observe(slot_id).await;
                }
                AgyStartupOutcome::RestartRequired => {
                    result.status = ProviderBoxStatus::Blocked;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                        "AGY startup requires a restart before text input is safe",
                        json!({
                            "slot_id": slot_id,
                        }),
                    ));
                    return None;
                }
                AgyStartupOutcome::Failed => return None,
            }
        }

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
            return self
                .wait_for_prompt_send_ready(result, slot_id, observation)
                .await;
        }

        if observation.snapshot.state == PtyCanonicalState::Running {
            observation = self
                .wait_until(slot_id, Duration::from_secs(2), |obs| {
                    is_ready_for_text(obs)
                })
                .await;
            if is_ready_for_text(&observation) {
                return self
                    .wait_for_prompt_send_ready(result, slot_id, observation)
                    .await;
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

    async fn wait_for_prompt_send_ready(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        initial: AgyObservation,
    ) -> Option<AgyObservation> {
        let before = initial.clone();
        let mut observation = initial;
        let mut last_session_state = self
            .pty
            .get_status(slot_id)
            .await
            .as_ref()
            .map(|status| status.state);
        if is_ready_for_text(&observation) && matches!(last_session_state, Some(SessionState::Idle))
        {
            return Some(observation);
        }

        let started = Instant::now();
        loop {
            if started.elapsed() >= Duration::from_secs(AGY_PROMPT_SEND_READY_WAIT_SECS) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            observation = self.observe(slot_id).await;
            last_session_state = self
                .pty
                .get_status(slot_id)
                .await
                .as_ref()
                .map(|status| status.state);
            if is_ready_for_text(&observation)
                && matches!(last_session_state, Some(SessionState::Idle))
            {
                result.record_step(PtyStepRecord::new(
                    Self::pty_observation(slot_id, &before),
                    PtyStepAction {
                        action_type: "wait".to_string(),
                        human_input: format!(
                            "wait until AGY PTY session state is idle before prompt submit ({}s)",
                            AGY_PROMPT_SEND_READY_WAIT_SECS
                        ),
                        redacted: false,
                    },
                    Self::pty_observation(slot_id, &observation),
                    Some("AGY screen and PTY session are both ready for prompt input".to_string()),
                    PtyStepVerificationStatus::Verified,
                ));
                return Some(observation);
            }
        }

        let mut step = PtyStepRecord::new(
            Self::pty_observation(slot_id, &before),
            PtyStepAction {
                action_type: "wait".to_string(),
                human_input: format!(
                    "wait until AGY PTY session state is idle before prompt submit ({}s)",
                    AGY_PROMPT_SEND_READY_WAIT_SECS
                ),
                redacted: false,
            },
            Self::pty_observation(slot_id, &observation),
            Some("AGY screen and PTY session are both ready for prompt input".to_string()),
            PtyStepVerificationStatus::Failed,
        );
        step.diagnostics.push(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
            "AGY screen is ready, but PTY session state did not become idle for prompt submission",
            json!({
                "slot_id": slot_id,
                "session_state": last_session_state,
                "screen_state": observation.snapshot.state,
                "reason": observation.snapshot.reason,
            }),
        ));
        result.record_step(step);
        result.status = ProviderBoxStatus::Blocked;
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
            "AGY slot is not ready for prompt submission",
            json!({
                "slot_id": slot_id,
                "session_state": last_session_state,
                "screen_state": observation.snapshot.state,
                "reason": observation.snapshot.reason,
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
            tokio::time::sleep(Duration::from_millis(600)).await;
            let settled = self.observe(slot_id).await;
            if is_model_picker(&settled) {
                Some(settled)
            } else {
                Some(observation)
            }
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

        for _ in 0..3 {
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
                if !is_model_picker(&observation) {
                    if current_model_eq(&observation, target_model) {
                        result.model_switch_result = Some(ModelSwitchResult {
                            status: ModelSwitchStatus::Verified,
                            requested_model: Some(target_model.to_string()),
                            requested_model_profile: request.model_profile.clone(),
                            verified_model: observation
                                .snapshot
                                .screen_identity
                                .as_ref()
                                .and_then(|identity| identity.current_model.clone()),
                            verification_source: Some("agy:screen_identity".to_string()),
                        });
                        return true;
                    }
                    result.add_diagnostic(ProviderBoxDiagnostic::warning(
                        DIAG_MODEL_SWITCH_UNVERIFIED,
                        "AGY model picker closed during navigation; reopening before retry",
                        json!({
                            "slot_id": slot_id,
                            "target_model": target_model,
                            "reason": observation.snapshot.reason,
                        }),
                    ));
                    observation = match self.open_model_picker(result, slot_id).await {
                        Some(observation) => observation,
                        None => return false,
                    };
                    continue;
                }
                break;
            } else {
                break;
            }
        }

        let mut all_seen_selected = HashSet::new();
        for (direction, key, label) in [("down", "\x1b[B", "move AGY model picker selection down")]
        {
            let mut phase_seen_selected = HashSet::new();
            for _ in 0..MODEL_PICKER_MAX_DOWN {
                if !is_model_picker(&observation) {
                    observation = match self.open_model_picker(result, slot_id).await {
                        Some(observation) => observation,
                        None => return false,
                    };
                    phase_seen_selected.clear();
                }
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
                if current_model_eq(&observation, target_model) {
                    result.model_switch_result = Some(ModelSwitchResult {
                        status: ModelSwitchStatus::Verified,
                        requested_model: Some(target_model.to_string()),
                        requested_model_profile: request.model_profile.clone(),
                        verified_model: observation
                            .snapshot
                            .screen_identity
                            .as_ref()
                            .and_then(|identity| identity.current_model.clone()),
                        verification_source: Some("agy:screen_identity".to_string()),
                    });
                    return true;
                }
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
                "scan_directions": ["down"],
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
                result.add_diagnostic(ProviderBoxDiagnostic::warning(
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
                return (None, observation);
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
        let mut paste_action = PtyStepAction::text("<pure text prompt paste>");
        paste_action.redacted = true;
        let paste_payload = format!("\x1b[200~{prompt}\x1b[201~");
        let _after_paste = self
            .write_step(
                result,
                slot_id,
                paste_action,
                &paste_payload,
                Some("AGY composer receives the text-only prompt".to_string()),
            )
            .await;
        if result
            .step_records
            .last()
            .is_some_and(|step| step.verification_status == PtyStepVerificationStatus::Failed)
        {
            return false;
        }

        let after_enter = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("AGY submits the text-only prompt".to_string()),
            )
            .await;
        if result
            .step_records
            .last()
            .is_some_and(|step| step.verification_status == PtyStepVerificationStatus::Failed)
        {
            return false;
        }
        if after_enter.snapshot.state == PtyCanonicalState::Running
            || !is_ready_for_text(&after_enter)
        {
            return true;
        }

        true
    }

    async fn monitor_turn(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        cursor: &AgyTranscriptCursor,
        prompt: &str,
        policy: &TimeoutCancelPolicy,
        attempt: u8,
    ) -> AgyMonitorOutcome {
        let timeout_secs = request.timeout_secs.unwrap_or(policy.running_timeout_secs);
        let durable_final_idle_grace_secs = durable_final_idle_grace_secs();
        let started = Instant::now();
        let mut last_screen_hash: Option<String> = None;
        let mut last_progress = Instant::now();
        let mut idle_seen_at: Option<Instant> = None;

        loop {
            match self.extract_turn_from_transcripts(cursor, prompt).await {
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

            if let Some(diagnostic) = text_turn_submission_surface_diagnostic(slot_id, &observation)
            {
                let mut failed = ProviderBoxResult::base(request, ProviderBoxStatus::Failed);
                failed.slot_id = Some(slot_id.to_string());
                failed.step_records = result.step_records.clone();
                failed.add_diagnostic(diagnostic);
                return AgyMonitorOutcome::Failed(failed);
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
                    if seen_at.elapsed() >= Duration::from_secs(durable_final_idle_grace_secs) {
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
                                "extraction": "workspace_cursor",
                                "agy_home": self.agy_home.display().to_string(),
                                "idle_grace_secs": durable_final_idle_grace_secs,
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

    async fn capture_transcript_cursor(
        &self,
        request: &ProviderInteractionRequest,
        slot_id: &str,
    ) -> AgyTranscriptCursor {
        let workspace = text_only_workspace_for_request(request, slot_id);
        let history = HistoryIndex::load(&self.agy_home).await;
        let brain = self.agy_home.join("brain");
        let mut sessions = HashMap::new();
        for (session_id, transcript) in discover_sessions(&brain).await {
            if !session_workspace_matches(&history, &session_id, workspace.as_deref()) {
                continue;
            }
            let Some(session) = parse_session(&transcript).await else {
                continue;
            };
            sessions.insert(
                session_id,
                AgySessionCursor {
                    transcript_path: transcript,
                    step_count: session.steps.len(),
                },
            );
        }
        AgyTranscriptCursor {
            workspace,
            sessions,
        }
    }

    async fn extract_turn_from_transcripts(
        &self,
        cursor: &AgyTranscriptCursor,
        prompt: &str,
    ) -> AgyTranscriptOutcome {
        let history = HistoryIndex::load(&self.agy_home).await;
        let brain = self.agy_home.join("brain");
        let sessions = discover_sessions(&brain).await;
        for (session_id, transcript) in sessions {
            let cursor_for_session = cursor.sessions.get(&session_id);
            let workspace_match =
                session_workspace_matches(&history, &session_id, cursor.workspace.as_deref());
            if !workspace_match && cursor.workspace.is_some() && cursor_for_session.is_none() {
                if !cursor.sessions.is_empty() {
                    continue;
                }
                debug!(
                    session_id,
                    "AGY text-only cursor has no baseline sessions; allowing prompt match fallback before history catches up"
                );
            }
            let Some(session) = parse_session(&transcript).await else {
                continue;
            };
            if session.session_id != session_id {
                debug!(
                    session_id,
                    "AGY parsed session id differs from directory id"
                );
            }
            let start_index = cursor_for_session
                .map(|value| value.step_count)
                .unwrap_or(0);
            let require_prompt_match = cursor_for_session.is_none();
            if let Some(outcome) =
                extract_cursor_turn(&session, start_index, prompt, require_prompt_match)
            {
                return match outcome {
                    AgyTranscriptOutcome::Completed(mut final_turn) => {
                        final_turn.transcript_path = cursor_for_session
                            .map(|value| value.transcript_path.display().to_string())
                            .unwrap_or_else(|| transcript.display().to_string());
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

        let mut current_usage = usage;
        let mut usage_pages = Vec::new();
        let mut seen_pages = HashSet::new();
        let mut last_usage = current_usage.clone();
        for page_index in 0..AGY_USAGE_MAX_PAGES {
            let Some(screen_usage) = usage_screen_value(&current_usage) else {
                break;
            };
            let page_signature = usage_page_signature(&screen_usage);
            if !seen_pages.insert(page_signature) {
                break;
            }
            let has_more_pages = usage_screen_has_more_pages(&screen_usage);
            usage_pages.push(screen_usage);
            last_usage = current_usage.clone();
            if !has_more_pages {
                break;
            }

            let after_scroll = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("pagedown"),
                    "\x1b[6~",
                    Some(format!(
                        "scroll AGY /usage page down to collect remaining model quotas ({}/{})",
                        page_index + 1,
                        AGY_USAGE_MAX_PAGES
                    )),
                )
                .await;
            current_usage = if usage_screen_value(&after_scroll).is_some() {
                after_scroll
            } else {
                self.wait_until(slot_id, Duration::from_secs(3), |obs| {
                    usage_screen_value(obs).is_some()
                })
                .await
            };
        }

        if !usage_pages.is_empty() {
            let quotas = merge_usage_model_quotas(&usage_pages);
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
                account_ref: last_usage
                    .snapshot
                    .screen_identity
                    .as_ref()
                    .and_then(|identity| identity.account.clone()),
                model: last_usage
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
                confidence: last_usage.snapshot.confidence as f32,
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
                    "reason": current_usage.snapshot.reason,
                }),
            ));
        }
    }

    async fn open_mcp_servers_locked(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> Option<AgyObservation> {
        let first = self.observe(slot_id).await;
        if is_mcp_servers_screen(&first) {
            let _ = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("escape"),
                    "\x1b",
                    Some("close current AGY MCP screen before refresh".to_string()),
                )
                .await;
            let _ = self
                .wait_until(slot_id, Duration::from_secs(5), is_ready_for_text)
                .await;
        }

        if self.ensure_composer_ready(result, slot_id).await.is_none() {
            return None;
        }

        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/mcp"),
                "/mcp",
                Some("type AGY /mcp command".to_string()),
            )
            .await;
        let after = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some("execute AGY /mcp command".to_string()),
            )
            .await;
        let observation = if is_mcp_servers_screen(&after) {
            after
        } else {
            self.wait_until(slot_id, Duration::from_secs(6), is_mcp_servers_screen)
                .await
        };

        if is_mcp_servers_screen(&observation) {
            Some(observation)
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE,
                "AGY /mcp screen did not render a recognizable MCP server list",
                json!({
                    "slot_id": slot_id,
                    "reason": observation.snapshot.reason,
                    "state": observation.snapshot.state,
                    "blocked_kind": observation.snapshot.blocked_kind,
                }),
            ));
            None
        }
    }

    async fn refresh_mcp_status_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> Option<AgyObservation> {
        let target = Self::request_mcp_server(request);
        let observation = self.open_mcp_servers_locked(result, slot_id).await?;
        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &observation));
        result.mcp_status = Some(mcp_status_value(request, slot_id, &target, &observation));
        result.status = ProviderBoxStatus::Completed;
        Some(observation)
    }

    async fn reconnect_mcp_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let target = Self::request_mcp_server(request);
        let Some(observation) = self.open_mcp_servers_locked(result, slot_id).await else {
            return;
        };

        match self
            .select_mcp_server_locked(request, result, slot_id, observation, &target)
            .await
        {
            Some(_) => {}
            None => return,
        }

        let actions = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some(format!("open AGY MCP actions for {target}")),
            )
            .await;
        if !mcp_restart_action_visible(&actions) {
            result.status = ProviderBoxStatus::Unverified;
            result.mcp_status = Some(mcp_status_value(request, slot_id, &target, &actions));
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_RECONNECT_UNVERIFIED,
                "AGY MCP actions menu did not expose Restart for the selected server",
                json!({
                    "slot_id": slot_id,
                    "server": target,
                    "reason": actions.snapshot.reason,
                }),
            ));
            return;
        }

        let mut restarted = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::key("enter"),
                "\r",
                Some(format!("restart AGY MCP server {target}")),
            )
            .await;
        if !mcp_server_connected(&restarted, &target) {
            restarted = self
                .wait_until(slot_id, Duration::from_secs(8), |obs| {
                    is_mcp_servers_screen(obs) && mcp_server_connected(obs, &target)
                })
                .await;
        }

        let status = self.pty.get_status(slot_id).await;
        result.slot_status = Some(slot_status_value(slot_id, status.as_ref(), &restarted));
        result.mcp_status = Some(mcp_status_value(request, slot_id, &target, &restarted));
        if mcp_server_connected(&restarted, &target) {
            result.status = ProviderBoxStatus::Completed;
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_RECONNECT_UNVERIFIED,
                "AGY MCP Restart completed without a verified connected status",
                json!({
                    "slot_id": slot_id,
                    "server": target,
                    "mcp_status": result.mcp_status,
                }),
            ));
        }
    }

    async fn select_mcp_server_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
        mut observation: AgyObservation,
        target: &str,
    ) -> Option<AgyObservation> {
        let entries = extract_mcp_server_entries_from_screen(&observation.text);
        let selected_index = entries.iter().position(|entry| entry.selected).unwrap_or(0);
        let Some(target_index) = entries
            .iter()
            .position(|entry| mcp_server_name_eq(&entry.name, target))
        else {
            result.status = ProviderBoxStatus::Failed;
            result.mcp_status = Some(mcp_status_value(request, slot_id, target, &observation));
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_STATUS_UNAVAILABLE,
                "AGY MCP server was not found in the /mcp list",
                json!({
                    "slot_id": slot_id,
                    "server": target,
                    "available_servers": entries.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>(),
                }),
            ));
            return None;
        };

        let steps = if entries.is_empty() {
            0
        } else {
            (target_index + entries.len() - selected_index) % entries.len()
        };
        for _ in 0..steps {
            observation = self
                .write_step(
                    result,
                    slot_id,
                    PtyStepAction::key("down"),
                    "\x1b[B",
                    Some(format!("move AGY MCP selection toward {target}")),
                )
                .await;
        }
        if selected_mcp_server_name(&observation)
            .as_deref()
            .is_some_and(|selected| mcp_server_name_eq(selected, target))
        {
            Some(observation)
        } else {
            result.status = ProviderBoxStatus::Unverified;
            result.mcp_status = Some(mcp_status_value(request, slot_id, target, &observation));
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_MCP_RECONNECT_UNVERIFIED,
                "AGY MCP server selection did not land on the requested server",
                json!({
                    "slot_id": slot_id,
                    "server": target,
                    "selected_server": selected_mcp_server_name(&observation),
                }),
            ));
            None
        }
    }

    async fn clear_screen_locked(&self, result: &mut ProviderBoxResult, slot_id: &str) {
        if self.ensure_composer_ready(result, slot_id).await.is_none() {
            return;
        }

        let _ = self
            .write_step(
                result,
                slot_id,
                PtyStepAction::text("/clear"),
                "/clear",
                Some("type AGY /clear command".to_string()),
            )
            .await;
        let mut observation = self
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

    async fn cleanup_after_pure_text_locked(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) {
        let mut cleanup = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        self.clear_screen_locked(&mut cleanup, slot_id).await;
        if cleanup.status == ProviderBoxStatus::Completed {
            return;
        }
        let observation = self.observe(slot_id).await;
        result.add_diagnostic(ProviderBoxDiagnostic::warning(
            DIAG_PROVIDER_CONTROL_ACTION_UNVERIFIED,
            "AGY text-only turn completed, but post-turn /clear cleanup was not verified",
            json!({
                "slot_id": slot_id,
                "cleanup_status": cleanup.status,
                "cleanup_diagnostics": cleanup.diagnostics,
                "reason": observation.snapshot.reason,
                "blocked_kind": observation.snapshot.blocked_kind,
            }),
        ));
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

    async fn recover_private_text_slot_before_turn(
        &self,
        result: &mut ProviderBoxResult,
        slot_id: &str,
    ) -> bool {
        if !is_private_agy_text_slot_id(slot_id) {
            return false;
        }
        let Some(status) = self.pty.get_status(slot_id).await else {
            return false;
        };
        if status.engine != CliEngine::Agy
            || matches!(status.state, SessionState::Exited | SessionState::Error)
        {
            return false;
        }
        let observation = self.observe(slot_id).await;
        if !should_restart_private_text_slot_before_turn(slot_id, status.state, &observation) {
            return false;
        }

        result.add_diagnostic(ProviderBoxDiagnostic::warning(
            DIAG_PROVIDER_TURN_STALLED,
            "AGY private text-only slot was not ready at turn start; restarting stale replica",
            json!({
                "slot_id": slot_id,
                "session_state": status.state,
                "screen_state": observation.snapshot.state,
                "reason": observation.snapshot.reason,
            }),
        ));
        match self.pty.kill(slot_id).await {
            Ok(()) => true,
            Err(err) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "AGY stale private text-only slot could not be killed before restart",
                    json!({
                        "slot_id": slot_id,
                        "error": err.to_string(),
                    }),
                ));
                false
            }
        }
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
            pty_step: true,
            status: true,
            mcp_status: true,
            mcp_reconnect: true,
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

    async fn mcp_status(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;
        self.refresh_mcp_status_locked(request, &mut result, &slot_id)
            .await;
        result
    }

    async fn mcp_reconnect(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;
        self.reconnect_mcp_locked(request, &mut result, &slot_id)
            .await;
        result
    }

    async fn pty_step(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let step = match Self::request_manual_pty_step(request) {
            Ok(step) => step,
            Err(diagnostic) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(diagnostic);
                return result;
            }
        };

        let slot_id = if Self::request_spawn_if_missing(request) {
            let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
                return result;
            };
            slot_id
        } else {
            let slot_id = Self::request_slot_id(request);
            result.slot_id = Some(slot_id.clone());
            let Some(status) = self.pty.get_status(&slot_id).await else {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Cannot send a PTY step to an unavailable AGY slot",
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
            if matches!(status.state, SessionState::Exited | SessionState::Error) {
                let observation = self.observe(&slot_id).await;
                result.slot_status = Some(slot_status_value(&slot_id, Some(&status), &observation));
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Cannot send a PTY step to an exited or errored AGY slot",
                    json!({
                        "slot_id": slot_id,
                        "session_state": status.state,
                    }),
                ));
                return result;
            }
            slot_id
        };
        result.slot_id = Some(slot_id.clone());

        let (action, bytes) = if step.action_type == "key" {
            let key = step.key.as_deref().unwrap_or_default();
            let Some((canonical, bytes)) = agy_manual_key_bytes(key) else {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "AGY key PTY step uses an unsupported key",
                    json!({
                        "slot_id": slot_id,
                        "key": key,
                        "allowed_keys": AGY_MANUAL_KEY_NAMES,
                    }),
                ));
                return result;
            };
            (PtyStepAction::key(canonical), bytes.to_string())
        } else {
            let text = step.text.as_deref().unwrap_or_default();
            if let Err(diagnostic) = validate_manual_text_step(Some(&slot_id), text) {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(diagnostic);
                return result;
            }
            let mut action = if step.redacted {
                PtyStepAction::text("<manual text>")
            } else {
                PtyStepAction::text(text)
            };
            action.redacted = step.redacted;
            (action, text.to_string())
        };

        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;
        let after = self
            .write_step(
                &mut result,
                &slot_id,
                action,
                &bytes,
                step.expected_change.clone(),
            )
            .await;
        let status = self.pty.get_status(&slot_id).await;
        result.slot_status = Some(slot_status_value(&slot_id, status.as_ref(), &after));
        let failed = result
            .step_records
            .last()
            .is_some_and(|step| step.verification_status == PtyStepVerificationStatus::Failed);
        result.status = if failed {
            ProviderBoxStatus::Failed
        } else {
            ProviderBoxStatus::Completed
        };
        result
    }

    async fn switch_model(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        self.reclaim_idle_text_slots().await;
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
            self.mark_text_slot_used(&slot_id).await;
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
        self.reclaim_idle_text_slots().await;
        let _text_turn_guard = self.text_turn_global_lock.lock().await;
        let requested_slot_id = Self::request_slot_id(request);
        let mut cold_start_slot = match self.pty.get_status(&requested_slot_id).await {
            Some(status) => matches!(status.state, SessionState::Exited | SessionState::Error),
            None => true,
        };
        if self
            .recover_private_text_slot_before_turn(&mut result, &requested_slot_id)
            .await
        {
            cold_start_slot = true;
        }
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        self.mark_text_slot_used(&slot_id).await;
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;

        if let Some(target_model) = request.model.as_deref() {
            if !target_model.trim().is_empty() {
                let model_policy = request.model_switch_policy.clone().unwrap_or_default();
                let model_ready = if should_switch_model_for_pure_text_slot(
                    &slot_id,
                    cold_start_slot,
                    model_policy.allow_respawn,
                ) {
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
            let cursor = self.capture_transcript_cursor(request, &slot_id).await;
            if !self
                .submit_prompt_step(&mut result, &slot_id, &prompt)
                .await
            {
                result.status = ProviderBoxStatus::Failed;
                return result;
            }
            match self
                .monitor_turn(
                    request,
                    &mut result,
                    &slot_id,
                    &cursor,
                    &prompt,
                    &policy,
                    attempt,
                )
                .await
            {
                AgyMonitorOutcome::Completed(final_turn) => {
                    result.status = ProviderBoxStatus::Completed;
                    result.provider_conversation_id = Some(final_turn.session_id);
                    result.durable_source = Some(final_turn.transcript_path);
                    result.final_text = Some(final_turn.final_text);
                    self.cleanup_after_pure_text_locked(request, &mut result, &slot_id)
                        .await;
                    self.mark_text_slot_used(&slot_id).await;
                    result.status = ProviderBoxStatus::Completed;
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
                    if is_retryable_text_turn_failure(&failed) && attempt + 1 < max_attempts {
                        result.diagnostics.extend(failed.diagnostics.clone());
                        result.add_diagnostic(ProviderBoxDiagnostic::warning(
                            DIAG_PROVIDER_TURN_STALLED,
                            "AGY text-only turn failed without a durable final; clearing slot and retrying once",
                            json!({
                                "slot_id": slot_id,
                                "attempt": attempt,
                                "retry_attempt": attempt + 1,
                            }),
                        ));
                        self.cleanup_after_pure_text_locked(request, &mut result, &slot_id)
                            .await;
                        result.status = ProviderBoxStatus::Unknown;
                        if self
                            .ensure_composer_ready(&mut result, &slot_id)
                            .await
                            .is_none()
                        {
                            return result;
                        }
                        continue;
                    }
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
            ProviderControlAction::ClearInput => {
                let after = self
                    .write_step(
                        &mut result,
                        &slot_id,
                        PtyStepAction::key("ctrl+c"),
                        "\x03",
                        Some("clear AGY composer input with Ctrl-C".to_string()),
                    )
                    .await;
                let failed = result.step_records.last().is_some_and(|step| {
                    step.verification_status == PtyStepVerificationStatus::Failed
                });
                result.status = if failed {
                    ProviderBoxStatus::Failed
                } else {
                    ProviderBoxStatus::Completed
                };
                let status = self.pty.get_status(&slot_id).await;
                result.slot_status = Some(slot_status_value(&slot_id, status.as_ref(), &after));
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

fn is_starting_or_unknown(observation: &AgyObservation) -> bool {
    observation.snapshot.state == PtyCanonicalState::Unknown
        || observation
            .snapshot
            .reason
            .starts_with("session_state:Starting")
}

fn is_agy_startup_signing_in(observation: &AgyObservation) -> bool {
    observation.snapshot.reason == "agy:startup_signing_in" || {
        let lower = observation.text.to_ascii_lowercase();
        lower.contains("welcome to the")
            && lower.contains("antigravity cli")
            && lower.contains("not signed in")
            && lower.contains("signing in")
    }
}

fn is_workspace_trust_prompt(observation: &AgyObservation) -> bool {
    observation.snapshot.reason == "agy:workspace_trust_prompt"
        || observation.snapshot.blocked_kind.as_deref() == Some("workspace_trust")
        || {
            let lower = observation.text.to_ascii_lowercase();
            lower.contains("accessing workspace")
                && lower.contains("do you trust the contents of this project")
                && lower.contains("yes, i trust this folder")
                && lower.contains("no, exit")
        }
}

fn should_resolve_agy_startup_surface(state: SessionState, observation: &AgyObservation) -> bool {
    matches!(state, SessionState::Starting)
        || is_starting_or_unknown(observation)
        || is_agy_startup_signing_in(observation)
        || is_workspace_trust_prompt(observation)
}

fn selected_workspace_trust_option(observation: &AgyObservation) -> AgyTrustSelection {
    for line in observation.text.lines() {
        let trimmed = line
            .trim_start()
            .trim_start_matches(|c: char| matches!(c, '│' | '┃' | '║' | '┆' | '┊'))
            .trim_start();
        let selected = trimmed.starts_with('>')
            || trimmed.starts_with('›')
            || trimmed.starts_with('❯')
            || trimmed.starts_with('▸')
            || trimmed.starts_with('▶')
            || trimmed.starts_with('➜')
            || trimmed.starts_with('→');
        if !selected {
            continue;
        }
        let lower = clean_agy_line(trimmed).to_ascii_lowercase();
        if lower.starts_with("yes, i trust this folder") {
            return AgyTrustSelection::Trust;
        }
        if lower.starts_with("no, exit") {
            return AgyTrustSelection::Exit;
        }
    }
    AgyTrustSelection::Unknown
}

fn is_usage_screen(observation: &AgyObservation) -> bool {
    observation.snapshot.reason == "agy:usage_meter"
        || observation.snapshot.screen_usage.is_some()
        || observation
            .text
            .to_ascii_lowercase()
            .contains("model quota")
}

fn is_mcp_servers_screen(observation: &AgyObservation) -> bool {
    observation.snapshot.reason == "agy:mcp_servers"
        || observation.snapshot.blocked_kind.as_deref() == Some("mcp_servers")
        || {
            let lower = observation.text.to_ascii_lowercase();
            lower.contains("mcp servers")
                && lower.contains("keyboard:")
                && (lower.contains("enter actions") || lower.contains("enter select"))
                && lower.contains("esc to cancel")
        }
}

fn is_overlay_screen(observation: &AgyObservation) -> bool {
    is_model_picker(observation)
        || is_usage_screen(observation)
        || is_mcp_servers_screen(observation)
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
        && !is_mcp_servers_screen(observation)
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

fn text_turn_submission_surface_diagnostic(
    slot_id: &str,
    observation: &AgyObservation,
) -> Option<ProviderBoxDiagnostic> {
    if !is_slash_command_surface(observation) {
        return None;
    }
    Some(ProviderBoxDiagnostic::warning(
        DIAG_PROVIDER_TURN_STALLED,
        "AGY entered a slash command surface during text-only prompt submission",
        json!({
            "slot_id": slot_id,
            "screen_state": observation.snapshot.state,
            "reason": observation.snapshot.reason,
            "blocked_kind": observation.snapshot.blocked_kind,
            "retry_policy": "clear_and_retry_text_only_turn",
        }),
    ))
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

fn missiond_runtime_dir() -> PathBuf {
    std::env::var("MISSIOND_RUNTIME_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".missiond")
                .join("runtime")
                .join("missiond")
        })
}

fn agy_text_only_workspace(slot_id: &str) -> PathBuf {
    missiond_runtime_dir()
        .join("provider-box")
        .join("agy-text-only")
        .join(slot_id)
        .join("workspace")
}

fn prepare_text_only_workspace(cwd: &Path, slot_id: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(cwd)?;
    let policy_dir = cwd.join(".missiond-provider-box");
    std::fs::create_dir_all(&policy_dir)?;
    let policy = json!({
        "schema": "missiond.agy-text-only-workspace-policy.v1",
        "slot_id": slot_id,
        "mode": "text_only_llm_export",
        "agy_cli_observation": {
            "version_family": "1.0.x",
            "disable_all_tools_flag": false,
            "help_flags_used": ["--sandbox"],
            "help_flags_not_found": ["--no-tools", "--disable-tools"],
            "settings_panel_disable_all_tools": false
        },
        "desired_permissions": {
            "toolPermission": "strict",
            "allowNonWorkspaceAccess": false,
            "enableTerminalSandbox": true,
            "permissions": {
                "deny": [
                    "read_file(*)",
                    "write_file(*)",
                    "command(*)",
                    "unsandboxed(*)",
                    "read_url(*)",
                    "execute_url(*)",
                    "mcp(*)"
                ]
            }
        },
        "enforced_by_provider_box": [
            "isolated runtime cwd per private replica slot",
            "agy --sandbox when AGY accepts the toggle",
            "sidecar correlation id outside prompt text",
            "pre-submit durable transcript cursor",
            "post-submit durable transcript violation gate"
        ],
        "note": "AGY 1.0.x documents global settings permissions but does not expose a stable per-process no-tools/profile flag in agy --help. Provider-box therefore treats this file as local policy evidence and fails closed from durable transcripts if a tool/file/shell/MCP step appears."
    });
    let policy_text = serde_json::to_string_pretty(&policy)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    std::fs::write(
        policy_dir.join("agy-text-only-policy.json"),
        format!("{policy_text}\n"),
    )?;
    std::fs::write(
        cwd.join("README.missiond-text-only.txt"),
        "MissionD provider-box AGY text-only workspace.\nThis directory is isolated from project code and is used only for router-facing plain text turns.\n",
    )?;
    Ok(())
}

fn text_only_workspace_for_request(
    request: &ProviderInteractionRequest,
    slot_id: &str,
) -> Option<PathBuf> {
    request.cwd.as_ref().map(PathBuf::from).or_else(|| {
        if request.command == BoxCommand::PureTextSingleTurn && is_private_agy_text_slot_id(slot_id)
        {
            Some(agy_text_only_workspace(slot_id))
        } else {
            request.project_root.as_ref().map(PathBuf::from)
        }
    })
}

fn session_workspace_matches(
    history: &HistoryIndex,
    session_id: &str,
    workspace: Option<&Path>,
) -> bool {
    let Some(workspace) = workspace else {
        return true;
    };
    history
        .resolve_cwd(session_id)
        .as_deref()
        .is_some_and(|cwd| paths_match(cwd, workspace))
}

fn paths_match(observed: &str, expected: &Path) -> bool {
    let observed = PathBuf::from(observed);
    if observed == expected {
        return true;
    }
    let observed_canonical = observed.canonicalize().ok();
    let expected_canonical = expected.canonicalize().ok();
    match (observed_canonical, expected_canonical) {
        (Some(observed), Some(expected)) => observed == expected,
        _ => false,
    }
}

fn agy_slot_pool_id(model: &str) -> String {
    format!("slot-pool-agy-{}", slug_model(model))
}

fn is_agy_text_model_exportable(model: &str) -> bool {
    !normalize_model(model).starts_with("gpt-oss ")
}

fn is_private_agy_text_slot_id(slot_id: &str) -> bool {
    if !slot_id.starts_with("slot-agy-") {
        return false;
    }
    let Some((_, replica)) = slot_id.rsplit_once('-') else {
        return false;
    };
    matches!(replica, "a" | "b")
}

fn should_switch_model_for_pure_text_slot(
    slot_id: &str,
    cold_start_slot: bool,
    allow_model_switch: bool,
) -> bool {
    allow_model_switch || (cold_start_slot && is_private_agy_text_slot_id(slot_id))
}

fn should_restart_private_text_slot_before_turn(
    slot_id: &str,
    session_state: SessionState,
    observation: &AgyObservation,
) -> bool {
    is_private_agy_text_slot_id(slot_id)
        && !matches!(session_state, SessionState::Exited | SessionState::Error)
        && !is_ready_for_text(observation)
}

fn idle_agy_text_slot_reclaim_candidates(
    last_used: &HashMap<String, Instant>,
    now: Instant,
) -> Vec<String> {
    last_used
        .iter()
        .filter_map(|(slot_id, seen_at)| {
            if !is_private_agy_text_slot_id(slot_id) {
                return None;
            }
            if now.duration_since(*seen_at) >= Duration::from_secs(AGY_TEXT_SLOT_IDLE_RECLAIM_SECS)
            {
                Some(slot_id.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
}

async fn reclaim_idle_agy_text_slots(
    pty: &Arc<PTYManager>,
    slot_last_used: &Arc<Mutex<HashMap<String, Instant>>>,
) {
    let now = Instant::now();
    let candidates = {
        let last_used = slot_last_used.lock().await;
        idle_agy_text_slot_reclaim_candidates(&last_used, now)
    };

    for slot_id in candidates {
        let Some(status) = pty.get_status(&slot_id).await else {
            slot_last_used.lock().await.remove(&slot_id);
            continue;
        };
        if !matches!(
            status.state,
            SessionState::Idle
                | SessionState::SlashMenu
                | SessionState::Exited
                | SessionState::Error
        ) {
            continue;
        }
        if let Err(err) = pty.kill(&slot_id).await {
            warn!(
                slot_id = %slot_id,
                error = %err,
                "Failed to reclaim idle AGY text-only slot"
            );
            continue;
        }
        slot_last_used.lock().await.remove(&slot_id);
        debug!(
            slot_id = %slot_id,
            idle_secs = AGY_TEXT_SLOT_IDLE_RECLAIM_SECS,
            "Reclaimed idle AGY text-only slot"
        );
    }
}

fn start_agy_text_slot_reaper(
    pty: Arc<PTYManager>,
    slot_last_used: Arc<Mutex<HashMap<String, Instant>>>,
) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(AGY_TEXT_SLOT_REAPER_INTERVAL_SECS));
        loop {
            interval.tick().await;
            reclaim_idle_agy_text_slots(&pty, &slot_last_used).await;
        }
    });
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

    let expected_models = if target_index > selected_index {
        entries[selected_index + 1..=target_index]
            .iter()
            .map(|entry| entry.model.clone())
            .collect()
    } else {
        entries[selected_index + 1..]
            .iter()
            .chain(entries[..=target_index].iter())
            .map(|entry| entry.model.clone())
            .collect()
    };

    Some(ModelPickerNavigationPlan {
        direction: "down",
        key: "\x1b[B",
        expected_models,
    })
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

fn extract_mcp_server_entries_from_screen(text: &str) -> Vec<AgyMcpServerEntry> {
    let mut entries = Vec::new();
    let mut in_latest_mcp_page = false;
    for line in text.lines() {
        let cleaned = clean_agy_line(line);
        let lower = cleaned.to_ascii_lowercase();
        if lower.starts_with("mcp servers") {
            entries.clear();
            in_latest_mcp_page = true;
            continue;
        }
        if !in_latest_mcp_page {
            continue;
        }
        if lower.starts_with("keyboard:") || lower.contains("[restart]") || lower == "disable" {
            continue;
        }
        let selected = picker_line_selected(line);
        let (connected, rest) = if let Some(rest) = cleaned.strip_prefix('✓') {
            (true, rest.trim())
        } else if let Some(rest) = cleaned.strip_prefix('✗') {
            (false, rest.trim())
        } else {
            continue;
        };
        let rest_lower = rest.to_ascii_lowercase();
        let (name, tools_summary, error) = if let Some(index) = rest_lower.find("tools:") {
            (
                rest[..index].trim().to_string(),
                Some(rest[index + "tools:".len()..].trim().to_string()),
                None,
            )
        } else if let Some(index) = rest_lower.find("error:") {
            (
                rest[..index].trim().to_string(),
                None,
                Some(rest[index + "error:".len()..].trim().to_string()),
            )
        } else {
            (rest.trim().to_string(), None, None)
        };
        if name.is_empty() {
            continue;
        }
        entries.push(AgyMcpServerEntry {
            name,
            selected,
            connected,
            status: if connected { "connected" } else { "failed" }.to_string(),
            tools_summary: tools_summary.filter(|value| !value.is_empty()),
            error: error.filter(|value| !value.is_empty()),
        });
    }
    entries
}

fn mcp_status_value(
    request: &ProviderInteractionRequest,
    slot_id: &str,
    target: &str,
    observation: &AgyObservation,
) -> Value {
    let servers = extract_mcp_server_entries_from_screen(&observation.text);
    let selected_server = servers
        .iter()
        .find(|entry| entry.selected)
        .map(|entry| entry.name.clone());
    let target_entry = servers
        .iter()
        .find(|entry| mcp_server_name_eq(&entry.name, target));
    let aggregate_status = if servers.is_empty() {
        "unknown"
    } else if servers.iter().all(|entry| entry.connected) {
        "connected"
    } else if target_entry.as_ref().is_some_and(|entry| !entry.connected) {
        "target_failed"
    } else {
        "degraded"
    };
    json!({
        "schema": "missiond.provider-box.agy-mcp-status.v1",
        "provider": request.provider.clone().or_else(|| Some("agy_cli".to_string())),
        "engine": CliEngine::Agy,
        "slot_id": slot_id,
        "source": "agy:/mcp",
        "observed_at": chrono::Utc::now().to_rfc3339(),
        "status": aggregate_status,
        "target_server": target,
        "target_status": target_entry.map(|entry| entry.status.clone()),
        "selected_server": selected_server,
        "restart_action_visible": mcp_restart_action_visible(observation),
        "servers": servers
            .iter()
            .map(|entry| {
                json!({
                    "name": entry.name,
                    "selected": entry.selected,
                    "status": entry.status,
                    "connected": entry.connected,
                    "tools_summary": entry.tools_summary,
                    "error": entry.error,
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn selected_mcp_server_name(observation: &AgyObservation) -> Option<String> {
    extract_mcp_server_entries_from_screen(&observation.text)
        .into_iter()
        .find(|entry| entry.selected)
        .map(|entry| entry.name)
}

fn mcp_server_connected(observation: &AgyObservation, target: &str) -> bool {
    extract_mcp_server_entries_from_screen(&observation.text)
        .into_iter()
        .any(|entry| mcp_server_name_eq(&entry.name, target) && entry.connected)
}

fn mcp_restart_action_visible(observation: &AgyObservation) -> bool {
    observation
        .text
        .lines()
        .map(clean_agy_line)
        .any(|line| line.to_ascii_lowercase().contains("[restart]"))
}

fn mcp_server_name_eq(observed: &str, target: &str) -> bool {
    observed.trim().eq_ignore_ascii_case(target.trim())
}

fn usage_screen_value(observation: &AgyObservation) -> Option<Value> {
    observation
        .snapshot
        .screen_usage
        .as_ref()
        .and_then(|usage| serde_json::to_value(usage).ok())
}

fn merge_usage_model_quotas(usage_pages: &[Value]) -> Vec<ProviderModelUsage> {
    let mut seen = HashSet::new();
    let mut quotas = Vec::new();
    for page in usage_pages {
        for quota in usage_model_quotas(page) {
            if seen.insert(quota.model.clone()) {
                quotas.push(quota);
            }
        }
    }
    quotas
}

fn usage_model_quotas(screen_usage: &Value) -> Vec<ProviderModelUsage> {
    screen_usage
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
        .collect()
}

fn usage_screen_has_more_pages(screen_usage: &Value) -> bool {
    usage_visible_range(screen_usage).is_some_and(|(_, end, total)| end < total)
}

fn usage_visible_range(screen_usage: &Value) -> Option<(u64, u64, u64)> {
    let range = screen_usage.get("visibleRange")?;
    Some((
        range.get("start")?.as_u64()?,
        range.get("end")?.as_u64()?,
        range.get("total")?.as_u64()?,
    ))
}

fn usage_page_signature(screen_usage: &Value) -> String {
    let range = usage_visible_range(screen_usage)
        .map(|(start, end, total)| format!("{start}-{end}/{total}"))
        .unwrap_or_else(|| "unknown-range".to_string());
    let models = usage_model_quotas(screen_usage)
        .into_iter()
        .map(|quota| quota.model)
        .collect::<Vec<_>>()
        .join("|");
    format!("{range}:{models}")
}

fn agy_manual_key_bytes(key: &str) -> Option<(&'static str, &'static str)> {
    let normalized = key
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], "")
        .replace(['+', '/'], "")
        .replace("control", "ctrl")
        .replace(' ', "");
    match normalized.as_str() {
        "enter" | "return" => Some(("enter", "\r")),
        "esc" | "escape" => Some(("escape", "\x1b")),
        "up" | "arrowup" => Some(("up", "\x1b[A")),
        "down" | "arrowdown" => Some(("down", "\x1b[B")),
        "right" | "arrowright" => Some(("right", "\x1b[C")),
        "left" | "arrowleft" => Some(("left", "\x1b[D")),
        "tab" => Some(("tab", "\t")),
        "backspace" => Some(("backspace", "\x7f")),
        "delete" | "del" => Some(("delete", "\x1b[3~")),
        "ctrlc" => Some(("ctrl+c", "\x03")),
        "ctrld" => Some(("ctrl+d", AGY_CTRL_D)),
        "pageup" => Some(("pageup", "\x1b[5~")),
        "pagedown" => Some(("pagedown", "\x1b[6~")),
        "home" => Some(("home", "\x1b[H")),
        "end" => Some(("end", "\x1b[F")),
        _ => None,
    }
}

fn validate_manual_text_step(
    slot_id: Option<&str>,
    text: &str,
) -> Result<(), ProviderBoxDiagnostic> {
    if text.chars().count() > AGY_MANUAL_TEXT_LIMIT {
        return Err(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_INVALID_REQUEST,
            "AGY text PTY step exceeds the maximum length",
            json!({
                "slot_id": slot_id,
                "max_chars": AGY_MANUAL_TEXT_LIMIT,
            }),
        ));
    }
    if text.contains('\n') || text.contains('\r') {
        return Err(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_INVALID_REQUEST,
            "AGY text PTY step must not include Enter; send text and Enter as separate steps",
            json!({
                "slot_id": slot_id,
                "rule": "text_and_enter_are_separate_observe_act_observe_steps",
            }),
        ));
    }
    Ok(())
}

fn is_retryable_text_turn_failure(result: &ProviderBoxResult) -> bool {
    matches!(result.status, ProviderBoxStatus::Failed)
        && result.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_str(),
                DIAG_PROVIDER_DURABLE_FINAL_MISSING
                    | DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED
                    | DIAG_PROVIDER_TURN_STALLED
            )
        })
}

fn durable_final_idle_grace_secs() -> u64 {
    std::env::var("MISSIOND_AGY_DURABLE_FINAL_IDLE_GRACE_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(AGY_DURABLE_FINAL_IDLE_GRACE_DEFAULT_SECS)
        .clamp(
            AGY_DURABLE_FINAL_IDLE_GRACE_MIN_SECS,
            AGY_DURABLE_FINAL_IDLE_GRACE_MAX_SECS,
        )
}

fn extract_cursor_turn(
    session: &AgySession,
    start_index: usize,
    prompt: &str,
    require_prompt_match: bool,
) -> Option<AgyTranscriptOutcome> {
    let start = session
        .steps
        .iter()
        .enumerate()
        .skip(start_index)
        .find_map(|(index, step)| {
            (step.step_type == "USER_INPUT"
                && (!require_prompt_match || step_contains_prompt(step, prompt)))
            .then_some(index)
        })?;

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
        "PLANNER_RESPONSE" | "CONVERSATION_HISTORY" | "EPHEMERAL_MESSAGE"
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

fn step_contains_prompt(step: &AgyStep, prompt: &str) -> bool {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return false;
    }
    let content = step.content.as_ref().map(value_text).unwrap_or_default();
    let content_trimmed = content.trim();
    if content_trimmed.contains(prompt) {
        return true;
    }
    let content_normalized = normalize_prompt_match_text(content_trimmed);
    let prompt_normalized = normalize_prompt_match_text(prompt);
    content_normalized.contains(&prompt_normalized)
        || prompt_anchor_match(&content_normalized, &prompt_normalized)
}

fn normalize_prompt_match_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn prompt_anchor_match(content: &str, prompt: &str) -> bool {
    if prompt.chars().count() < 512 {
        return false;
    }
    let mut anchors = Vec::new();
    for anchor in [
        prompt_anchor_prefix(prompt, 384),
        prompt_anchor_middle(prompt, 384),
        prompt_anchor_suffix(prompt, 384),
    ] {
        if anchor.chars().count() >= 96 && !anchors.iter().any(|existing| existing == &anchor) {
            anchors.push(anchor);
        }
    }
    anchors
        .iter()
        .filter(|anchor| content.contains(anchor.as_str()))
        .count()
        >= 2
}

fn prompt_anchor_prefix(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn prompt_anchor_middle(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return chars.into_iter().collect();
    }
    let start = chars.len().saturating_sub(max_chars) / 2;
    chars[start..start + max_chars].iter().collect()
}

fn prompt_anchor_suffix(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars().rev().take(max_chars).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
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
        if !is_agy_text_model_exportable(&entry.display_name) {
            continue;
        }
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

    use crate::provider_box::types::BoxCommand;

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
    fn text_turn_slash_surface_is_retryable_stall() {
        let obs = observation(&[
            "────────────────────────────────────────",
            "> /clear",
            "────────────────────────────────────────",
            "? for shortcuts                                                                                  Claude Opus 4.6 (Thinking)",
        ]);
        let diagnostic =
            text_turn_submission_surface_diagnostic("slot-agy-claude-opus-46-thinking-a", &obs)
                .expect("diagnostic");
        assert_eq!(diagnostic.code, DIAG_PROVIDER_TURN_STALLED);

        let request = ProviderInteractionRequest::pure_text(CliEngine::Agy, "hello".to_string());
        let mut result = ProviderBoxResult::base(&request, ProviderBoxStatus::Failed);
        result.add_diagnostic(diagnostic);
        assert!(is_retryable_text_turn_failure(&result));
    }

    #[test]
    fn agy_driver_parses_mcp_server_status_page() {
        let obs = observation(&[
            "────────────────────────────────────────",
            "MCP Servers",
            "Plugins (~/.gemini/antigravity-cli/plugins)",
            ">  ✓ missiond  Tools: mission_board_query, mission_board_create, mission_board_update, mission_board_delete,",
            "               mission_board_claim, +93 more",
            "   ✗ missiond-reconnect-teach  error: missiond reconnect teaching failure : calling \"initialize\": EOF",
            "Keyboard: ↑/↓ Navigate  enter Actions  esc to cancel                                      Gemini 3.5 Flash (Medium)",
        ]);

        assert!(is_mcp_servers_screen(&obs));
        assert!(is_overlay_screen(&obs));
        assert!(!is_ready_for_text(&obs));
        assert!(!is_hard_blocked(&obs));

        let servers = extract_mcp_server_entries_from_screen(&obs.text);
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "missiond");
        assert!(servers[0].selected);
        assert!(servers[0].connected);
        assert_eq!(servers[1].name, "missiond-reconnect-teach");
        assert!(!servers[1].connected);
        assert_eq!(
            servers[1].error.as_deref(),
            Some("missiond reconnect teaching failure : calling \"initialize\": EOF")
        );

        let mut request = ProviderInteractionRequest::new(BoxCommand::McpStatus, CliEngine::Agy);
        request.provider = Some("agy_cli".to_string());
        let value = mcp_status_value(&request, "slot-agy-research", "missiond", &obs);
        assert_eq!(value["status"], "degraded");
        assert_eq!(value["target_status"], "connected");
        assert_eq!(value["selected_server"], "missiond");
    }

    #[test]
    fn agy_driver_detects_mcp_restart_action_menu() {
        let obs = observation(&[
            "────────────────────────────────────────",
            "MCP Servers",
            "Plugins (~/.gemini/antigravity-cli/plugins)",
            "   ✓ missiond  Tools: mission_board_query, mission_board_create",
            ">  ✗ missiond-reconnect-teach  error: missiond reconnect teaching failure : calling \"initialize\": EOF",
            "   [Restart]   Disable",
            "Keyboard: ←/→ Navigate  enter Select  esc to cancel                                      Gemini 3.5 Flash (Medium)",
        ]);

        assert!(is_mcp_servers_screen(&obs));
        assert!(mcp_restart_action_visible(&obs));
        assert_eq!(
            selected_mcp_server_name(&obs).as_deref(),
            Some("missiond-reconnect-teach")
        );
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
    fn agy_driver_recognizes_startup_signing_in_as_not_ready() {
        let obs = observation(&[
            "Welcome to the Antigravity CLI. You are currently not signed in.",
            "⣷  Signing in...",
        ]);

        assert!(is_agy_startup_signing_in(&obs));
        assert!(!is_ready_for_text(&obs));
        assert!(should_resolve_agy_startup_surface(
            SessionState::Starting,
            &obs
        ));
    }

    #[test]
    fn agy_driver_workspace_trust_requires_selected_yes_before_enter() {
        let yes = observation(&[
            "Accessing workspace:",
            "/Users/jinchen",
            "Do you trust the contents of this project?",
            "Antigravity CLI requires permission to read, edit, and execute files here.",
            "> Yes, I trust this folder",
            "  No, exit",
            "↑/↓ Navigate · enter Confirm",
            "Claude Opus 4.6 (Thinking)",
        ]);
        let no = observation(&[
            "Accessing workspace:",
            "/Users/jinchen",
            "Do you trust the contents of this project?",
            "Antigravity CLI requires permission to read, edit, and execute files here.",
            "  Yes, I trust this folder",
            "> No, exit",
            "↑/↓ Navigate · enter Confirm",
            "Claude Opus 4.6 (Thinking)",
        ]);

        assert!(is_workspace_trust_prompt(&yes));
        assert_eq!(
            selected_workspace_trust_option(&yes),
            AgyTrustSelection::Trust
        );
        assert_eq!(
            selected_workspace_trust_option(&no),
            AgyTrustSelection::Exit
        );
        assert!(!is_ready_for_text(&yes));
    }

    #[test]
    fn agy_driver_prefers_exit_slash_command_bytes() {
        assert_eq!(AGY_EXIT_COMMAND, "/exit");
    }

    #[test]
    fn agy_manual_key_bytes_are_allowlisted() {
        assert_eq!(agy_manual_key_bytes("enter"), Some(("enter", "\r")));
        assert_eq!(agy_manual_key_bytes("esc"), Some(("escape", "\x1b")));
        assert_eq!(agy_manual_key_bytes("down"), Some(("down", "\x1b[B")));
        assert_eq!(agy_manual_key_bytes("ctrl+d"), Some(("ctrl+d", AGY_CTRL_D)));
        assert!(agy_manual_key_bytes("f13").is_none());
    }

    #[test]
    fn agy_manual_text_step_rejects_embedded_enter() {
        let err = validate_manual_text_step(Some("slot-agy-test"), "/exit\r")
            .expect_err("enter must be separate");

        assert_eq!(err.code, DIAG_PROVIDER_BOX_INVALID_REQUEST);
        assert!(err.message.contains("separate steps"));
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
    fn agy_driver_treats_login_prompt_ctrl_d_confirmation_as_exit_fallback_state() {
        let obs = observation(&[
            " Welcome to the Antigravity CLI. You are currently not signed in.",
            "",
            " Select login method:",
            " > 1. Google OAuth",
            "   2. Use a Google Cloud project",
            "",
            " [Use arrow keys to navigate, Enter to select]",
            "press ctrl+d again to exit",
        ]);

        assert!(is_exit_confirm_pending(&obs));
        assert!(!is_overlay_screen(&obs));
        assert!(!is_ready_for_text(&obs));
        assert!(is_hard_blocked(&obs));
    }

    #[test]
    fn agy_driver_treats_oauth_authorization_prompt_as_hard_auth_block() {
        let obs = observation(&[
            " Open this link in the browser (be sure to copy-paste the whole URL):",
            " https://accounts.google.com/o/oauth2/auth?access_type=offline&client_id=redacted",
            " If you aren't automatically redirected, paste the authorization code below:",
            " authorization code...",
        ]);

        assert_eq!(obs.snapshot.reason, "agy:oauth_authorization_prompt");
        assert_eq!(
            obs.snapshot.blocked_kind.as_deref(),
            Some("auth_code_required")
        );
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
    fn cursor_turn_extracts_final_after_matching_user() {
        let session = AgySession {
            session_id: "conv-1".to_string(),
            steps: vec![
                step(1, "USER_INPUT", Some(json!("previous turn"))),
                step(2, "PLANNER_RESPONSE", Some(json!("previous final"))),
                step(3, "USER_INPUT", Some(json!("hello from router"))),
                step(4, "PLANNER_RESPONSE", Some(json!("final answer"))),
            ],
        };

        let outcome = extract_cursor_turn(&session, 2, "hello from router", true).expect("matched");

        match outcome {
            AgyTranscriptOutcome::Completed(turn) => {
                assert_eq!(turn.session_id, "conv-1");
                assert_eq!(turn.final_text, "final answer");
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    #[test]
    fn cursor_turn_rejects_tool_call_before_final() {
        let mut tool_step = step(2, "PLANNER_RESPONSE", None);
        tool_step.tool_calls = Some(vec![AgyToolCall {
            name: "run_command".to_string(),
            args: json!({"cmd": "pwd"}),
        }]);
        let session = AgySession {
            session_id: "conv-1".to_string(),
            steps: vec![step(1, "USER_INPUT", Some(json!("read a file"))), tool_step],
        };

        let outcome = extract_cursor_turn(&session, 0, "read a file", true).expect("matched");

        assert!(matches!(outcome, AgyTranscriptOutcome::Violation { .. }));
    }

    #[test]
    fn cursor_turn_ignores_ephemeral_messages_before_final() {
        let session = AgySession {
            session_id: "conv-1".to_string(),
            steps: vec![
                step(1, "USER_INPUT", Some(json!("short prompt"))),
                step(2, "EPHEMERAL_MESSAGE", Some(json!("Working..."))),
                step(3, "PLANNER_RESPONSE", Some(json!("plain final"))),
            ],
        };

        let outcome = extract_cursor_turn(&session, 0, "short prompt", true).expect("matched");

        match outcome {
            AgyTranscriptOutcome::Completed(turn) => {
                assert_eq!(turn.final_text, "plain final");
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    #[test]
    fn cursor_turn_matches_multiline_prompt_without_prompt_injected_correlation() {
        let session = AgySession {
            session_id: "conv-1".to_string(),
            steps: vec![
                step(1, "USER_INPUT", Some(json!("line one\nline two"))),
                step(2, "PLANNER_RESPONSE", Some(json!("plain final"))),
            ],
        };

        let outcome =
            extract_cursor_turn(&session, 0, "line one\nline two", true).expect("matched");

        assert!(matches!(outcome, AgyTranscriptOutcome::Completed(_)));
    }

    #[test]
    fn cursor_turn_accepts_first_user_after_baseline_without_prompt_match() {
        let session = AgySession {
            session_id: "conv-1".to_string(),
            steps: vec![
                step(1, "USER_INPUT", Some(json!("previous turn"))),
                step(2, "PLANNER_RESPONSE", Some(json!("previous final"))),
                step(
                    3,
                    "USER_INPUT",
                    Some(json!("truncated transcript user payload")),
                ),
                step(4, "PLANNER_RESPONSE", Some(json!("final answer"))),
            ],
        };

        let outcome =
            extract_cursor_turn(&session, 2, "long prompt that is not fully logged", false)
                .expect("matched first new user turn");

        match outcome {
            AgyTranscriptOutcome::Completed(turn) => {
                assert_eq!(turn.final_text, "final answer");
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    #[test]
    fn prompt_match_accepts_large_prompt_by_stable_anchors() {
        let prompt = format!(
            "{}\n{}\n{}",
            "alpha ".repeat(120),
            "middle unique source catalog batch 7 ".repeat(30),
            "omega unique citation bounds ".repeat(80)
        );
        let logged = format!("<USER_REQUEST>\n{prompt}\n</USER_REQUEST>");
        let session = AgySession {
            session_id: "conv-1".to_string(),
            steps: vec![
                step(1, "USER_INPUT", Some(json!(logged))),
                step(2, "PLANNER_RESPONSE", Some(json!("final answer"))),
            ],
        };

        let outcome = extract_cursor_turn(&session, 0, &prompt, true).expect("matched by anchors");

        assert!(matches!(outcome, AgyTranscriptOutcome::Completed(_)));
    }

    #[test]
    fn text_turn_failure_without_durable_final_is_retryable() {
        let request = ProviderInteractionRequest::pure_text(CliEngine::Agy, "hello".to_string());
        let mut result = ProviderBoxResult::base(&request, ProviderBoxStatus::Failed);
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_DURABLE_FINAL_MISSING,
            "missing final",
            json!({}),
        ));

        assert!(is_retryable_text_turn_failure(&result));
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
    fn model_picker_navigation_plan_wraps_down_when_target_is_above_selected() {
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

        assert_eq!(plan.direction, "down");
        assert_eq!(
            plan.expected_models,
            vec![
                "Gemini 3.1 Pro (High)".to_string(),
                "Gemini 3.5 Flash (Medium)".to_string(),
                "Gemini 3.5 Flash (High)".to_string(),
            ]
        );
    }

    #[test]
    fn model_picker_navigation_plan_uses_down_to_avoid_current_row_up_bug() {
        let entries = extract_model_picker_entries_from_screen(
            "Switch Model\n\
               Gemini 3.5 Flash (Medium)\n\
               Gemini 3.5 Flash (High)\n\
             > Gemini 3.5 Flash (Low)       (current)\n\
               Gemini 3.1 Pro (Low)\n\
               Gemini 3.1 Pro (High)\n\
               Claude Sonnet 4.6 (Thinking)\n\
               Claude Opus 4.6 (Thinking)\n\
               GPT-OSS 120B (Medium)\n\
             Keyboard: up/down",
        );

        let plan = plan_model_picker_navigation_from_entries(
            &entries,
            Some("Gemini 3.5 Flash (Low)"),
            "Gemini 3.5 Flash (Medium)",
        )
        .expect("navigation plan");

        assert_eq!(plan.direction, "down");
        assert_eq!(
            plan.expected_models,
            vec![
                "Gemini 3.1 Pro (Low)".to_string(),
                "Gemini 3.1 Pro (High)".to_string(),
                "Claude Sonnet 4.6 (Thinking)".to_string(),
                "Claude Opus 4.6 (Thinking)".to_string(),
                "GPT-OSS 120B (Medium)".to_string(),
                "Gemini 3.5 Flash (Medium)".to_string(),
            ]
        );
    }

    #[test]
    fn usage_quota_merge_preserves_model_order_and_deduplicates_overlap() {
        let pages = vec![
            json!({
                "modelQuotas": [
                    {"model": "Gemini 3.5 Flash (Medium)", "percent": 100, "status": "Quota available"},
                    {"model": "Gemini 3.5 Flash (High)", "percent": 100, "status": "Quota available"}
                ],
                "visibleRange": {"start": 1, "end": 20, "total": 33}
            }),
            json!({
                "modelQuotas": [
                    {"model": "Gemini 3.5 Flash (High)", "percent": 100, "status": "Quota available"},
                    {"model": "Claude Opus 4.6 (Thinking)", "percent": 100, "status": "Quota available"}
                ],
                "visibleRange": {"start": 14, "end": 33, "total": 33}
            }),
        ];

        let quotas = merge_usage_model_quotas(&pages);

        assert_eq!(quotas.len(), 3);
        assert_eq!(quotas[0].model, "Gemini 3.5 Flash (Medium)");
        assert_eq!(quotas[1].model, "Gemini 3.5 Flash (High)");
        assert_eq!(quotas[2].model, "Claude Opus 4.6 (Thinking)");
        assert!(usage_screen_has_more_pages(&pages[0]));
        assert!(!usage_screen_has_more_pages(&pages[1]));
    }

    #[test]
    fn gpt_oss_is_not_exported_as_agy_text_model() {
        assert!(is_agy_text_model_exportable("Claude Opus 4.6 (Thinking)"));
        assert!(!is_agy_text_model_exportable("GPT-OSS 120B (Medium)"));
        assert!(is_private_agy_text_slot_id(
            "slot-agy-claude-opus-46-thinking-a"
        ));
        assert!(!is_private_agy_text_slot_id("slot-agy-usage-probe"));
    }

    #[test]
    fn idle_reclaim_candidates_only_include_old_private_text_slots() {
        let now = Instant::now();
        let mut last_used = HashMap::new();
        last_used.insert(
            "slot-agy-claude-opus-46-thinking-a".to_string(),
            now - Duration::from_secs(AGY_TEXT_SLOT_IDLE_RECLAIM_SECS + 1),
        );
        last_used.insert(
            "slot-agy-claude-opus-46-thinking-b".to_string(),
            now - Duration::from_secs(AGY_TEXT_SLOT_IDLE_RECLAIM_SECS - 1),
        );
        last_used.insert(
            "slot-agy-usage-probe".to_string(),
            now - Duration::from_secs(AGY_TEXT_SLOT_IDLE_RECLAIM_SECS + 1),
        );
        last_used.insert(
            "slot-codex-worker-a".to_string(),
            now - Duration::from_secs(AGY_TEXT_SLOT_IDLE_RECLAIM_SECS + 1),
        );

        let mut candidates = idle_agy_text_slot_reclaim_candidates(&last_used, now);
        candidates.sort();
        assert_eq!(
            candidates,
            vec!["slot-agy-claude-opus-46-thinking-a".to_string()]
        );
    }

    #[test]
    fn pure_text_model_switch_is_allowed_only_for_cold_private_or_explicit_policy() {
        assert!(should_switch_model_for_pure_text_slot(
            "slot-agy-gemini-35-flash-medium-a",
            true,
            false
        ));
        assert!(!should_switch_model_for_pure_text_slot(
            "slot-agy-gemini-35-flash-medium-a",
            false,
            false
        ));
        assert!(!should_switch_model_for_pure_text_slot(
            "slot-agy-usage-probe",
            true,
            false
        ));
        assert!(should_switch_model_for_pure_text_slot(
            "slot-agy-usage-probe",
            false,
            true
        ));
    }

    #[test]
    fn private_text_slot_restart_detects_stale_thinking_replica() {
        let stale = observation(&["⣻", "for 2s", "? for shortcuts"]);
        assert!(should_restart_private_text_slot_before_turn(
            "slot-agy-claude-opus-46-thinking-b",
            SessionState::Thinking,
            &stale
        ));
        assert!(should_restart_private_text_slot_before_turn(
            "slot-agy-claude-opus-46-thinking-b",
            SessionState::Idle,
            &observation(&[
                "────────────────────────────────────────",
                "> /c",
                "────────────────────────────────────────",
                "/changelog        Show release notes and changes",
                "> /clear          Clear conversation and start a new one",
                "/config           Open settings panel",
                "↑/↓ Navigate · enter Select · tab Complete",
                "esc to cancel                                                                                    Gemini 3.5 Flash (High)",
            ])
        ));

        let ready = observation(&[
            "      ▄▀▀▄        Antigravity CLI 1.0.3",
            "     ▀▀▀▀▀▀       jjrrqqq@gmail.com (Google AI Ultra)",
            "    ▀▀▀▀▀▀▀▀      Claude Opus 4.6 (Thinking)",
            "   ▄▀▀    ▀▀▄     ~/Projects/missiond",
            ">",
            "? for shortcuts                                                                               Claude Opus 4.6 (Thinking)",
        ]);
        assert!(!should_restart_private_text_slot_before_turn(
            "slot-agy-claude-opus-46-thinking-b",
            SessionState::Starting,
            &ready
        ));
        assert!(!should_restart_private_text_slot_before_turn(
            "slot-agy-usage-probe",
            SessionState::Thinking,
            &stale
        ));
    }

    #[test]
    fn request_force_restart_reads_status_spawn_aliases() {
        let mut request = ProviderInteractionRequest::new(BoxCommand::Status, CliEngine::Agy);
        request.desired_worker = Some(json!({"force_restart": true}));
        assert!(AgyProviderDriver::request_force_restart(&request));

        request.desired_worker = Some(json!({"respawn": true}));
        assert!(AgyProviderDriver::request_force_restart(&request));

        request.desired_worker = Some(json!({"spawn_if_missing": true}));
        assert!(!AgyProviderDriver::request_force_restart(&request));
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

    #[test]
    fn router_export_omits_gpt_oss_text_source() {
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
                provider_model_id: "agy:gpt-oss-120b-medium".to_string(),
                display_name: "GPT-OSS 120B (Medium)".to_string(),
                family: Some("GPT-OSS".to_string()),
                routeable_default: true,
                switch_capability: "interactive_model_picker".to_string(),
                usage_probe_capability: "interactive_usage_screen".to_string(),
                confidence: 0.9,
            }],
            diagnostics: Vec::new(),
        };

        let export = build_router_export(&request, &catalog);

        assert!(export.routeable_entries.is_empty());
        assert!(export.blocked_entries.is_empty());
    }
}
