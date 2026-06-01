use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use missiond_core::db::traits::MissionStore;
use missiond_core::pty::{recognize_screen, PtyCanonicalState};
use missiond_core::types::{CliEngine, SharedProjectRegistry};
use missiond_core::{LearnedPermissions, PTYManager, PTYSlot, PTYSpawnOptions, SessionState};
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock};

use super::driver::{ProviderDriver, ProviderDriverCapabilities};
use super::types::{
    ProviderBoxDiagnostic, ProviderBoxResult, ProviderBoxStatus, ProviderInteractionRequest,
    PtyObservation, PtyStepAction, PtyStepRecord, PtyStepVerificationStatus,
    DIAG_PROVIDER_BOX_INVALID_REQUEST, DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
    DIAG_PROVIDER_DURABLE_FINAL_MISSING, DIAG_PROVIDER_TEXT_ONLY_VIOLATION,
    DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED, DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED,
};

const DEFAULT_CODEX_SLOT: &str = "slot-codex-provider-box";
const OBSERVE_SETTLE_MS: u64 = 250;
const CODEX_EXEC_TEXT_PROVIDER: &str = "codex_exec_text";

#[derive(Clone)]
pub(crate) struct CodexProviderDriver {
    pty: Arc<PTYManager>,
    store: Arc<dyn MissionStore>,
    pty_session_uuids: Arc<RwLock<HashSet<String>>>,
    project_registry: SharedProjectRegistry,
    learned: Option<Arc<LearnedPermissions>>,
    slot_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    codex_home: PathBuf,
}

#[derive(Debug, Clone)]
struct CodexObservation {
    lines: Vec<String>,
    text: String,
    snapshot: missiond_core::pty::PtyRecognitionSnapshot,
}

#[derive(Debug, Clone)]
struct CodexTurnFinal {
    session_id: String,
    rollout_path: String,
    final_text: String,
}

#[derive(Debug, Clone)]
struct CodexExecJsonlAnalysis {
    event_count: usize,
    final_text: Option<String>,
    violation: Option<Value>,
}

