use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::Result;
use missiond_core::event::events::BoardEvent;
use missiond_core::types::CreateBoardTaskInput;
use missiond_mcp::tools::ToolResult;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;
use tokio::sync::{watch, RwLock};
use tracing::{info, warn};

use crate::engine::control_plane_kernel::{ControlPlaneKernel, UpsertTaskContractCommand};
use crate::state::AppState;

const NIGHTLY_REPORT_DIR: &str = ".missiond/v3/runtime/nightly-evolution";
const SELF_EVOLUTION_PROPOSAL_DIR: &str = ".missiond/v3/runtime/self-evolution";
const SELF_EVOLUTION_ANALYZER: &str = "scripts/analyze-v3-self-evolution.mjs";
const MAX_SELF_EVOLUTION_PROPOSALS: usize = 3;
const NIGHTLY_DEFAULT_INTERVAL_SECS: u64 = 86_400;
const NIGHTLY_SCHEDULE_ENABLED_ENV: &str = "MISSIOND_NIGHTLY_EVOLUTION_SCHEDULE";

static NIGHTLY_EVOLUTION_RUNTIME: OnceLock<Arc<NightlyEvolutionRuntime>> = OnceLock::new();

#[derive(Debug)]
struct NightlyEvolutionRuntime {
    runs: AtomicU64,
    reports_written: AtomicU64,
    followup_tasks_created: AtomicU64,
    last_run_at_epoch: AtomicI64,
    last_mode: RwLock<Option<String>>,
    last_report_path: RwLock<Option<String>>,
    last_findings_count: AtomicU64,
    last_task_id: RwLock<Option<String>>,
    last_error: RwLock<Option<String>>,
}

impl Default for NightlyEvolutionRuntime {
    fn default() -> Self {
        Self {
            runs: AtomicU64::new(0),
            reports_written: AtomicU64::new(0),
            followup_tasks_created: AtomicU64::new(0),
            last_run_at_epoch: AtomicI64::new(0),
            last_mode: RwLock::new(None),
            last_report_path: RwLock::new(None),
            last_findings_count: AtomicU64::new(0),
            last_task_id: RwLock::new(None),
            last_error: RwLock::new(None),
        }
    }
}

impl NightlyEvolutionRuntime {
    async fn record_result(&self, result: &NightlyRunResult) {
        self.runs.fetch_add(1, Ordering::Relaxed);
        self.reports_written.fetch_add(1, Ordering::Relaxed);
        self.last_run_at_epoch
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
        self.last_findings_count
            .store(result.findings.len() as u64, Ordering::Relaxed);
        *self.last_mode.write().await = Some(result.mode.clone());
        *self.last_report_path.write().await = Some(result.report_path.display().to_string());
        *self.last_task_id.write().await = result.followup_task_id.clone();
        *self.last_error.write().await = None;
        if result.followup_task_id.is_some() {
            self.followup_tasks_created.fetch_add(1, Ordering::Relaxed);
        }
    }

    async fn record_error(&self, err: impl Into<String>) {
        *self.last_error.write().await = Some(err.into());
    }

    async fn snapshot(&self) -> Value {
        json!({
            "schema": "missiond.nightly-evolution-status.v1",
            "scheduleEnabled": nightly_evolution_schedule_enabled(),
            "runs": self.runs.load(Ordering::Relaxed),
            "reportsWritten": self.reports_written.load(Ordering::Relaxed),
            "followupTasksCreated": self.followup_tasks_created.load(Ordering::Relaxed),
            "lastRunAtEpoch": self.last_run_at_epoch.load(Ordering::Relaxed),
            "lastMode": self.last_mode.read().await.clone(),
            "lastReportPath": self.last_report_path.read().await.clone(),
            "lastFindingsCount": self.last_findings_count.load(Ordering::Relaxed),
            "lastTaskId": self.last_task_id.read().await.clone(),
            "lastError": self.last_error.read().await.clone(),
        })
    }
}

