//! Lisp Survey Worker — commit-triggered intent.lisp maintenance.
//!
//! Subscribes to `ContextualCommitDetected` events from the EventBus.
//! Resolves the project from the committing slot's cwd, checks if the project
//! has an intent.lisp, and routes an incremental deep-survey prompt to the
//! `lisp-surveyor` persistent Claude Code (Sonnet) slot.
//!
//! Flow: TaggerChunker emits commit → this worker debounces → resolves project
//! → builds incremental survey prompt → slot_manager.execute("lisp_survey", prompt)

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::state::AppState;
use missiond_core::event::events::SystemEvent;
use missiond_core::event::subscription::SubscriptionOpts;

use super::{BackgroundWorker, WorkerContext, WorkerKind};

/// Debounce window — batch rapid commits on the same project.
const DEBOUNCE_SECS: u64 = 60;
/// Self-trigger filter prefix (commits made by survey updates).
const SURVEY_COMMIT_PREFIX: &str = "chore(survey):";
/// Maximum diff size sent to the surveyor slot.
const MAX_DIFF_SIZE: usize = 2_000_000;

pub(crate) struct LispSurveyWorker;

impl BackgroundWorker for LispSurveyWorker {
    const KIND: WorkerKind = WorkerKind::Sonnet;

    fn name(&self) -> &'static str {
        "lisp_survey"
    }

    async fn run(self, state: Arc<AppState>, mut ctx: WorkerContext) {
        let mut sub = match state
            .bus
            .subscribe::<SystemEvent>("lisp_survey", SubscriptionOpts::named("lisp_survey"))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "lisp_survey: bus subscribe failed, worker exiting");
                return;
            }
        };
        // project_id → last trigger time
        let mut debounce: HashMap<String, Instant> = HashMap::new();
        let mut processing: HashSet<String> = HashSet::new();

        info!("Lisp survey worker started (v2 bus)");

        loop {
            ctx.wait_if_paused().await;
            let Some(ack) = sub.next().await else {
                break;
            };

            let SystemEvent::ContextualCommitDetected {
                commit_hash,
                summary,
                slot_id,
                ..
            } = ack.event().clone()
            else {
                ack.ack().await;
                continue;
            };

            // Self-trigger filter
            if summary.starts_with(SURVEY_COMMIT_PREFIX) {
                debug!("lisp_survey: skipping own commit");
                ack.ack().await;
                continue;
            }

            // Resolve project from slot's cwd
            let project_id = match resolve_project(&state, slot_id.as_deref()).await {
                Some(pid) => pid,
                None => {
                    debug!(
                        slot_id = ?slot_id,
                        "lisp_survey: cannot resolve project for slot"
                    );
                    ack.ack().await;
                    continue;
                }
            };

            // Check if project has intent.lisp
            let intent_path = match find_intent_lisp(&state, &project_id).await {
                Some(p) => p,
                None => {
                    debug!(
                        project = %project_id,
                        "lisp_survey: project has no intent.lisp, skipping"
                    );
                    ack.ack().await;
                    continue;
                }
            };

            // Debounce per project
            let now = Instant::now();
            if let Some(last) = debounce.get(&project_id) {
                if now.duration_since(*last) < Duration::from_secs(DEBOUNCE_SECS) {
                    debug!(project = %project_id, "lisp_survey: debounced");
                    ack.ack().await;
                    continue;
                }
            }

            // Concurrency guard
            if processing.contains(&project_id) {
                debug!(project = %project_id, "lisp_survey: already processing");
                ack.ack().await;
                continue;
            }

            debounce.insert(project_id.clone(), now);
            processing.insert(project_id.clone());

            let hash = commit_hash.clone();
            let summary_text = summary.clone();
            let pid = project_id.clone();

            match process_survey(&state, &pid, &hash, &summary_text, &intent_path).await {
                Ok(true) => {
                    info!(
                        project = %pid,
                        commit = %&hash[..7.min(hash.len())],
                        "lisp_survey: intent.lisp updated"
                    );
                    ctx.record_success();
                }
                Ok(false) => {
                    debug!(project = %pid, "lisp_survey: NO_CHANGE");
                }
                Err(e) => {
                    warn!(error = %e, project = %pid, "lisp_survey: failed");
                    ctx.record_failure();
                }
            }

            processing.remove(&project_id);
            ack.ack().await;
        }
    }
}

