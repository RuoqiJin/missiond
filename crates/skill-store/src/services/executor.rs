use crate::db;
use crate::error::{AppError, AppResult};
use crate::models::ExecStatus;
use crate::services::billing::BillingService;
use crate::services::defense::{self, DefenseLayer};
use crate::services::llm_proxy::LlmProxy;
use std::sync::Arc;

/// Core Skill execution pipeline
pub struct SkillExecutor {
    pub llm: LlmProxy,
    pub defense: Arc<DefenseLayer>,
    pub billing: Arc<BillingService>,
    pub db: Arc<db::Database>,
}

pub struct InvokeResult {
    pub content: String,
    pub execution_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

impl SkillExecutor {
    /// Full execution pipeline:
    /// 1. Auth & quota check
    /// 2. Input defense
    /// 3. Prompt assembly (sandwich)
    /// 4. LLM call
    /// 5. Output defense
    /// 6. Billing
    pub async fn invoke(
        &self,
        user_id: &str,
        skill_id: &str,
        user_inputs: &serde_json::Value,
    ) -> AppResult<InvokeResult> {
        let execution_id = uuid::Uuid::new_v4().to_string();

        // Step 1: Load skill & check quota
        let (skill, quota_check) = {
            let pool = self.db.pool();
            let skill = db::skills::get_skill_by_id(pool, skill_id)
                .await
                .map_err(|_| AppError::NotFound(format!("Skill {skill_id} not found")))?;

            if !skill.is_active {
                return Err(AppError::NotFound("Skill is inactive".into()));
            }

            let quota_check = self.billing.check_quota(pool, user_id).await?;

            // Verify tier access
            if skill.tier > quota_check.max_tier() {
                return Err(AppError::Forbidden(format!(
                    "Your plan allows up to tier {}, this skill requires tier {}",
                    quota_check.max_tier(),
                    skill.tier
                )));
            }

            (skill, quota_check)
        };

        // Step 2: Render user inputs into a single string
        let rendered_input = render_inputs(user_inputs);

        // Step 3: Input defense (concurrent with prompt assembly)
        let defense = self.defense.clone();
        let input_for_defense = rendered_input.clone();
        let defense_handle =
            tokio::spawn(async move { defense.check_input(&input_for_defense).await });

        // Step 4: Assemble sandwich prompt
        let (system_prompt, user_message) =
            defense::build_defended_prompt(&skill.prompt_template, &rendered_input);

        // Wait for defense check
        match defense_handle.await {
            Ok(Ok(())) => {} // Safe
            Ok(Err(e)) => {
                // Blocked — record and return
                let _ = self
                    .billing
                    .record_execution(
                        self.db.pool(),
                        &execution_id,
                        user_id,
                        skill_id,
                        &ExecStatus::BlockedInjection,
                        0,
                        0,
                        skill.price_per_use,
                        &quota_check,
                    )
                    .await;
                return Err(AppError::InjectionBlocked(e.to_string()));
            }
            Err(e) => {
                tracing::error!("Defense task panicked: {e}");
                // Fail open
            }
        }

        // Step 5: Call LLM
        let llm_result = self
            .llm
            .chat(&system_prompt, &user_message)
            .await
            .map_err(|e| AppError::LlmError(e.to_string()))?;

        // Step 6: Output defense
        if let Err(_e) = self
            .defense
            .check_output(&llm_result.content, &skill.prompt_template)
        {
            let _ = self
                .billing
                .record_execution(
                    self.db.pool(),
                    &execution_id,
                    user_id,
                    skill_id,
                    &ExecStatus::BlockedLeak,
                    llm_result.input_tokens,
                    llm_result.output_tokens,
                    skill.price_per_use,
                    &quota_check,
                )
                .await;
            return Err(AppError::LeakBlocked);
        }

        // Step 7: Record execution & billing
        {
            self.billing
                .record_execution(
                    self.db.pool(),
                    &execution_id,
                    user_id,
                    skill_id,
                    &ExecStatus::Success,
                    llm_result.input_tokens,
                    llm_result.output_tokens,
                    skill.price_per_use,
                    &quota_check,
                )
                .await?;
        }

        Ok(InvokeResult {
            content: llm_result.content,
            execution_id,
            input_tokens: llm_result.input_tokens,
            output_tokens: llm_result.output_tokens,
        })
    }
}

/// Render JSON inputs into a flat string for prompt injection
fn render_inputs(inputs: &serde_json::Value) -> String {
    match inputs {
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(k, v)| {
                let val = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                format!("{k}: {val}")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