fn runtime() -> Arc<NightlyEvolutionRuntime> {
    NIGHTLY_EVOLUTION_RUNTIME
        .get_or_init(|| Arc::new(NightlyEvolutionRuntime::default()))
        .clone()
}

pub(crate) async fn status_snapshot() -> Value {
    runtime().snapshot().await
}

pub(crate) fn start_nightly_evolution_service(
    state: &AppState,
    shutdown_rx: watch::Receiver<bool>,
) {
    if !nightly_evolution_schedule_enabled() {
        info!(
            env = NIGHTLY_SCHEDULE_ENABLED_ENV,
            "nightly-evolution schedule disabled by default; use mission_nightly_evolution or set env=true to run periodically"
        );
        return;
    }
    let state = state.clone();
    let runtime = runtime();
    tokio::spawn(async move { run_schedule_loop(state, runtime, shutdown_rx).await });
    info!("nightly-evolution service started (observe-first schedule)");
}

fn nightly_evolution_schedule_enabled() -> bool {
    matches!(
        std::env::var(NIGHTLY_SCHEDULE_ENABLED_ENV)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

async fn run_schedule_loop(
    state: AppState,
    runtime: Arc<NightlyEvolutionRuntime>,
    mut shutdown: watch::Receiver<bool>,
) {
    let interval_secs = std::env::var("MISSIOND_NIGHTLY_EVOLUTION_INTERVAL_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|secs| *secs >= 60)
        .unwrap_or(NIGHTLY_DEFAULT_INTERVAL_SECS);
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;

    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            _ = interval.tick() => {
                match run_nightly_evolution_once(&state, NightlyRunOptions {
                    mode: "observe-only".to_string(),
                    apply: false,
                    reason: "scheduled-nightly-evolution".to_string(),
                }).await {
                    Ok(result) => runtime.record_result(&result).await,
                    Err(err) => {
                        warn!(error = %err, "nightly-evolution scheduled run failed");
                        runtime.record_error(err.to_string()).await;
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NightlyEvolutionArgs {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    apply: Option<bool>,
    #[serde(default)]
    reason: Option<String>,
}

pub(crate) async fn mission_nightly_evolution(state: &AppState, args: Value) -> Result<ToolResult> {
    let parsed: NightlyEvolutionArgs =
        serde_json::from_value(args).unwrap_or(NightlyEvolutionArgs {
            mode: None,
            apply: None,
            reason: None,
        });
    let options = NightlyRunOptions {
        mode: parsed.mode.unwrap_or_else(|| "observe-only".to_string()),
        apply: parsed.apply.unwrap_or(false),
        reason: parsed
            .reason
            .unwrap_or_else(|| "manual-mission_nightly_evolution".to_string()),
    };
    let result = run_nightly_evolution_once(state, options).await?;
    runtime().record_result(&result).await;
    Ok(ToolResult::json_pretty(&json!({
        "schema": "missiond.nightly-evolution-run.v1",
        "ok": true,
        "mode": result.mode,
        "apply": result.apply,
        "reason": result.reason,
        "reportPath": result.report_path.display().to_string(),
        "proposalPaths": result.proposal_paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
        "analyzerDiagnostics": result.analyzer_diagnostics,
        "findings": result.findings,
        "followupTaskId": result.followup_task_id,
    })))
}

#[derive(Debug, Clone)]
struct NightlyRunOptions {
    mode: String,
    apply: bool,
    reason: String,
}

#[derive(Debug, Clone)]
struct NightlyRunResult {
    mode: String,
    apply: bool,
    reason: String,
    report_path: PathBuf,
    proposal_paths: Vec<PathBuf>,
    analyzer_diagnostics: Vec<String>,
    findings: Vec<NightlyFinding>,
    followup_task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NightlyFinding {
    id: String,
    #[serde(default)]
    proposal_id: Option<String>,
    #[serde(rename = "class")]
    class: String,
    risk: String,
    summary: String,
    #[serde(default)]
    evidence_refs: Vec<String>,
    #[serde(default)]
    affected_surfaces: Vec<String>,
    #[serde(default)]
    recommended_change: String,
    #[serde(default)]
    acceptance: Vec<String>,
    #[serde(default)]
    non_goals: Vec<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    next_action: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelfEvolutionAnalyzerOutput {
    ok: bool,
    #[serde(default)]
    findings: Vec<NightlyFinding>,
    #[serde(default)]
    diagnostics: Vec<String>,
}

#[derive(Debug)]
struct SelfEvolutionAnalyzerRun {
    ok: bool,
    findings: Vec<NightlyFinding>,
    diagnostics: Vec<String>,
}

async fn run_nightly_evolution_once(
    state: &AppState,
    options: NightlyRunOptions,
) -> Result<NightlyRunResult> {
    let root = missiond_root(state).await;
    let compile_diagnostics = ensure_compiled_runtime_available(&root).await;
    let convergence = read_final_convergence_snapshot(&root).await;
    let analyzer = run_self_evolution_analyzer(&root).await;
    let mut analyzer_diagnostics = compile_diagnostics;
    analyzer_diagnostics.extend(analyzer.diagnostics.clone());
    let mut findings = analyzer.findings;
    if !analyzer.ok {
        findings.insert(0, analyzer_error_finding(&analyzer_diagnostics));
    }
    let proposal_findings = select_proposal_findings(&findings, &options.mode);
    let proposal_paths = write_proposals(&root, &proposal_findings).await?;
    let followup_task_id = if options.apply {
        create_requested_followup_if_needed(
            state,
            &proposal_findings,
            &proposal_paths,
            &options,
            &convergence,
        )
        .await?
    } else {
        None
    };
    let report = NightlyReport {
        mode: options.mode.clone(),
        apply: options.apply,
        reason: options.reason.clone(),
        convergence,
        findings: findings.clone(),
        proposal_paths: proposal_paths.clone(),
        analyzer_diagnostics: analyzer_diagnostics.clone(),
        followup_task_id: followup_task_id.clone(),
    };
    let report_path = write_report(&root, &report).await?;
    Ok(NightlyRunResult {
        mode: options.mode,
        apply: options.apply,
        reason: options.reason,
        report_path,
        proposal_paths,
        analyzer_diagnostics,
        findings,
        followup_task_id,
    })
}

async fn missiond_root(state: &AppState) -> PathBuf {
    let registry = state.project_registry.read().await;
    registry
        .get("missiond")
        .map(|project| PathBuf::from(&project.path))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

async fn read_final_convergence_snapshot(root: &Path) -> Value {
    let output = tokio::time::timeout(
        Duration::from_secs(45),
        Command::new("node")
            .args([
                "scripts/check-v3-final-convergence.mjs",
                "--json",
                "--static-only",
            ])
            .current_dir(root)
            .output(),
    )
    .await;
    match output {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            serde_json::from_str::<Value>(&stdout).unwrap_or_else(|err| {
                json!({
                    "ok": false,
                    "failed_stage": "parse-final-convergence-json",
                    "error": err.to_string(),
                    "stdoutTail": tail(&stdout, 2000),
                })
            })
        }
        Ok(Err(err)) => json!({
            "ok": false,
            "failed_stage": "spawn-final-convergence",
            "error": err.to_string(),
        }),
        Err(_) => json!({
            "ok": false,
            "failed_stage": "timeout-final-convergence",
            "error": "check-v3-final-convergence timed out",
        }),
    }
}

async fn ensure_compiled_runtime_available(root: &Path) -> Vec<String> {
    let compiled_dir = root.join(".missiond/v3/runtime/compiled");
    let required = [
        compiled_dir.join("compiled-semantic-ir.json"),
        compiled_dir.join("compiled-workflows.json"),
    ];
    if required.iter().all(|path| path.exists()) {
        return Vec::new();
    }
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        Command::new("node")
            .args(["scripts/compile-v3-runtime.mjs", "--json"])
            .current_dir(root)
            .output(),
    )
    .await;
    match output {
        Ok(Ok(output)) if output.status.success() => Vec::new(),
        Ok(Ok(output)) => vec![format!(
            "compile-v3-runtime failed: {}",
            tail(
                &format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ),
                3000
            )
        )],
        Ok(Err(err)) => vec![format!("compile-v3-runtime failed to start: {err}")],
        Err(_) => vec!["compile-v3-runtime timed out".to_string()],
    }
}

async fn run_self_evolution_analyzer(root: &Path) -> SelfEvolutionAnalyzerRun {
    let output = tokio::time::timeout(
        Duration::from_secs(75),
        Command::new("node")
            .args([SELF_EVOLUTION_ANALYZER, "--json"])
            .current_dir(root)
            .output(),
    )
    .await;
    match output {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            match serde_json::from_str::<SelfEvolutionAnalyzerOutput>(&stdout) {
                Ok(parsed) => {
                    let mut diagnostics = parsed.diagnostics;
                    if !output.status.success() {
                        diagnostics.push(format!(
                            "self-evolution analyzer exited {}: {}",
                            output.status,
                            tail(&String::from_utf8_lossy(&output.stderr), 2000)
                        ));
                    }
                    SelfEvolutionAnalyzerRun {
                        ok: parsed.ok && output.status.success(),
                        findings: parsed.findings,
                        diagnostics,
                    }
                }
                Err(err) => SelfEvolutionAnalyzerRun {
                    ok: false,
                    findings: Vec::new(),
                    diagnostics: vec![format!(
                        "failed to parse self-evolution analyzer JSON: {err}; stdout_tail={}",
                        tail(&stdout, 2000)
                    )],
                },
            }
        }
        Ok(Err(err)) => SelfEvolutionAnalyzerRun {
            ok: false,
            findings: Vec::new(),
            diagnostics: vec![format!("self-evolution analyzer failed to start: {err}")],
        },
        Err(_) => SelfEvolutionAnalyzerRun {
            ok: false,
            findings: Vec::new(),
            diagnostics: vec!["self-evolution analyzer timed out".to_string()],
        },
    }
}

fn analyzer_error_finding(diagnostics: &[String]) -> NightlyFinding {
    NightlyFinding {
        id: "self-evolution-analyzer-error".to_string(),
        proposal_id: None,
        class: "requires-user-decision".to_string(),
        risk: "low".to_string(),
        summary: "Self-evolution analyzer failed; diagnostics were preserved in the nightly report."
            .to_string(),
        evidence_refs: diagnostics.to_vec(),
        affected_surfaces: vec!["nightly-evolution-loop".to_string()],
        recommended_change: "Repair analyzer/runtime projection before trusting self-evolution findings."
            .to_string(),
        acceptance: vec![
            "node scripts/analyze-v3-self-evolution.mjs --json".to_string(),
            "node scripts/check-v3-nightly-evolution-isomorphism.mjs".to_string(),
        ],
        non_goals: vec![
            "Do not dispatch implementation workers from analyzer failure.".to_string(),
            "Do not read KB, Board history, provider logs, or worker telemetry.".to_string(),
        ],
        created_at: None,
        next_action: Some(
            "Create a visible diagnostic proposal only if apply=true requests requires-user-decision."
                .to_string(),
        ),
    }
}

fn select_proposal_findings(findings: &[NightlyFinding], mode: &str) -> Vec<NightlyFinding> {
    let mut selected: Vec<NightlyFinding> = findings
        .iter()
        .filter(|finding| proposal_allowed_for_mode(finding, mode))
        .cloned()
        .collect();
    selected.sort_by(|a, b| {
        risk_rank(&a.risk)
            .cmp(&risk_rank(&b.risk))
            .then_with(|| a.id.cmp(&b.id))
    });
    selected.truncate(MAX_SELF_EVOLUTION_PROPOSALS);
    selected
}

fn proposal_allowed_for_mode(finding: &NightlyFinding, mode: &str) -> bool {
    if mode == "observe-only" {
        return true;
    }
    finding.class == mode && followup_allowed_for_mode(finding, mode)
}

fn followup_allowed_for_mode(finding: &NightlyFinding, mode: &str) -> bool {
    match mode {
        "safe-backfill" => finding.risk == "low",
        "needs-investigation" => true,
        "architecture-proposal" => true,
        "requires-user-decision" => true,
        _ => false,
    }
}

fn risk_rank(risk: &str) -> u8 {
    match risk {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        _ => 9,
    }
}

async fn create_requested_followup_if_needed(
    state: &AppState,
    proposal_findings: &[NightlyFinding],
    proposal_paths: &[PathBuf],
    options: &NightlyRunOptions,
    convergence: &Value,
) -> Result<Option<String>> {
    if proposal_findings.is_empty() || proposal_paths.is_empty() {
        return Ok(None);
    }
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let dedupe_key = format!("nightly-evolution:{date}:{}:proposal-review", options.mode);
    if let Some(existing) = state
        .store
        .find_open_task_by_dedupe_key(&dedupe_key)
        .await?
    {
        return Ok(Some(existing.id.to_string()));
    }
    let input = build_followup_task_input(
        options,
        proposal_findings,
        proposal_paths,
        convergence,
        dedupe_key,
    );
    let task = state.store.create_board_task(&input).await?;
    let task_id = task.id.to_string();
    ControlPlaneKernel::new(state)
        .upsert_task_contract_command(UpsertTaskContractCommand {
            task_id: task_id.clone(),
            project_id: task.project.clone(),
            runtime_metadata: task.runtime_metadata.clone(),
        })
        .await?;
    let ev = BoardEvent::TaskCreated {
        task_id: task_id.clone(),
        title: task.title.clone(),
        category: task.category.clone(),
    };
    crate::engine::master_control::notify_board_event_direct(&ev);
    let _ = state.bus.publish_board(ev).await;
    Ok(Some(task_id))
}

fn build_followup_task_input(
    options: &NightlyRunOptions,
    proposal_findings: &[NightlyFinding],
    proposal_paths: &[PathBuf],
    convergence: &Value,
    dedupe_key: String,
) -> CreateBoardTaskInput {
    let failed_stage = convergence
        .get("failed_stage")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let proposal_path_text = proposal_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let finding_ids = proposal_findings
        .iter()
        .map(|finding| finding.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let runtime_metadata = nightly_followup_runtime_metadata(
        options,
        proposal_findings,
        proposal_paths,
        convergence,
        dedupe_key.as_str(),
    );
    let description = format!(
        "Review MissionD self-evolution proposal artifact(s).\n\nMode: {}\nReason: {}\nFailed stage: {}\nFinding ids: {}\n\nProposal paths:\n{}\n\nAcceptance:\n1. Read the proposal Lisp artifact(s) and referenced SSOT/checker evidence.\n2. Decide whether to approve, reject, or ask for a narrower exact shard.\n3. Do not edit code, Lisp, or checker files from this review task.",
        options.mode,
        options.reason,
        failed_stage,
        finding_ids,
        proposal_path_text,
    );
    CreateBoardTaskInput {
        title: format!("Review nightly self-evolution proposals: {}", options.mode),
        description: Some(description),
        priority: Some("medium".to_string()),
        category: Some("dev".to_string()),
        project: Some("missiond".to_string()),
        auto_execute: Some(false),
        hidden: Some(false),
        dedupe_key: Some(dedupe_key),
        context_intent: Some("research".to_string()),
        runtime_metadata: Some(runtime_metadata),
        ..Default::default()
    }
}

fn nightly_followup_runtime_metadata(
    options: &NightlyRunOptions,
    proposal_findings: &[NightlyFinding],
    proposal_paths: &[PathBuf],
    convergence: &Value,
    dedupe_key: &str,
) -> Value {
    let failed_stage = convergence
        .get("failed_stage")
        .and_then(Value::as_str)
        .unwrap_or("none");
    json!({
        "schema": "missiond.board-task-runtime-metadata.v1",
        "source": "nightly_evolution",
        "control_state": "task_contracts",
        "dispatch_metadata": {
            "task_class": "self-evolution-proposal-review",
            "project_id": "missiond",
            "mode": options.mode.clone(),
            "reason": options.reason.clone(),
            "failed_stage": failed_stage,
            "dedupe_key": dedupe_key,
            "finding_ids": proposal_findings
                .iter()
                .map(|finding| finding.id.as_str())
                .collect::<Vec<_>>(),
            "completion_protocol": "review-only task; self-evolution remains proposal-only"
        },
        "read_scope": proposal_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        "write_scope": [],
        "must_not_touch": [],
        "capability_grant_ids": [],
        "sandbox_profile": "system-self-evolution-review",
        "projection_policy": "description_notes_are_projection_only"
    })
}

async fn write_proposals(
    root: &Path,
    findings: &[NightlyFinding],
) -> std::io::Result<Vec<PathBuf>> {
    if findings.is_empty() {
        return Ok(Vec::new());
    }
    let dir = root.join(SELF_EVOLUTION_PROPOSAL_DIR);
    tokio::fs::create_dir_all(&dir).await?;
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let created_at = chrono::Utc::now().to_rfc3339();
    let mut paths = Vec::with_capacity(findings.len());
    for finding in findings {
        let proposal_id = format!("self-evolution:{timestamp}:{}", finding.id);
        let path = dir.join(format!(
            "{}-{}.proposal.lisp",
            timestamp,
            sanitize_filename_segment(&finding.id)
        ));
        tokio::fs::write(&path, render_proposal(finding, &proposal_id, &created_at)).await?;
        paths.push(path);
    }
    Ok(paths)
}

fn sanitize_filename_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '-',
        })
        .collect()
}

fn render_proposal(finding: &NightlyFinding, proposal_id: &str, created_at: &str) -> String {
    format!(
        "(self-evolution-proposal\n  :proposal_id {}\n  :finding_id {}\n  :class {}\n  :risk {}\n  :summary {}\n  :evidence_refs {}\n  :affected_surfaces {}\n  :recommended_change {}\n  :acceptance {}\n  :non_goals {}\n  :created_at {}\n)\n",
        lisp_string(proposal_id),
        lisp_string(&finding.id),
        finding.class,
        finding.risk,
        lisp_string(&finding.summary),
        lisp_string_list(&finding.evidence_refs),
        lisp_string_list(&finding.affected_surfaces),
        lisp_string(&finding.recommended_change),
        lisp_string_list(&finding.acceptance),
        lisp_string_list(&finding.non_goals),
        lisp_string(finding.created_at.as_deref().unwrap_or(created_at)),
    )
}

#[derive(Debug)]
struct NightlyReport {
    mode: String,
    apply: bool,
    reason: String,
    convergence: Value,
    findings: Vec<NightlyFinding>,
    proposal_paths: Vec<PathBuf>,
    analyzer_diagnostics: Vec<String>,
    followup_task_id: Option<String>,
}

async fn write_report(root: &Path, report: &NightlyReport) -> std::io::Result<PathBuf> {
    let dir = root.join(NIGHTLY_REPORT_DIR);
    tokio::fs::create_dir_all(&dir).await?;
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let path = dir.join(format!("{date}.report.lisp"));
    tokio::fs::write(&path, render_report(report)).await?;
    Ok(path)
}

fn render_report(report: &NightlyReport) -> String {
    format!(
        "(nightly-evolution-report\n  :schema \"missiond.nightly-evolution-report.v1\"\n  :updated-at {}\n  :mode {}\n  :apply {}\n  :reason {}\n  :followup-task-id {}\n  :final-convergence-ok {}\n  :failed-stage {}\n  :proposal-paths {}\n  :analyzer-diagnostics {}\n  :findings {}\n)\n",
        lisp_string(&chrono::Utc::now().to_rfc3339()),
        lisp_string(&report.mode),
        if report.apply { "true" } else { "false" },
        lisp_string(&report.reason),
        lisp_option_string(report.followup_task_id.as_deref()),
        if report
            .convergence
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "true"
        } else {
            "false"
        },
        lisp_option_string(
            report
                .convergence
                .get("failed_stage")
                .and_then(Value::as_str)
        ),
        lisp_string_list(
            &report
                .proposal_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
        ),
        lisp_string_list(&report.analyzer_diagnostics),
        render_findings(&report.findings),
    )
}

fn render_findings(findings: &[NightlyFinding]) -> String {
    if findings.is_empty() {
        return "[]".to_string();
    }
    format!(
        "[{}]",
        findings
            .iter()
            .map(|finding| {
                format!(
                    "(:id {} :class {} :risk {} :summary {} :evidence-refs {} :affected-surfaces {} :recommended-change {} :acceptance {} :non-goals {} :next-action {})",
                    lisp_string(&finding.id),
                    finding.class,
                    finding.risk,
                    lisp_string(&finding.summary),
                    lisp_string_list(&finding.evidence_refs),
                    lisp_string_list(&finding.affected_surfaces),
                    lisp_string(&finding.recommended_change),
                    lisp_string_list(&finding.acceptance),
                    lisp_string_list(&finding.non_goals),
                    lisp_string(finding.next_action.as_deref().unwrap_or(&finding.recommended_change)),
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn lisp_option_string(value: Option<&str>) -> String {
    value.map(lisp_string).unwrap_or_else(|| "nil".to_string())
}

fn lisp_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

fn lisp_string_list(values: &[String]) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            values
                .iter()
                .map(|value| lisp_string(value))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

fn tail(text: &str, max: usize) -> String {
    let mut chars: Vec<char> = text.chars().rev().take(max).collect();
    chars.reverse();
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_finding(id: &str, class: &str, risk: &str) -> NightlyFinding {
        NightlyFinding {
            id: id.to_string(),
            proposal_id: None,
            class: class.to_string(),
            risk: risk.to_string(),
            summary: "summary with \"quotes\"\nand newline".to_string(),
            evidence_refs: vec![".missiond/v3/missiond-blueprint.lisp:1".to_string()],
            affected_surfaces: vec!["nightly-evolution-loop".to_string()],
            recommended_change: "Write a proposal only.".to_string(),
            acceptance: vec!["node scripts/analyze-v3-self-evolution.mjs --json".to_string()],
            non_goals: vec!["Do not edit code.".to_string()],
            created_at: None,
            next_action: Some("Review proposal.".to_string()),
        }
    }

    #[test]
    fn analyzer_json_parses_into_findings() {
        let parsed: SelfEvolutionAnalyzerOutput = serde_json::from_value(json!({
            "ok": true,
            "diagnostics": [],
            "findings": [{
                "id": "surface-flow-gap",
                "proposalId": null,
                "class": "safe-backfill",
                "risk": "low",
                "summary": "missing flow",
                "evidenceRefs": ["compiled-semantic-ir.json"],
                "affectedSurfaces": ["demo"],
                "recommendedChange": "add flow",
                "acceptance": ["node scripts/check-v3-pillar-flow-schema.mjs"],
                "nonGoals": ["do not edit code"],
                "createdAt": null,
                "nextAction": "review"
            }]
        }))
        .expect("valid analyzer output");
        assert!(parsed.ok);
        assert_eq!(parsed.findings[0].id, "surface-flow-gap");
        assert_eq!(
            parsed.findings[0].evidence_refs[0],
            "compiled-semantic-ir.json"
        );
    }

    #[test]
    fn proposal_selection_is_bounded_and_risk_sorted() {
        let findings = vec![
            sample_finding("medium-a", "architecture-proposal", "medium"),
            sample_finding("low-b", "safe-backfill", "low"),
            sample_finding("low-a", "safe-backfill", "low"),
            sample_finding("low-c", "safe-backfill", "low"),
        ];
        let selected = select_proposal_findings(&findings, "observe-only");
        assert_eq!(selected.len(), MAX_SELF_EVOLUTION_PROPOSALS);
        assert_eq!(selected[0].id, "low-a");
        assert_eq!(selected[1].id, "low-b");
        assert_eq!(selected[2].id, "low-c");
    }

    #[test]
    fn report_renders_proposal_paths_and_diagnostics() {
        let report = NightlyReport {
            mode: "observe-only".to_string(),
            apply: false,
            reason: "test".to_string(),
            convergence: json!({"ok": true}),
            findings: vec![sample_finding("surface-flow-gap", "safe-backfill", "low")],
            proposal_paths: vec![PathBuf::from(
                ".missiond/v3/runtime/self-evolution/20260522T000000Z-surface-flow-gap.proposal.lisp",
            )],
            analyzer_diagnostics: vec!["diagnostic".to_string()],
            followup_task_id: None,
        };
        let rendered = render_report(&report);
        assert!(rendered.contains("nightly-evolution-report"));
        assert!(rendered.contains(":proposal-paths"));
        assert!(rendered.contains("surface-flow-gap.proposal.lisp"));
        assert!(rendered.contains(":analyzer-diagnostics"));
    }

    #[test]
    fn proposal_renderer_escapes_strings_and_uses_fixed_fields() {
        let finding = sample_finding("surface-flow-gap", "safe-backfill", "low");
        let rendered = render_proposal(&finding, "proposal-1", "2026-05-22T00:00:00Z");
        assert!(rendered.contains("(self-evolution-proposal"));
        assert!(rendered.contains(":proposal_id"));
        assert!(rendered.contains(":finding_id"));
        assert!(rendered.contains(":evidence_refs"));
        assert!(rendered.contains(":affected_surfaces"));
        assert!(rendered.contains(":recommended_change"));
        assert!(rendered.contains(":non_goals"));
        assert!(rendered.contains("\\\"quotes\\\"\\nand newline"));
    }

    #[test]
    fn analyzer_failure_becomes_diagnostic_finding() {
        let finding = analyzer_error_finding(&["boom".to_string()]);
        assert_eq!(finding.id, "self-evolution-analyzer-error");
        assert_eq!(finding.class, "requires-user-decision");
        assert!(finding.evidence_refs.contains(&"boom".to_string()));
    }

    #[test]
    fn followup_task_is_visible_review_only() {
        let options = NightlyRunOptions {
            mode: "safe-backfill".to_string(),
            apply: true,
            reason: "test".to_string(),
        };
        let paths = vec![PathBuf::from(
            ".missiond/v3/runtime/self-evolution/20260522T000000Z-surface-flow-gap.proposal.lisp",
        )];
        let input = build_followup_task_input(
            &options,
            &[sample_finding("surface-flow-gap", "safe-backfill", "low")],
            &paths,
            &json!({"ok": true}),
            "dedupe".to_string(),
        );
        assert_eq!(input.auto_execute, Some(false));
        assert_eq!(input.hidden, Some(false));
        assert!(input
            .description
            .as_deref()
            .unwrap_or_default()
            .contains("Proposal paths:"));
        assert!(input
            .description
            .as_deref()
            .unwrap_or_default()
            .contains("Do not edit code"));
        let metadata = input.runtime_metadata.as_ref().expect("runtime metadata");
        assert_eq!(metadata["source"], "nightly_evolution");
        assert_eq!(metadata["control_state"], "task_contracts");
        assert_eq!(
            metadata["dispatch_metadata"]["task_class"],
            "self-evolution-proposal-review"
        );
        assert_eq!(metadata["write_scope"].as_array().unwrap().len(), 0);
        assert_eq!(metadata["sandbox_profile"], "system-self-evolution-review");
    }
}