/// Resolve project_id from a slot's cwd via ProjectRegistry.
async fn resolve_project(state: &AppState, slot_id: Option<&str>) -> Option<String> {
    let sid = slot_id?;
    let cwd = state.mission.get_slot(sid)?.config.cwd?;
    let registry = state.project_registry.read().await;
    registry.resolve(&cwd).map(|s| s.to_string())
}

/// Find intent.lisp path for a project. Returns absolute path if exists.
async fn find_intent_lisp(state: &AppState, project_id: &str) -> Option<String> {
    let registry = state.project_registry.read().await;
    let project = registry.get(project_id)?;

    // Check explicit intent_path from project config
    if let Some(ref rel) = project.intent_path {
        let abs = std::path::Path::new(&project.path).join(rel);
        if abs.exists() {
            return Some(abs.to_string_lossy().to_string());
        }
    }

    // Fallback: check standard location
    let standard = std::path::Path::new(&project.path).join(".missiond/intent.lisp");
    if standard.exists() {
        return Some(standard.to_string_lossy().to_string());
    }

    None
}

/// Get the project root path.
async fn project_root(state: &AppState, project_id: &str) -> Option<String> {
    let registry = state.project_registry.read().await;
    registry.get(project_id).map(|p| p.path.clone())
}

/// Build the incremental survey prompt for a commit.
fn build_survey_prompt(
    commit_hash: &str,
    summary: &str,
    intent_path: &str,
    diff: &str,
    project_root: &str,
) -> String {
    format!(
        r#"你是 intent.lisp 测绘维护专家。一个新的 commit 刚刚落地，请执行增量测绘更新。

## Commit 信息
- Hash: {commit_hash}
- Summary: {summary}
- Project root: {project_root}

## intent.lisp 路径
{intent_path}

## 代码变更（git diff）
{diff}

## 任务
1. 读取当前 intent.lisp（用 Read 工具）
2. 分析 diff 对架构的影响
3. 如果仅为实现细节（bug fix、文案、函数体内修改），不修改 intent.lisp，直接回复 `NO_CHANGE`
4. 如果涉及模块拓扑、接口契约、数据流、新增实体、新 migration、新 MCP 工具，用 Edit 工具更新 intent.lisp 对应 section
5. 更新 survey-hash（追加新 commit hash）和 last-updated 日期注释

## 规则
- 不动未变部分，只增/改/删 commit 涉及的 section
- 保持 S-expression 格式规范和括号平衡
- 每个 component 必须有 :target 指向实际文件
- 完成后回复一行摘要说明改了什么"#
    )
}

/// Main processing logic for a single commit.
async fn process_survey(
    state: &AppState,
    project_id: &str,
    commit_hash: &str,
    summary: &str,
    intent_path: &str,
) -> Result<bool> {
    let root = project_root(state, project_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Cannot find root for project '{}'", project_id))?;

    // Get diff content
    let root_clone = root.clone();
    let hash = commit_hash.to_string();
    let diff = tokio::task::spawn_blocking(move || -> Result<String> {
        let output = std::process::Command::new("git")
            .args(["diff-tree", "-p", "--no-commit-id", &hash])
            .current_dir(&root_clone)
            .output()
            .map_err(|e| anyhow::anyhow!("git diff-tree failed: {}", e))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "git diff-tree failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking: {}", e))??;

    // Truncate extreme diffs
    let diff = if diff.len() > MAX_DIFF_SIZE {
        format!(
            "{}...\n[截断: diff 原始 {} 字节，仅保留前 {}]",
            &diff[..MAX_DIFF_SIZE],
            diff.len(),
            MAX_DIFF_SIZE
        )
    } else {
        diff
    };

    let prompt = build_survey_prompt(commit_hash, summary, intent_path, &diff, &root);

    info!(
        project = %project_id,
        hash = %&commit_hash[..7.min(commit_hash.len())],
        prompt_len = prompt.len(),
        "lisp_survey: routing to surveyor slot"
    );

    // Route through SlotOrchestrator → persistent lisp-surveyor slot
    let response = state.slot_manager.execute("lisp_survey", &prompt).await?;

    if response.trim() == "NO_CHANGE" || response.contains("NO_CHANGE") {
        debug!(project = %project_id, "lisp_survey: surveyor says NO_CHANGE");
        return Ok(false);
    }

    Ok(true)
}
// trigger test 1775983673