impl CodexProviderDriver {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        pty: Arc<PTYManager>,
        store: Arc<dyn MissionStore>,
        pty_session_uuids: Arc<RwLock<HashSet<String>>>,
        project_registry: SharedProjectRegistry,
        learned: Option<Arc<LearnedPermissions>>,
    ) -> Self {
        Self {
            pty,
            store,
            pty_session_uuids,
            project_registry,
            learned,
            slot_locks: Arc::new(Mutex::new(HashMap::new())),
            codex_home: default_codex_home(),
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
            .unwrap_or_else(|| DEFAULT_CODEX_SLOT.to_string())
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

    fn request_dangerous_bypass(request: &ProviderInteractionRequest) -> bool {
        request.dangerously_bypass_approvals_and_sandbox
            || bool_any(
                request.tool_policy.as_ref(),
                &[
                    "dangerously_bypass_approvals_and_sandbox",
                    "dangerously_skip_permissions",
                    "dangerously_bypass",
                    "bypass_approvals_and_sandbox",
                    "bypass_mode",
                    "bypass",
                ],
            )
            || bool_any(
                request.desired_worker.as_ref(),
                &[
                    "dangerously_bypass_approvals_and_sandbox",
                    "dangerously_skip_permissions",
                    "dangerously_bypass",
                    "bypass_approvals_and_sandbox",
                    "bypass_mode",
                    "bypass",
                ],
            )
    }

    fn request_search_enabled(request: &ProviderInteractionRequest) -> bool {
        request
            .tool_policy
            .as_ref()
            .and_then(|policy| policy.get("search_enabled").and_then(Value::as_bool))
            .unwrap_or(true)
    }

    fn request_sandbox(request: &ProviderInteractionRequest) -> Option<String> {
        request
            .tool_policy
            .as_ref()
            .and_then(|policy| policy.get("sandbox").and_then(Value::as_str))
            .map(str::to_string)
            .or_else(|| Some("read-only".to_string()))
    }

    fn request_approval_policy(request: &ProviderInteractionRequest) -> Option<String> {
        Some(
            request
                .tool_policy
                .as_ref()
                .and_then(|policy| policy.get("approval_policy").and_then(Value::as_str))
                .unwrap_or("never")
                .to_string(),
        )
    }

    async fn existing_slot_matches_request(
        &self,
        slot_id: &str,
        request: &ProviderInteractionRequest,
    ) -> bool {
        let Some(options) = self.pty.get_spawn_options(slot_id).await else {
            return true;
        };
        if let Some(model) = request.model.as_deref() {
            if options.model.as_deref() != Some(model) {
                return false;
            }
            if options.reasoning_effort.as_deref() != request.model_profile.as_deref() {
                return false;
            }
        } else if let Some(profile) = request.model_profile.as_deref() {
            if options.reasoning_effort.as_deref() != Some(profile) {
                return false;
            }
        }
        if request_mentions_bypass_policy(request)
            && options.dangerously_skip_permissions != Self::request_dangerous_bypass(request)
        {
            return false;
        }
        true
    }

    async fn ensure_codex_binary(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> bool {
        self.locate_codex_binary(request, result).await.is_some()
    }

    async fn locate_codex_binary(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<PathBuf> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let output = Command::new(shell)
            .arg("-l")
            .arg("-i")
            .arg("-c")
            .arg("command -v codex")
            .output()
            .await;
        match output {
            Ok(output) if output.status.success() => {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if path.is_empty() {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                        "required CLI codex resolved to an empty path",
                        json!({
                            "engine": request.engine.to_string(),
                            "rule": "fast-fail; no fallback provider path is allowed"
                        }),
                    ));
                    None
                } else {
                    Some(PathBuf::from(path))
                }
            }
            Ok(output) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "required CLI codex is not available in the MissionD worker login shell",
                    json!({
                        "engine": request.engine.to_string(),
                        "slot_id": request.slot_id,
                        "stdout": String::from_utf8_lossy(&output.stdout).trim(),
                        "stderr": String::from_utf8_lossy(&output.stderr).trim(),
                        "rule": "fast-fail; no fallback provider path is allowed"
                    }),
                ));
                None
            }
            Err(err) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "could not preflight required CLI codex",
                    json!({
                        "engine": request.engine.to_string(),
                        "slot_id": request.slot_id,
                        "error": err.to_string(),
                        "rule": "fast-fail; no fallback provider path is allowed"
                    }),
                ));
                None
            }
        }
    }

    async fn ensure_slot(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<String> {
        let slot_id = Self::request_slot_id(request);
        if Self::request_force_restart(request) {
            let _ = self.pty.kill(&slot_id).await;
        }
        if let Some(status) = self.pty.get_status(&slot_id).await {
            if status.engine != CliEngine::Codex {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Requested provider-box slot is not a Codex slot",
                    json!({
                        "slot_id": slot_id,
                        "engine": status.engine.to_string(),
                    }),
                ));
                return None;
            }
            if !matches!(status.state, SessionState::Exited | SessionState::Error) {
                if self.existing_slot_matches_request(&slot_id, request).await {
                    return Some(slot_id);
                }
                let _ = self.pty.kill(&slot_id).await;
            }
        }

        if !self.ensure_codex_binary(request, result).await {
            return None;
        }

        let cwd = request
            .cwd
            .as_ref()
            .or(request.project_root.as_ref())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
        let slot = PTYSlot {
            id: slot_id.clone(),
            role: "provider-box-codex".to_string(),
            cwd: Some(cwd),
            engine: CliEngine::Codex,
        };
        self.pty.init_slot(&slot).await;

        let dangerous_bypass = Self::request_dangerous_bypass(request);
        let options = PTYSpawnOptions {
            auto_restart: true,
            wait_for_idle: true,
            timeout_secs: Some(90),
            mcp_config: None,
            dangerously_skip_permissions: dangerous_bypass,
            model: request.model.clone(),
            reasoning_effort: request.model_profile.clone(),
            search_enabled: Self::request_search_enabled(request),
            sandbox: if dangerous_bypass {
                None
            } else {
                Self::request_sandbox(request)
            },
            approval_policy: if dangerous_bypass {
                None
            } else {
                Self::request_approval_policy(request)
            },
            tool_policy_path: None,
            extra_env: HashMap::new(),
            initial_prompt: None,
            command_override: None,
        };
        match crate::slot_orchestrator::spawner::spawn_tracked_slot(
            &self.pty,
            &self.store,
            &self.pty_session_uuids,
            &self.project_registry,
            self.learned.as_ref(),
            &slot,
            options,
            None,
        )
        .await
        {
            Ok(_) => Some(slot_id),
            Err(err) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex PTY slot could not be spawned by provider-box",
                    json!({
                        "slot_id": slot_id,
                        "error": err.to_string(),
                        "rule": "fast-fail; no direct GenericCli fallback is allowed"
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
    ) -> CodexObservation {
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

    async fn observe(&self, slot_id: &str) -> CodexObservation {
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
        let snapshot = recognize_screen(CliEngine::Codex, &lines, state);
        let text = lines.join("\n");
        CodexObservation {
            lines,
            text,
            snapshot,
        }
    }

    fn pty_observation(slot_id: &str, observation: &CodexObservation) -> PtyObservation {
        PtyObservation::structured(
            format!("pty:{slot_id}"),
            observation.text.clone(),
            serde_json::to_value(&observation.snapshot).unwrap_or_else(|_| json!({})),
        )
    }

    async fn ensure_ready_for_prompt(&self, result: &mut ProviderBoxResult, slot_id: &str) -> bool {
        let started = Instant::now();
        loop {
            let observation = self.observe(slot_id).await;
            if matches!(
                observation.snapshot.state,
                PtyCanonicalState::Idle | PtyCanonicalState::Complete
            ) {
                return true;
            }
            if started.elapsed() > Duration::from_secs(8) {
                result.status = ProviderBoxStatus::Blocked;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex slot is not ready for provider-box prompt submission",
                    json!({
                        "slot_id": slot_id,
                        "state": observation.snapshot.state,
                        "reason": observation.snapshot.reason,
                        "blocked_kind": observation.snapshot.blocked_kind,
                    }),
                ));
                return false;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
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
        let mut action = PtyStepAction::text("<codex prompt paste + enter>");
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
            Some("Codex accepts prompt and starts a provider turn".to_string()),
            status,
        );
        if let Err(err) = send_result {
            step.diagnostics.push(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "Codex provider-box prompt submission failed",
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
    ) -> ProviderBoxResult {
        let timeout_secs = request.timeout_secs.unwrap_or(180).clamp(10, 7_200);
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut idle_seen_at: Option<Instant> = None;

        loop {
            if let Some(final_turn) = self
                .extract_turn_from_rollouts(&request.correlation_id)
                .await
            {
                result.status = ProviderBoxStatus::Completed;
                result.provider_conversation_id = Some(final_turn.session_id);
                result.durable_source = Some(final_turn.rollout_path);
                result.final_text = Some(final_turn.final_text);
                return result.clone();
            }

            let observation = self.observe(slot_id).await;
            if observation.snapshot.state == PtyCanonicalState::Blocked {
                let mut failed = ProviderBoxResult::base(request, ProviderBoxStatus::Blocked);
                failed.slot_id = Some(slot_id.to_string());
                failed.step_records = result.step_records.clone();
                failed.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex provider-box turn entered a blocked provider surface",
                    json!({
                        "slot_id": slot_id,
                        "reason": observation.snapshot.reason,
                        "blocked_kind": observation.snapshot.blocked_kind,
                    }),
                ));
                return failed;
            }

            if matches!(
                observation.snapshot.state,
                PtyCanonicalState::Idle | PtyCanonicalState::Complete
            ) {
                if let Some(seen_at) = idle_seen_at {
                    if seen_at.elapsed() >= Duration::from_secs(3) {
                        let mut failed =
                            ProviderBoxResult::base(request, ProviderBoxStatus::Failed);
                        failed.slot_id = Some(slot_id.to_string());
                        failed.step_records = result.step_records.clone();
                        failed.add_diagnostic(ProviderBoxDiagnostic::error(
                            DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                            "Codex returned to input but no matching durable rollout final was found",
                            json!({
                                "slot_id": slot_id,
                                "correlation_id": request.correlation_id,
                                "codex_home": self.codex_home.display().to_string(),
                                "rule": "PTY screen text is diagnostic only; no fallback final was synthesized"
                            }),
                        ));
                        return failed;
                    }
                } else {
                    idle_seen_at = Some(Instant::now());
                }
            } else {
                idle_seen_at = None;
            }

            if Instant::now() >= deadline {
                let _ = self.pty.write(slot_id, "\x1b").await;
                tokio::time::sleep(Duration::from_secs(1)).await;
                let mut failed = ProviderBoxResult::base(request, ProviderBoxStatus::Failed);
                failed.slot_id = Some(slot_id.to_string());
                failed.step_records = result.step_records.clone();
                failed.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED,
                    "Codex provider-box turn timed out before a matching durable final appeared",
                    json!({
                        "slot_id": slot_id,
                        "correlation_id": request.correlation_id,
                        "timeout_secs": timeout_secs,
                        "cancel": DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED,
                    }),
                ));
                return failed;
            }

            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    }

    fn validate_prompt_turn(
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> bool {
        let prompt_ok = request
            .prompt
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
        let attachments_ok = request.attachments.is_empty();
        if prompt_ok && attachments_ok {
            return true;
        }
        result.status = ProviderBoxStatus::Failed;
        result.add_diagnostic(ProviderBoxDiagnostic::error(
            DIAG_PROVIDER_BOX_INVALID_REQUEST,
            "Codex provider-box turn requires a non-empty prompt and currently supports no attachments",
            json!({
                "prompt_present": prompt_ok,
                "attachments": request.attachments.len(),
            }),
        ));
        false
    }

    fn validate_pure_text_exec_request(
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<(String, Option<String>, String)> {
        if !Self::validate_prompt_turn(request, result) {
            return None;
        }
        if request.dangerously_bypass_approvals_and_sandbox {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex exec text-only source does not allow bypassing approvals and sandbox",
                json!({
                    "provider": CODEX_EXEC_TEXT_PROVIDER,
                    "rule": "pure text lanes run in an isolated cwd with read-only sandbox and fail closed on tool events"
                }),
            ));
            return None;
        }
        if !(request.no_tools && request.no_mcp && request.no_shell && request.no_file_access) {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex exec text-only source requires no_tools/no_mcp/no_shell/no_file_access guards",
                json!({
                    "no_tools": request.no_tools,
                    "no_mcp": request.no_mcp,
                    "no_shell": request.no_shell,
                    "no_file_access": request.no_file_access,
                }),
            ));
            return None;
        }

        let Some(model) = request
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
        else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_INVALID_REQUEST,
                "Codex exec text-only source requires an explicit model",
                json!({
                    "examples": [
                        {"model": "gpt-5.5", "model_profile": "xhigh"},
                        {"model": "gpt-5.5"}
                    ]
                }),
            ));
            return None;
        };

        let reasoning = match normalize_codex_reasoning_effort(request.model_profile.as_deref()) {
            Ok(value) => value,
            Err(value) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_INVALID_REQUEST,
                    "Codex exec text-only source received an unsupported reasoning effort",
                    json!({
                        "model_profile": value,
                        "allowed": ["minimal", "low", "medium", "high", "xhigh", "default"]
                    }),
                ));
                return None;
            }
        };

        let prompt = request.prompt.clone().unwrap_or_default();
        Some((model, reasoning, prompt))
    }

    async fn run_codex_exec_text(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
        model: &str,
        reasoning: Option<&str>,
        prompt: &str,
    ) -> bool {
        let Some(codex_binary) = self.locate_codex_binary(request, result).await else {
            return false;
        };
        let workspace = codex_exec_workspace(&request.correlation_id);
        if let Err(err) = tokio::fs::create_dir_all(&workspace).await {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "Codex exec text-only workspace could not be created",
                json!({
                    "workspace": workspace.display().to_string(),
                    "error": err.to_string(),
                }),
            ));
            return false;
        }
        let output_file = workspace.join("last-message.txt");
        let args = codex_exec_text_args(&output_file, model, reasoning);
        let timeout_secs = request.timeout_secs.unwrap_or(180).clamp(10, 7_200);

        let mut command = Command::new(&codex_binary);
        command
            .args(&args)
            .arg(prompt)
            .current_dir(&workspace)
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex exec text-only process could not be started",
                    json!({
                        "codex_binary": codex_binary.display().to_string(),
                        "error": err.to_string(),
                    }),
                ));
                return false;
            }
        };

        let stdout_task = child.stdout.take().map(|mut stdout| {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let _ = stdout.read_to_end(&mut buf).await;
                buf
            })
        });
        let stderr_task = child.stderr.take().map(|mut stderr| {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let _ = stderr.read_to_end(&mut buf).await;
                buf
            })
        });

        let status =
            match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait()).await {
                Ok(Ok(status)) => status,
                Ok(Err(err)) => {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                        "Codex exec text-only process wait failed",
                        json!({
                            "error": err.to_string(),
                        }),
                    ));
                    return false;
                }
                Err(_) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED,
                        "Codex exec text-only process timed out and was killed",
                        json!({
                            "timeout_secs": timeout_secs,
                            "cancel": DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED,
                        }),
                    ));
                    return false;
                }
            };

        let stdout = join_output_task(stdout_task).await;
        let stderr = join_output_task(stderr_task).await;
        let stdout_text = String::from_utf8_lossy(&stdout).to_string();
        let stderr_text = String::from_utf8_lossy(&stderr).to_string();
        let analysis = analyze_codex_exec_jsonl(&stdout_text);

        if let Some(violation) = analysis.violation {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_TEXT_ONLY_VIOLATION,
                "Codex exec text-only turn attempted a tool, shell, MCP, or function call",
                json!({
                    "provider": CODEX_EXEC_TEXT_PROVIDER,
                    "event": violation,
                    "stdout_event_count": analysis.event_count,
                    "rule": "pure text source results are not returned after provider tool activity"
                }),
            ));
            return false;
        }

        if !status.success() {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "Codex exec text-only process exited unsuccessfully",
                json!({
                    "exit_status": status.code(),
                    "stdout_excerpt": stdout_text.chars().take(1200).collect::<String>(),
                    "stderr_excerpt": stderr_text.chars().take(1200).collect::<String>(),
                    "stdout_event_count": analysis.event_count,
                }),
            ));
            return false;
        }

        let final_text = tokio::fs::read_to_string(&output_file)
            .await
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| analysis.final_text.map(|value| value.trim().to_string()))
            .filter(|value| !value.is_empty());

        let Some(final_text) = final_text else {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                "Codex exec completed without a usable output-last-message final",
                json!({
                    "output_last_message": output_file.display().to_string(),
                    "stdout_event_count": analysis.event_count,
                }),
            ));
            return false;
        };

        result.status = ProviderBoxStatus::Completed;
        result.provider = Some(CODEX_EXEC_TEXT_PROVIDER.to_string());
        result.provider_conversation_id = Some(request.correlation_id.clone());
        result.durable_source = Some(output_file.display().to_string());
        result.slot_status = Some(json!({
            "kind": "codex_exec_text",
            "workspace": workspace.display().to_string(),
            "model": model,
            "reasoning_effort": reasoning,
            "stdout_event_count": analysis.event_count,
            "exit_status": status.code(),
        }));
        result.final_text = Some(final_text);
        true
    }

    async fn extract_turn_from_rollouts(&self, correlation_id: &str) -> Option<CodexTurnFinal> {
        let roots = [
            self.codex_home.join("sessions"),
            self.codex_home.join("archived_sessions"),
        ];
        let correlation_id = correlation_id.to_string();
        tokio::task::spawn_blocking(move || {
            let mut files = Vec::new();
            for root in roots {
                collect_jsonl_files(&root, &mut files);
            }
            files.sort_by(|a, b| modified_at(b).cmp(&modified_at(a)));
            files.truncate(80);
            files
                .iter()
                .filter_map(|path| extract_correlated_rollout(path, &correlation_id))
                .max_by_key(|turn| modified_at(Path::new(&turn.rollout_path)))
        })
        .await
        .ok()
        .flatten()
    }
}

