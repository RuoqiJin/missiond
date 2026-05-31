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
use tokio::io::AsyncWriteExt;
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

    async fn ensure_codex_binary(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> bool {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let output = Command::new(shell)
            .arg("-l")
            .arg("-i")
            .arg("-c")
            .arg("command -v codex")
            .output()
            .await;
        match output {
            Ok(output) if output.status.success() => true,
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
                false
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
                false
            }
        }
    }

    async fn ensure_slot(
        &self,
        request: &ProviderInteractionRequest,
        result: &mut ProviderBoxResult,
    ) -> Option<String> {
        let slot_id = Self::request_slot_id(request);
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
                return Some(slot_id);
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

        let options = PTYSpawnOptions {
            auto_restart: true,
            wait_for_idle: true,
            timeout_secs: Some(90),
            mcp_config: None,
            dangerously_skip_permissions: false,
            model: request.model.clone(),
            reasoning_effort: request
                .model_profile
                .clone()
                .or_else(|| request.model.clone()),
            search_enabled: true,
            sandbox: Some(
                request
                    .tool_policy
                    .as_ref()
                    .and_then(|policy| policy.get("sandbox").and_then(Value::as_str))
                    .map(str::to_string)
                    .or_else(|| Some("read-only".to_string()))
                    .unwrap(),
            ),
            approval_policy: Some("never".to_string()),
            tool_policy_path: None,
            extra_env: HashMap::new(),
            initial_prompt: None,
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
        if step.verification_status == PtyStepVerificationStatus::Ambiguous {
            step.diagnostics.push(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                "Codex provider-box prompt submission was not observed by the PTY",
                json!({
                    "slot_id": slot_id,
                    "rule": "provider-box may not monitor a turn until prompt acceptance is observed",
                    "before_reason": before.snapshot.reason,
                    "after_reason": after.snapshot.reason,
                    "after_state": after.snapshot.state,
                }),
            ));
        }
        let ok = step.verification_status == PtyStepVerificationStatus::Verified;
        result.record_step(step);
        ok
    }

    fn should_use_exec_text_turn(request: &ProviderInteractionRequest) -> bool {
        request.attachments.is_empty()
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
            })
    }

    async fn submit_exec_text_turn(
        &self,
        request: &ProviderInteractionRequest,
    ) -> ProviderBoxResult {
        // Migration-only headless path for provider-box pure-text turns:
        // `codex exec --json --output-last-message` is allowed here only when
        // the request carries no_tools/no_mcp/no_shell/no_file_access guards.
        let mut result = ProviderBoxResult::base(request, ProviderBoxStatus::Unknown);
        if !Self::validate_prompt_turn(request, &mut result) {
            return result;
        }
        if !self.ensure_codex_binary(request, &mut result).await {
            return result;
        }

        let runtime_dir = provider_box_runtime_dir();
        let exec_dir = runtime_dir.join("codex-exec");
        let scratch_dir = exec_dir
            .join("scratch")
            .join(safe_file_component(&request.correlation_id));
        if let Err(err) =
            fs::create_dir_all(&scratch_dir).and_then(|_| fs::create_dir_all(&exec_dir))
        {
            result.status = ProviderBoxStatus::Failed;
            result.add_diagnostic(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                "Codex exec runtime directory could not be created",
                json!({
                    "error": err.to_string(),
                    "runtime_dir": runtime_dir.display().to_string(),
                }),
            ));
            return result;
        }

        let output_path = exec_dir.join(format!(
            "codex-exec-{}.txt",
            safe_file_component(&request.correlation_id)
        ));
        let prompt = text_only_exec_prompt(request);
        let mut command = Command::new("codex");
        command
            .arg("exec")
            .arg("--json")
            .arg("--color")
            .arg("never")
            .arg("--output-last-message")
            .arg(&output_path)
            .arg("--skip-git-repo-check")
            .arg("--ignore-rules")
            .arg("-C")
            .arg(&scratch_dir)
            .arg("--sandbox")
            .arg("read-only")
            .arg("--ask-for-approval")
            .arg("never");
        if let Some(model) = request
            .model
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            command.arg("--model").arg(model);
        }
        if let Some(effort) = request
            .model_profile
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            command
                .arg("-c")
                .arg(format!("model_reasoning_effort=\"{}\"", effort));
        }
        command
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let before = PtyObservation::structured(
            "codex-exec:before",
            "",
            json!({
                "mode": "codex_exec_text_only",
                "scratch_dir": scratch_dir.display().to_string(),
                "output_path": output_path.display().to_string(),
                "no_tools": request.no_tools,
                "no_mcp": request.no_mcp,
                "no_shell": request.no_shell,
                "no_file_access": request.no_file_access,
            }),
        );
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex exec text-only turn could not be spawned",
                    json!({
                        "error": err.to_string(),
                        "rule": "fast-fail; no fallback provider path is allowed",
                    }),
                ));
                return result;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(err) = stdin.write_all(prompt.as_bytes()).await {
                result.status = ProviderBoxStatus::Failed;
                result.add_diagnostic(ProviderBoxDiagnostic::error(
                    DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                    "Codex exec prompt could not be written to stdin",
                    json!({
                        "error": err.to_string(),
                    }),
                ));
                let _ = child.kill().await;
                return result;
            }
        }

        let timeout_secs = request.timeout_secs.unwrap_or(180).clamp(10, 7_200);
        let output =
            match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
                .await
            {
                Ok(Ok(output)) => output,
                Ok(Err(err)) => {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_BOX_SLOT_UNAVAILABLE,
                        "Codex exec text-only turn failed while waiting for output",
                        json!({
                            "error": err.to_string(),
                        }),
                    ));
                    return result;
                }
                Err(_) => {
                    result.status = ProviderBoxStatus::Failed;
                    result.add_diagnostic(ProviderBoxDiagnostic::error(
                        DIAG_PROVIDER_TURN_TIMEOUT_CANCEL_FAILED,
                        "Codex exec text-only turn timed out",
                        json!({
                            "timeout_secs": timeout_secs,
                            "cancel": DIAG_PROVIDER_TURN_TIMEOUT_CANCELLED,
                        }),
                    ));
                    return result;
                }
            };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let final_text = fs::read_to_string(&output_path).unwrap_or_default();
        let after = PtyObservation::structured(
            "codex-exec:after",
            stdout_tail(&stdout, 4_000),
            json!({
                "status": output.status.code(),
                "success": output.status.success(),
                "stderr_tail": stdout_tail(&stderr, 2_000),
                "output_path": output_path.display().to_string(),
                "final_text_len": final_text.len(),
            }),
        );
        let mut step = PtyStepRecord::new(
            before,
            PtyStepAction::text("<codex exec text-only prompt via stdin>"),
            after,
            Some("Codex exec writes final assistant text to output-last-message".to_string()),
            if output.status.success() && !final_text.trim().is_empty() {
                PtyStepVerificationStatus::Verified
            } else {
                PtyStepVerificationStatus::Failed
            },
        );

        if codex_exec_contains_tool_activity(&stdout) {
            step.diagnostics.push(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_TEXT_ONLY_VIOLATION,
                "Codex exec emitted tool activity during a text-only provider-box turn",
                json!({
                    "rule": "no_tools/no_shell/no_file_access turns must remain pure text",
                    "stdout_tail": stdout_tail(&stdout, 2_000),
                }),
            ));
            result.status = ProviderBoxStatus::Failed;
        } else if output.status.success() && !final_text.trim().is_empty() {
            result.status = ProviderBoxStatus::Completed;
            result.final_text = Some(final_text);
            result.durable_source = Some(output_path.display().to_string());
            result.provider_conversation_id = Some(request.correlation_id.clone());
        } else {
            result.status = ProviderBoxStatus::Failed;
            step.diagnostics.push(ProviderBoxDiagnostic::error(
                DIAG_PROVIDER_DURABLE_FINAL_MISSING,
                "Codex exec completed without a non-empty final message",
                json!({
                    "status": output.status.code(),
                    "stderr_tail": stdout_tail(&stderr, 2_000),
                    "stdout_tail": stdout_tail(&stdout, 2_000),
                    "output_path": output_path.display().to_string(),
                }),
            ));
        }
        result.record_step(step);
        result
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
            pure_text_guard: false,
            control_action: false,
        }
    }

    async fn submit_turn(&self, request: &ProviderInteractionRequest) -> ProviderBoxResult {
        if Self::should_use_exec_text_turn(request) {
            return self.submit_exec_text_turn(request).await;
        }
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
        self.submit_exec_text_turn(request).await
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

fn provider_box_runtime_dir() -> PathBuf {
    std::env::var("MISSIOND_RUNTIME_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".missiond/runtime/missiond"))
        })
        .unwrap_or_else(|| PathBuf::from(".missiond/runtime/missiond"))
        .join("provider-box")
}

fn safe_file_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .chars()
        .take(160)
        .collect()
}

fn stdout_tail(value: &str, max_chars: usize) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(max_chars);
    chars[start..].iter().collect()
}

fn text_only_exec_prompt(request: &ProviderInteractionRequest) -> String {
    let prompt = correlate_prompt(request);
    format!(
        "MissionD provider-box text-only turn.\n\
         Constraints:\n\
         - Do not run shell commands, MCP tools, web search, file reads, file writes, or any other tool.\n\
         - Use only the context included in this prompt.\n\
         - Return only the requested final answer; do not describe these constraints.\n\
         - Preserve the requested output format from the prompt/output contract.\n\n{}",
        prompt
    )
}

fn codex_exec_contains_tool_activity(stdout: &str) -> bool {
    stdout.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("\"exec_command\"")
            || lower.contains("\"tool_call\"")
            || lower.contains("\"function_call\"")
            || lower.contains("\"mcp_tool_call\"")
            || lower.contains("\"command_begin\"")
            || lower.contains("\"command_output\"")
    })
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
}
