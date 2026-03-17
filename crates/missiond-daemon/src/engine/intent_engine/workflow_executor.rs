//! Workflow Executor — run skill workflow steps sequentially with MCP tool calls.
//!
//! Extracted from AppState::execute_workflow in main.rs.
//! Supports dry-run preview, context hooks, variable resolution,
//! retry/fallback/skip error handling, and recursive sub-workflows.

use anyhow::{anyhow, Result};
use serde_json::Value;
use tracing::{debug, info, warn};

use missiond_mcp::tools::ToolResult;
use crate::state::AppState;

impl AppState {
    /// Execute a skill workflow: load workflow block, run MCP tools sequentially
    pub(crate) fn execute_workflow<'a>(
        &'a self,
        skill_name: &'a str,
        action_id: &'a str,
        dry_run: bool,
        param_overrides: Option<Value>,
        depth: u32,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<missiond_core::WorkflowResult>> + Send + 'a>> {
        const MAX_DEPTH: u32 = 3;
        const STEP_TIMEOUT_SECS: u64 = 30;

        Box::pin(async move {
        use missiond_core::{WorkflowStepPreview, WorkflowStepResult, WorkflowResult, parse_workflow_blocks, resolve_vars};

        // Guard: prevent recursive workflow bombs
        if depth > MAX_DEPTH {
            return Err(anyhow!("Workflow recursion depth exceeded (max {}). Skill '{}' action '{}'", MAX_DEPTH, skill_name, action_id));
        }
        // Guard: prevent concurrent execution of same action
        if !dry_run {
            if let Ok(true) = self.store.skill_execution_is_running(skill_name, action_id).await {
                return Err(anyhow!("Action '{}' on skill '{}' is already running", action_id, skill_name));
            }
        }

        // Step 1: Load skill content from file
        let topic = self.store.skill_topic_get(skill_name).await
            .map_err(|e| anyhow!("DB: {}", e))?
            .ok_or_else(|| anyhow!("Skill '{}' not found", skill_name))?;

        let content = std::fs::read_to_string(&topic.file_path)
            .map_err(|e| anyhow!("Failed to read skill file {}: {}", topic.file_path, e))?;

        // Step 2: Parse workflow blocks from skill content
        let workflows = parse_workflow_blocks(&content);
        let workflow = workflows.iter()
            .find(|w| w.id == action_id)
            .ok_or_else(|| anyhow!("Workflow '{}' not found in skill '{}'", action_id, skill_name))?;

        // Step 3: Check requires_approval from frontmatter actions
        let actions_json = topic.actions_json.as_deref().unwrap_or("[]");
        let actions: Vec<missiond_core::SkillAction> = serde_json::from_str(actions_json).unwrap_or_default();
        let action_meta = actions.iter().find(|a| a.id == action_id);
        let requires_approval = action_meta.map(|a| a.requires_approval).unwrap_or(false);

        if requires_approval && !dry_run {
            return Ok(WorkflowResult::PendingApproval {
                action_id: action_id.to_string(),
                skill: skill_name.to_string(),
            });
        }

        // Step 4: Dry-run → return preview only
        if dry_run {
            let steps: Vec<WorkflowStepPreview> = workflow.steps.iter().map(|s| {
                WorkflowStepPreview {
                    name: s.name.clone(),
                    tool: s.tool.clone(),
                    params: s.params.clone(),
                }
            }).collect();
            return Ok(WorkflowResult::Preview { steps });
        }

        // Step 5: Create execution log
        let exec_id = uuid::Uuid::new_v4().to_string();
        let _ = self.store.skill_execution_insert(
            &exec_id, skill_name, action_id,
            workflow.steps.len() as i32, "manual",
        ).await;
        let exec_start = std::time::Instant::now();

        // Step 5b: Execute context hooks (pre-flight probes, best-effort)
        let mut context: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if let Some(ref hooks_json) = topic.context_hooks_json {
            if let Ok(hooks) = serde_json::from_str::<Vec<missiond_core::ContextHook>>(hooks_json) {
                for hook in &hooks {
                    let hook_result = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        self.call_tool(&hook.tool, hook.params.clone()),
                    ).await;
                    match hook_result {
                        Ok(result) => {
                            let output = result.content.first()
                                .map(|c| match c { missiond_mcp::ToolContent::Text { text } => text.clone() })
                                .unwrap_or_default();
                            // Escape ${...} in hook output to prevent injection into resolve_vars()
                            let safe_output = output.replace("${", "$\\{");
                            context.insert(hook.save_as.clone(), safe_output);
                            debug!(hook = %hook.tool, save_as = %hook.save_as, "Context hook completed");
                        }
                        Err(_) => {
                            warn!(hook = %hook.tool, "Context hook timed out (10s), skipping");
                        }
                    }
                }
            }
        }

        // Step 6: Sequential execution

        // Apply param_overrides to context
        if let Some(overrides) = param_overrides {
            if let Value::Object(map) = overrides {
                for (k, v) in map {
                    context.insert(k, v.as_str().unwrap_or(&v.to_string()).to_string());
                }
            }
        }

        let mut results: Vec<WorkflowStepResult> = Vec::new();
        let mut i = 0usize;
        let mut visit_counts: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
        const MAX_STEP_VISITS: u32 = 5; // absolute ceiling per step

        while i < workflow.steps.len() {
            let step = &workflow.steps[i];

            // Guard: prevent infinite fallback loops
            let visits = visit_counts.entry(i).or_insert(0);
            *visits += 1;
            if *visits > MAX_STEP_VISITS {
                let duration_ms = exec_start.elapsed().as_millis() as i64;
                let err_msg = format!("Step {} ('{}') visited {} times — infinite loop detected", i, step.tool, visits);
                warn!(%err_msg);
                let _ = self.store.skill_execution_update_with_duration(
                    &exec_id, "failed", (i + 1) as i32,
                    Some(&serde_json::to_string(&context).unwrap_or_default()),
                    Some(&err_msg),
                    Some(duration_ms),
                ).await;
                return Ok(WorkflowResult::Failed {
                    steps_completed: i,
                    error_step: i,
                    error: err_msg,
                    results,
                });
            }

            info!(exec_id = %exec_id, step = i, tool = %step.tool, "Executing workflow step");

            // Resolve ${var} references in params
            let resolved_params = resolve_vars(&step.params, &context);

            // Call the MCP tool with timeout
            let tool_result = match tokio::time::timeout(
                std::time::Duration::from_secs(STEP_TIMEOUT_SECS),
                self.call_tool(&step.tool, resolved_params),
            ).await {
                Ok(result) => result,
                Err(_) => {
                    let mut res = ToolResult::text(format!("Step timed out after {}s: {}", STEP_TIMEOUT_SECS, step.tool));
                    res.is_error = Some(true);
                    res
                }
            };
            let is_error = tool_result.is_error.unwrap_or(false);
            let output = tool_result.content.first()
                .map(|c| match c { missiond_mcp::ToolContent::Text { text } => text.clone() })
                .unwrap_or_default();

            // Save result to context if save_as is specified
            if let Some(ref key) = step.save_as {
                context.insert(key.clone(), output.clone());
            }

            results.push(WorkflowStepResult {
                name: step.name.clone(),
                tool: step.tool.clone(),
                success: !is_error,
                output: output.clone(),
            });

            // Update progress
            let _ = self.store.skill_execution_update(
                &exec_id, "running", (i + 1) as i32, None, None,
            ).await;

            // Error handling
            if is_error {
                let on_error = step.on_error.as_str();
                match on_error {
                    "skip" => {
                        warn!(step = i, tool = %step.tool, "Step failed, skipping");
                    }
                    "retry" => {
                        let max = step.max_retries.max(1);
                        let mut succeeded = false;
                        for attempt in 1..=max {
                            let backoff_secs = 1u64 << (attempt - 1).min(4);
                            warn!(step = i, tool = %step.tool, attempt, max, backoff_secs, "Retrying step");
                            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                            let retry_params = resolve_vars(&step.params, &context);
                            let retry_result = match tokio::time::timeout(
                                std::time::Duration::from_secs(STEP_TIMEOUT_SECS),
                                self.call_tool(&step.tool, retry_params),
                            ).await {
                                Ok(r) => r,
                                Err(_) => {
                                    let mut r = ToolResult::text("Retry timed out".to_string());
                                    r.is_error = Some(true);
                                    r
                                }
                            };
                            if !retry_result.is_error.unwrap_or(false) {
                                let retry_output = retry_result.content.first()
                                    .map(|c| match c { missiond_mcp::ToolContent::Text { text } => text.clone() })
                                    .unwrap_or_default();
                                if let Some(ref key) = step.save_as {
                                    context.insert(key.clone(), retry_output.clone());
                                }
                                if let Some(last) = results.last_mut() {
                                    last.success = true;
                                    last.output = retry_output;
                                }
                                succeeded = true;
                                break;
                            }
                        }
                        if !succeeded {
                            let duration_ms = exec_start.elapsed().as_millis() as i64;
                            let _ = self.store.skill_execution_update_with_duration(
                                &exec_id, "failed", (i + 1) as i32,
                                Some(&serde_json::to_string(&context).unwrap_or_default()),
                                Some(&format!("Failed after {} retries: {}", max, output)),
                                Some(duration_ms),
                            ).await;
                            return Ok(WorkflowResult::Failed {
                                steps_completed: i + 1,
                                error_step: i,
                                error: format!("Failed after {} retries: {}", max, output),
                                results,
                            });
                        }
                    }
                    s if s.starts_with("fallback:") => {
                        let target_id = &s["fallback:".len()..];
                        if let Some(target_idx) = workflow.steps.iter().position(|st| st.id.as_deref() == Some(target_id)) {
                            warn!(step = i, tool = %step.tool, target = target_id, target_idx, "Falling back");
                            i = target_idx;
                            continue; // Jump without incrementing
                        } else {
                            let duration_ms = exec_start.elapsed().as_millis() as i64;
                            let err_msg = format!("Fallback target '{}' not found", target_id);
                            let _ = self.store.skill_execution_update_with_duration(
                                &exec_id, "failed", (i + 1) as i32,
                                Some(&serde_json::to_string(&context).unwrap_or_default()),
                                Some(&err_msg),
                                Some(duration_ms),
                            ).await;
                            return Ok(WorkflowResult::Failed {
                                steps_completed: i + 1,
                                error_step: i,
                                error: err_msg,
                                results,
                            });
                        }
                    }
                    _ => {
                        // "stop" (default)
                        let duration_ms = exec_start.elapsed().as_millis() as i64;
                        let _ = self.store.skill_execution_update_with_duration(
                            &exec_id, "failed", (i + 1) as i32,
                            Some(&serde_json::to_string(&context).unwrap_or_default()),
                            Some(&output),
                            Some(duration_ms),
                        ).await;
                        return Ok(WorkflowResult::Failed {
                            steps_completed: i + 1,
                            error_step: i,
                            error: output,
                            results,
                        });
                    }
                }
            }
            i += 1;
        }

        // Success
        let duration_ms = exec_start.elapsed().as_millis() as i64;
        let _ = self.store.skill_execution_update_with_duration(
            &exec_id, "success", workflow.steps.len() as i32,
            Some(&serde_json::to_string(&context).unwrap_or_default()),
            None,
            Some(duration_ms),
        ).await;

        Ok(WorkflowResult::Success {
            steps_completed: workflow.steps.len(),
            results,
        })
        }) // Box::pin(async move)
    }
}