#[async_trait]
impl ProviderDriver for CodexProviderDriver {
    fn engine(&self) -> CliEngine {
        CliEngine::Codex
    }

    fn capabilities(&self) -> ProviderDriverCapabilities {
        ProviderDriverCapabilities {
            submit_turn: true,
            switch_model: false,
            usage_probe: false,
            model_catalog: false,
            pure_text_guard: true,
            control_action: false,
            pty_step: false,
            status: true,
            mcp_status: false,
            mcp_reconnect: false,
        }
    }

    async fn status(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        let slot_id = Self::request_slot_id(request);
        result.slot_id = Some(slot_id.clone());

        if Self::request_spawn_if_missing(request) || Self::request_force_restart(request) {
            let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
                return result;
            };
            result.slot_id = Some(slot_id.clone());
        } else {
            let Some(status) = self.pty.get_status(&slot_id).await else {
                result.status = ProviderBoxStatus::Unknown;
                result.add_diagnostic(ProviderBoxDiagnostic::warning(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex slot status is unavailable",
                    json!({
                        "slot_id": slot_id,
                        "spawn_if_missing": false,
                    }),
                ));
                return result;
            };
            if status.engine != CliEngine::Codex {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Requested slot is not a Codex slot",
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
            Some("observe current Codex CLI state".to_string()),
        )
        .await;
        result.status = ProviderBoxStatus::Completed;
        result
    }

