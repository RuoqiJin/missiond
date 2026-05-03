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

use crate::state::AppState;

const NIGHTLY_REPORT_DIR: &str = ".missiond/v3/runtime/nightly-evolution";
const NIGHTLY_DEFAULT_INTERVAL_SECS: u64 = 86_400;

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
    let state = state.clone();
    let runtime = runtime();
    tokio::spawn(async move { run_schedule_loop(state, runtime, shutdown_rx).await });
    info!("nightly-evolution service started (observe-first schedule)");
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
    findings: Vec<NightlyFinding>,
    followup_task_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NightlyFinding {
    id: String,
    class: String,
    risk: String,
    summary: String,
    evidence: Vec<String>,
    next_action: String,
}

async fn run_nightly_evolution_once(
    state: &AppState,
    options: NightlyRunOptions,
) -> Result<NightlyRunResult> {
    let root = missiond_root(state).await;
    let convergence = read_final_convergence_snapshot(&root).await;
    let recent_commits = read_recent_commits(&root).await.unwrap_or_default();
    let findings = build_findings(&convergence);
    let followup_task_id = if options.apply {
        create_requested_followup_if_needed(state, &findings, &options, &convergence).await?
    } else {
        None
    };
    let report = NightlyReport {
        mode: options.mode.clone(),
        apply: options.apply,
        reason: options.reason.clone(),
        convergence,
        recent_commits,
        findings: findings.clone(),
        followup_task_id: followup_task_id.clone(),
    };
    let report_path = write_report(&root, &report).await?;
    Ok(NightlyRunResult {
        mode: options.mode,
        apply: options.apply,
        reason: options.reason,
        report_path,
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

async fn read_recent_commits(root: &Path) -> Result<Vec<String>> {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        Command::new("git")
            .args(["log", "-5", "--oneline", "--", ".missiond/v3"])
            .current_dir(root)
            .output(),
    )
    .await??;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn build_findings(convergence: &Value) -> Vec<NightlyFinding> {
    let mut findings = vec![
        NightlyFinding {
            id: "v3-runtime-projection-review".to_string(),
            class: "needs-investigation".to_string(),
            risk: "low".to_string(),
            summary: "Review MissionD V3 runtime-projection surfaces for policy/code drift without reading KB, historical conversations, or provider logs.".to_string(),
            evidence: vec![
                ".missiond/v3/missiond-blueprint.lisp".to_string(),
                "node scripts/check-v3-final-convergence.mjs --json --static-only".to_string(),
            ],
            next_action: "Create a read-only V3 SSOT investigation only when mode=needs-investigation and apply=true.".to_string(),
        },
        NightlyFinding {
            id: "v3-surface-checker-drift".to_string(),
            class: "safe-backfill".to_string(),
            risk: "low".to_string(),
            summary: "Check whether V3 implementation-map surfaces still have matching checker pins and file anchors.".to_string(),
            evidence: vec![
                ".missiond/v3/missiond-blueprint.lisp".to_string(),
                "scripts/check-v3-code-isomorphism-complete.mjs".to_string(),
            ],
            next_action: "If a checker pin is missing, create an exact Lisp/checker backfill task.".to_string(),
        },
        NightlyFinding {
            id: "v3-logic-consistency-review".to_string(),
            class: "architecture-proposal".to_string(),
            risk: "low".to_string(),
            summary: "Inspect only MissionD V3 Lisp for contradictory loops, duplicated responsibilities, missing entry/core/egress steps, and unclear ownership.".to_string(),
            evidence: vec![
                ".missiond/v3/missiond-blueprint.lisp".to_string(),
                ".missiond/v3/evidence/".to_string(),
            ],
            next_action: "Write an architecture proposal only; do not open KB/history/memory tasks in default nightly mode.".to_string(),
        },
        NightlyFinding {
            id: "v3-lisp-density-review".to_string(),
            class: "architecture-proposal".to_string(),
            risk: "medium".to_string(),
            summary: "Review MissionD V3 Lisp for repeated prose and move evidence into sidecars while preserving executable entry/core/egress steps.".to_string(),
            evidence: vec![
                ".missiond/v3/missiond-blueprint.lisp".to_string(),
            ],
            next_action: "Generate proposal only; do not auto-compress without checker coverage.".to_string(),
        },
    ];
    if !convergence
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        findings.insert(
            0,
            NightlyFinding {
                id: "final-convergence-blocker".to_string(),
                class: "safe-backfill".to_string(),
                risk: "low".to_string(),
                summary: format!(
                    "Final convergence static snapshot is not green: {}",
                    convergence
                        .get("failed_stage")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                ),
                evidence: vec!["node scripts/check-v3-final-convergence.mjs --json --static-only".to_string()],
                next_action: "Create a visible investigation BoardTask with exact failed_stage and blocking items.".to_string(),
            },
        );
    }
    findings
}

fn select_requested_followup<'a>(
    findings: &'a [NightlyFinding],
    mode: &str,
) -> Option<&'a NightlyFinding> {
    if mode == "observe-only" {
        return None;
    }
    findings
        .iter()
        .find(|finding| finding.class == mode && followup_allowed_for_mode(finding, mode))
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

async fn create_requested_followup_if_needed(
    state: &AppState,
    findings: &[NightlyFinding],
    options: &NightlyRunOptions,
    convergence: &Value,
) -> Result<Option<String>> {
    let Some(finding) = select_requested_followup(findings, &options.mode) else {
        return Ok(None);
    };
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let dedupe_key = format!("nightly-evolution:{date}:{}", finding.id);
    if let Some(existing) = state
        .store
        .find_open_task_by_dedupe_key(&dedupe_key)
        .await?
    {
        return Ok(Some(existing.id.to_string()));
    }
    let failed_stage = convergence
        .get("failed_stage")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let description = format!(
        "Nightly evolution finding: {}\n\nMode: {}\nReason: {}\nFailed stage: {}\n\nEvidence:\n{}\n\nAcceptance:\n1. Read the nightly report and referenced Lisp/checker surfaces.\n2. Produce a context-pack or concise diagnosis only.\n3. Do not edit code unless a later task gives exact write scope.",
        finding.summary,
        options.mode,
        options.reason,
        failed_stage,
        finding.evidence.join("\n"),
    );
    let input = CreateBoardTaskInput {
        title: format!("Nightly evolution: {}", finding.id),
        description: Some(description),
        priority: Some("medium".to_string()),
        category: Some("dev".to_string()),
        project: Some("missiond".to_string()),
        auto_execute: Some(false),
        hidden: Some(false),
        dedupe_key: Some(dedupe_key),
        context_intent: Some("research".to_string()),
        ..Default::default()
    };
    let task = state.store.create_board_task(&input).await?;
    let task_id = task.id.to_string();
    let ev = BoardEvent::TaskCreated {
        task_id: task_id.clone(),
        title: task.title.clone(),
        category: task.category.clone(),
    };
    crate::engine::master_control::notify_board_event_direct(&ev);
    let _ = state.bus.publish_board(ev).await;
    Ok(Some(task_id))
}

#[derive(Debug)]
struct NightlyReport {
    mode: String,
    apply: bool,
    reason: String,
    convergence: Value,
    recent_commits: Vec<String>,
    findings: Vec<NightlyFinding>,
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
        "(nightly-evolution-report\n  :schema \"missiond.nightly-evolution-report.v1\"\n  :updated-at {}\n  :mode {}\n  :apply {}\n  :reason {}\n  :followup-task-id {}\n  :final-convergence-ok {}\n  :failed-stage {}\n  :recent-commits {}\n  :findings {}\n)\n",
        lisp_string(&chrono::Utc::now().to_rfc3339()),
        lisp_string(&report.mode),
        if report.apply { "true" } else { "false" },
        lisp_string(&report.reason),
        lisp_option_string(report.followup_task_id.as_deref()),
        if report.convergence.get("ok").and_then(Value::as_bool).unwrap_or(false) { "true" } else { "false" },
        lisp_option_string(report.convergence.get("failed_stage").and_then(Value::as_str)),
        lisp_string_list(&report.recent_commits),
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
                    "(:id {} :class {} :risk {} :summary {} :evidence {} :next-action {})",
                    lisp_string(&finding.id),
                    finding.class,
                    finding.risk,
                    lisp_string(&finding.summary),
                    lisp_string_list(&finding.evidence),
                    lisp_string(&finding.next_action),
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
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
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

    #[test]
    fn findings_focus_only_on_v3_ssot_topics() {
        let findings = build_findings(&json!({"ok": true}));
        let ids: Vec<_> = findings.iter().map(|finding| finding.id.as_str()).collect();
        assert!(ids.contains(&"v3-runtime-projection-review"));
        assert!(ids.contains(&"v3-surface-checker-drift"));
        assert!(ids.contains(&"v3-logic-consistency-review"));
        assert!(ids.contains(&"v3-lisp-density-review"));
        assert!(!ids.contains(&"commit-lisp-drift-loop"));
        assert!(!ids.contains(&"legacy-direct-worker-dual-track"));
        assert!(!ids.contains(&"pty-diagnostic-only-audit"));
    }

    #[test]
    fn failed_final_convergence_adds_low_risk_followup_finding() {
        let findings = build_findings(&json!({
            "ok": false,
            "failed_stage": "static-checkers"
        }));
        let finding = findings.first().expect("first finding");
        assert_eq!(finding.id, "final-convergence-blocker");
        assert_eq!(finding.class, "safe-backfill");
    }

    #[test]
    fn followup_selection_respects_requested_mode() {
        let findings = build_findings(&json!({"ok": true}));
        let selected = select_requested_followup(&findings, "needs-investigation")
            .expect("needs-investigation finding");
        assert_eq!(selected.id, "v3-runtime-projection-review");
        assert_eq!(selected.class, "needs-investigation");
    }

    #[test]
    fn followup_selection_does_not_fall_back_to_safe_backfill() {
        let findings = build_findings(&json!({"ok": true}));
        assert!(select_requested_followup(&findings, "observe-only").is_none());
        assert!(select_requested_followup(&findings, "safe-backfill").is_some());
        assert!(select_requested_followup(&findings, "unknown-mode").is_none());
    }

    #[test]
    fn report_renders_findings_and_recent_commits() {
        let report = NightlyReport {
            mode: "observe-only".to_string(),
            apply: false,
            reason: "test".to_string(),
            convergence: json!({"ok": true}),
            recent_commits: vec!["abc commit".to_string()],
            findings: build_findings(&json!({"ok": true})),
            followup_task_id: None,
        };
        let rendered = render_report(&report);
        assert!(rendered.contains("nightly-evolution-report"));
        assert!(rendered.contains("v3-runtime-projection-review"));
        assert!(rendered.contains("abc commit"));
    }
}