    async fn submit_turn(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        if !Self::validate_prompt_turn(request, &mut result) {
            return result;
        }
        let Some(slot_id) = self.ensure_slot(request, &mut result).await else {
            return result;
        };
        result.slot_id = Some(slot_id.clone());
        let lock = self.slot_lock(&slot_id).await;
        let _guard = lock.lock().await;

        if !self.ensure_ready_for_prompt(&mut result, &slot_id).await {
            return result;
        }
        let prompt = correlate_prompt(request);
        if !self
            .submit_prompt_step(&mut result, &slot_id, prompt.as_str())
            .await
        {
            result.status = ProviderBoxStatus::Failed;
            return result;
        }
        self.monitor_turn(request, &mut result, &slot_id).await
    }

    async fn pure_text_single_turn(
        &self,
        request: &ProviderInteractionRequest,
    ) -> ProviderBoxResult {
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        result.provider = Some(CODEX_EXEC_TEXT_PROVIDER.to_string());
        result.slot_id = None;
        let Some((model, reasoning, prompt)) =
            Self::validate_pure_text_exec_request(request, &mut result)
        else {
            return result;
        };

        self.run_codex_exec_text(request, &mut result, &model, reasoning.as_deref(), &prompt)
            .await;
        result
    }
}

fn correlate_prompt(request: &ProviderInteractionRequest) -> String {
    let prompt = request.prompt.clone().unwrap_or_default();
    if prompt.contains(&request.correlation_id) {
        return prompt;
    }
    format!(
        "MissionD provider-box correlation_id: {}\n\
         MissionD provider-box turn_id_hint: {}\n\n{}",
        request.correlation_id,
        request.task_id.as_deref().unwrap_or("none"),
        prompt
    )
}

fn bool_any(value: Option<&Value>, keys: &[&str]) -> bool {
    let Some(value) = value else {
        return false;
    };
    keys.iter()
        .any(|key| value.get(*key).and_then(Value::as_bool).unwrap_or(false))
}

fn request_mentions_bypass_policy(request: &ProviderInteractionRequest) -> bool {
    const KEYS: &[&str] = &[
        "dangerously_bypass_approvals_and_sandbox",
        "dangerously_skip_permissions",
        "dangerously_bypass",
        "bypass_approvals_and_sandbox",
        "bypass_mode",
        "bypass",
    ];
    request.dangerously_bypass_approvals_and_sandbox
        || [
            request.tool_policy.as_ref(),
            request.desired_worker.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| KEYS.iter().any(|key| value.get(*key).is_some()))
}

fn slot_status_value(
    slot_id: &str,
    status: Option<&missiond_core::PTYAgentInfo>,
    observation: &CodexObservation,
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

fn default_codex_home() -> PathBuf {
    std::env::var("CODEX_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("MISSIOND_CODEX_HOME").ok().map(PathBuf::from))
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".codex"))
        })
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn codex_exec_text_args(output_file: &Path, model: &str, reasoning: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "exec".to_string(),
        "--json".to_string(),
        "--output-last-message".to_string(),
        output_file.display().to_string(),
        "--color".to_string(),
        "never".to_string(),
        "--ephemeral".to_string(),
        "--ignore-user-config".to_string(),
        "--ignore-rules".to_string(),
        "--skip-git-repo-check".to_string(),
        "--sandbox".to_string(),
        "read-only".to_string(),
        "--model".to_string(),
        model.to_string(),
        "-c".to_string(),
        "approval_policy=\"never\"".to_string(),
        "-c".to_string(),
        "features.shell_tool=false".to_string(),
        "-c".to_string(),
        "features.web_search=false".to_string(),
        "-c".to_string(),
        "tools.web_search=false".to_string(),
        "-c".to_string(),
        "apps._default.enabled=false".to_string(),
        "-c".to_string(),
        "apps._default.default_tools_enabled=false".to_string(),
        "-c".to_string(),
        "tools.view_image=false".to_string(),
    ];
    if let Some(reasoning) = reasoning {
        args.push("-c".to_string());
        args.push(format!("model_reasoning_effort=\"{reasoning}\""));
    }
    args
}

fn normalize_codex_reasoning_effort(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let normalized = value.to_ascii_lowercase();
    if normalized == "default" {
        return Ok(None);
    }
    match normalized.as_str() {
        "minimal" | "low" | "medium" | "high" | "xhigh" => Ok(Some(normalized)),
        _ => Err(value.to_string()),
    }
}

fn codex_exec_workspace(correlation_id: &str) -> PathBuf {
    let root = std::env::var("MISSIOND_PROVIDER_BOX_CODEX_EXEC_ROOT")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("MISSIOND_RUNTIME_DIR")
                .ok()
                .map(|root| PathBuf::from(root).join("provider-box/codex-exec-text"))
        })
        .or_else(|| {
            std::env::var("HOME").ok().map(|home| {
                PathBuf::from(home).join(".missiond/runtime/missiond/provider-box/codex-exec-text")
            })
        })
        .unwrap_or_else(|| PathBuf::from(".missiond/runtime/provider-box/codex-exec-text"));
    root.join(sanitize_path_segment(correlation_id))
}

fn sanitize_path_segment(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.is_empty() {
        format!("corr-{}", uuid::Uuid::new_v4().simple())
    } else {
        sanitized
    }
}

async fn join_output_task(task: Option<tokio::task::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    match task {
        Some(task) => task.await.unwrap_or_default(),
        None => Vec::new(),
    }
}

fn analyze_codex_exec_jsonl(stdout: &str) -> CodexExecJsonlAnalysis {
    let mut analysis = CodexExecJsonlAnalysis {
        event_count: 0,
        final_text: None,
        violation: None,
    };
    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        analysis.event_count += 1;
        if analysis.violation.is_none() && codex_exec_event_is_tool_violation(&event) {
            analysis.violation = Some(event.clone());
        }
        if let Some(text) = codex_assistant_text(&event) {
            if !text.trim().is_empty() {
                analysis.final_text = Some(text);
            }
        }
    }
    analysis
}

fn codex_exec_event_is_tool_violation(event: &Value) -> bool {
    let type_fields = [
        event.get("type").and_then(Value::as_str),
        event.pointer("/payload/type").and_then(Value::as_str),
        event.pointer("/item/type").and_then(Value::as_str),
    ];
    if type_fields.into_iter().flatten().any(|value| {
        matches!(
            value,
            "function_call"
                | "function_call_output"
                | "tool_call"
                | "tool_result"
                | "mcp_tool_call"
                | "approval_request"
        )
    }) {
        return true;
    }
    let name = event
        .pointer("/payload/name")
        .or_else(|| event.pointer("/item/name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    matches!(
        name,
        "exec_command"
            | "shell_command"
            | "apply_patch"
            | "read_file"
            | "write_file"
            | "view_image"
            | "mcp"
    )
}

fn collect_jsonl_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, out);
        } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn modified_at(path: &Path) -> std::time::SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
}

fn extract_correlated_rollout(path: &Path, correlation_id: &str) -> Option<CodexTurnFinal> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut session_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("codex-rollout")
        .to_string();
    let mut after_marker = false;
    let mut final_text = None;

    for line in reader.lines().map_while(Result::ok) {
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if event.get("type").and_then(Value::as_str) == Some("session_meta") {
            if let Some(id) = event.pointer("/payload/id").and_then(Value::as_str) {
                session_id = id.to_string();
            }
        }
        if event_contains_text(&event, correlation_id) {
            after_marker = true;
            final_text = None;
            continue;
        }
        if !after_marker {
            continue;
        }
        if event.pointer("/payload/type").and_then(Value::as_str) == Some("user_message") {
            if !event_contains_text(&event, correlation_id) {
                after_marker = false;
                final_text = None;
            }
            continue;
        }
        if let Some(text) = codex_assistant_text(&event) {
            if !text.trim().is_empty() {
                final_text = Some(text);
            }
        }
    }

    final_text.map(|text| CodexTurnFinal {
        session_id,
        rollout_path: path.display().to_string(),
        final_text: text,
    })
}

fn event_contains_text(event: &Value, needle: &str) -> bool {
    value_text(event).contains(needle)
}

fn codex_assistant_text(event: &Value) -> Option<String> {
    match event.pointer("/payload/type").and_then(Value::as_str) {
        Some("task_complete") => event
            .pointer("/payload/last_agent_message")
            .and_then(Value::as_str)
            .or_else(|| event.pointer("/payload/message").and_then(Value::as_str))
            .map(str::to_string),
        Some("agent_message") => event
            .pointer("/payload/message")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(items) => items.iter().map(value_text).collect::<Vec<_>>().join("\n"),
        Value::Object(map) => map.values().map(value_text).collect::<Vec<_>>().join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollout_extractor_prefers_task_complete_after_correlation() {
        let dir = std::env::temp_dir().join(format!(
            "missiond-codex-provider-box-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("rollout.jsonl");
        let corr = "corr-test";
        let user = json!({
            "type": "event_msg",
            "payload": {"type": "user_message", "message": format!("hello {corr}")}
        });
        let final_event = json!({
            "type": "event_msg",
            "payload": {
                "type": "task_complete",
                "last_agent_message": "final from durable rollout"
            }
        });
        fs::write(&path, format!("{user}\n{final_event}\n")).expect("write");

        let turn = extract_correlated_rollout(&path, corr).expect("turn");

        assert_eq!(turn.final_text, "final from durable rollout");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn correlated_prompt_preserves_existing_correlation() {
        let mut request = ProviderInteractionRequest::new(
            super::super::types::BoxCommand::WorkerTurn,
            CliEngine::Codex,
        );
        request.correlation_id = "corr-existing".to_string();
        request.prompt = Some("contains corr-existing already".to_string());

        assert_eq!(
            correlate_prompt(&request),
            "contains corr-existing already".to_string()
        );
    }

    #[test]
    fn codex_bypass_can_be_requested_from_top_level_or_tool_policy() {
        let mut top_level = ProviderInteractionRequest::new(
            super::super::types::BoxCommand::Status,
            CliEngine::Codex,
        );
        top_level.dangerously_bypass_approvals_and_sandbox = true;
        assert!(CodexProviderDriver::request_dangerous_bypass(&top_level));

        let mut policy = ProviderInteractionRequest::new(
            super::super::types::BoxCommand::Status,
            CliEngine::Codex,
        );
        policy.tool_policy = Some(json!({
            "bypass_approvals_and_sandbox": true,
        }));
        assert!(CodexProviderDriver::request_dangerous_bypass(&policy));
    }

    #[test]
    fn codex_default_reasoning_is_absent_when_model_profile_is_omitted() {
        let request = ProviderInteractionRequest::new(
            super::super::types::BoxCommand::Status,
            CliEngine::Codex,
        );

        assert!(request.model_profile.is_none());
    }

    #[test]
    fn codex_exec_text_args_disable_agentic_tools() {
        let output = Path::new("/tmp/missiond-codex-last.txt");
        let args = codex_exec_text_args(output, "gpt-5.5", Some("xhigh"));

        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--json" && pair[1] == "--output-last-message"));
        assert!(args.contains(&"--ignore-user-config".to_string()));
        assert!(args.contains(&"--ignore-rules".to_string()));
        assert!(args.contains(&"approval_policy=\"never\"".to_string()));
        assert!(args.contains(&"features.shell_tool=false".to_string()));
        assert!(args.contains(&"tools.web_search=false".to_string()));
        assert!(args.contains(&"apps._default.enabled=false".to_string()));
        assert!(args.contains(&"apps._default.default_tools_enabled=false".to_string()));
        assert!(args.contains(&"tools.view_image=false".to_string()));
        assert!(args.contains(&"model_reasoning_effort=\"xhigh\"".to_string()));
    }

    #[test]
    fn codex_exec_text_default_reasoning_omits_override() {
        let output = Path::new("/tmp/missiond-codex-last.txt");
        let args = codex_exec_text_args(output, "gpt-5.5", None);

        assert!(!args
            .iter()
            .any(|arg| arg.starts_with("model_reasoning_effort=")));
    }

    #[test]
    fn codex_exec_jsonl_analysis_detects_tool_violation() {
        let stdout = json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "shell_command",
                "arguments": "{}"
            }
        })
        .to_string();

        let analysis = analyze_codex_exec_jsonl(&stdout);

        assert_eq!(analysis.event_count, 1);
        assert!(analysis.violation.is_some());
    }

    #[test]
    fn codex_exec_jsonl_analysis_accepts_plain_final() {
        let stdout = json!({
            "type": "event_msg",
            "payload": {
                "type": "task_complete",
                "last_agent_message": "plain final"
            }
        })
        .to_string();

        let analysis = analyze_codex_exec_jsonl(&stdout);

        assert_eq!(analysis.final_text.as_deref(), Some("plain final"));
        assert!(analysis.violation.is_none());
    }
}
