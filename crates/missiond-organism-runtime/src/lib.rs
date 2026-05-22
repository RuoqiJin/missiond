use std::collections::BTreeMap;

use async_trait::async_trait;
use missiond_kernel::{
    ActivationMode, Cell, CellCtx, Effect, EventEnvelope, IdempotencyWindow, RuntimeBudgets,
};
use thiserror::Error;

pub mod autopilot;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("runtime budget exceeded: {0}")]
    Budget(String),
    #[error("effect interpreter failed: {0}")]
    Interpreter(String),
    #[error("cell timed out after {0}ms")]
    Timeout(u64),
}

#[async_trait]
pub trait EffectInterpreter: Send + Sync {
    async fn interpret(&self, effect: &Effect) -> Result<(), RuntimeError>;
}

pub struct NoopEffectInterpreter;

#[async_trait]
impl EffectInterpreter for NoopEffectInterpreter {
    async fn interpret(&self, _effect: &Effect) -> Result<(), RuntimeError> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct RuntimeGuard {
    budgets: RuntimeBudgets,
    event_counts: BTreeMap<String, u32>,
    idempotency: IdempotencyWindow,
}

impl RuntimeGuard {
    pub fn new(budgets: RuntimeBudgets) -> Self {
        Self {
            idempotency: IdempotencyWindow::new(budgets.idempotency_cache_size),
            event_counts: BTreeMap::new(),
            budgets,
        }
    }

    fn check_event(&mut self, event: &EventEnvelope) -> Result<(), RuntimeError> {
        if event.causation_depth > self.budgets.max_causation_depth {
            return Err(RuntimeError::Budget(format!(
                "causation depth {} > {}",
                event.causation_depth, self.budgets.max_causation_depth
            )));
        }
        let correlation = event
            .correlation_id
            .clone()
            .unwrap_or_else(|| event.id.clone());
        let count = self.event_counts.entry(correlation.clone()).or_insert(0);
        *count += 1;
        if *count > self.budgets.max_events_per_correlation {
            return Err(RuntimeError::Budget(format!(
                "correlation {correlation} exceeded {} events",
                self.budgets.max_events_per_correlation
            )));
        }
        Ok(())
    }

    fn retain_new_effects(&mut self, effects: Vec<Effect>) -> Vec<Effect> {
        effects
            .into_iter()
            .filter(|effect| {
                effect
                    .idempotency_key()
                    .map(|key| self.idempotency.insert_new(key))
                    .unwrap_or(true)
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeOutcome {
    pub mode: ActivationMode,
    pub effects: Vec<Effect>,
    pub executed: bool,
    pub quarantined: bool,
    pub diagnostics: Vec<String>,
}

pub struct CellRuntime<I> {
    mode: ActivationMode,
    guard: RuntimeGuard,
    interpreter: I,
}

impl<I> CellRuntime<I>
where
    I: EffectInterpreter,
{
    pub fn new(mode: ActivationMode, budgets: RuntimeBudgets, interpreter: I) -> Self {
        Self {
            mode,
            guard: RuntimeGuard::new(budgets),
            interpreter,
        }
    }

    pub async fn handle_event<C>(
        &mut self,
        genome_id: impl Into<String>,
        cell: &C,
        event: &EventEnvelope,
    ) -> Result<RuntimeOutcome, RuntimeError>
    where
        C: Cell,
    {
        self.guard.check_event(event)?;
        let ctx = CellCtx {
            genome_id: genome_id.into(),
            activation: self.mode,
        };
        let timeout_ms = self.guard.budgets.max_cell_runtime_ms;
        let effects = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            cell.on_event(&ctx, event),
        )
        .await
        .map_err(|_| RuntimeError::Timeout(timeout_ms))?;
        let effects = self.guard.retain_new_effects(effects);
        match self.mode {
            ActivationMode::Shadow => Ok(RuntimeOutcome {
                mode: self.mode,
                effects,
                executed: false,
                quarantined: false,
                diagnostics: vec!["shadow mode recorded expected effects".to_string()],
            }),
            ActivationMode::Rollback => Ok(RuntimeOutcome {
                mode: self.mode,
                effects,
                executed: false,
                quarantined: false,
                diagnostics: vec!["rollback mode delegated to legacy path".to_string()],
            }),
            ActivationMode::Active => {
                for effect in &effects {
                    if !matches!(effect, Effect::Noop) {
                        self.interpreter.interpret(effect).await?;
                    }
                }
                Ok(RuntimeOutcome {
                    mode: self.mode,
                    effects,
                    executed: true,
                    quarantined: false,
                    diagnostics: Vec::new(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use missiond_kernel::{CommandEnvelope, Effect};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CommandCell;

    #[async_trait]
    impl Cell for CommandCell {
        fn id(&self) -> &'static str {
            "test-cell"
        }

        fn tissue(&self) -> &'static str {
            "test"
        }

        fn receptors(&self) -> &'static [&'static str] {
            &["TestEvent"]
        }

        async fn on_event(
            &self,
            _ctx: &missiond_kernel::CellCtx,
            _event: &EventEnvelope,
        ) -> Vec<Effect> {
            vec![Effect::Command(
                CommandEnvelope::new("DoThing", json!({})).with_idempotency_key("same-command"),
            )]
        }
    }

    struct CountingInterpreter(Arc<AtomicUsize>);

    #[async_trait]
    impl EffectInterpreter for CountingInterpreter {
        async fn interpret(&self, _effect: &Effect) -> Result<(), RuntimeError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn shadow_does_not_execute_effects() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut runtime = CellRuntime::new(
            ActivationMode::Shadow,
            RuntimeBudgets::default(),
            CountingInterpreter(count.clone()),
        );
        let event = EventEnvelope::new("TestEvent", json!({}));
        let outcome = runtime
            .handle_event("test-genome", &CommandCell, &event)
            .await
            .unwrap();
        assert_eq!(outcome.effects.len(), 1);
        assert!(!outcome.executed);
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn active_deduplicates_idempotency_keys() {
        let count = Arc::new(AtomicUsize::new(0));
        let mut runtime = CellRuntime::new(
            ActivationMode::Active,
            RuntimeBudgets::default(),
            CountingInterpreter(count.clone()),
        );
        let mut event = EventEnvelope::new("TestEvent", json!({}));
        event.correlation_id = Some("corr".to_string());
        let first = runtime
            .handle_event("test-genome", &CommandCell, &event)
            .await
            .unwrap();
        let second = runtime
            .handle_event("test-genome", &CommandCell, &event)
            .await
            .unwrap();
        assert_eq!(first.effects.len(), 1);
        assert_eq!(second.effects.len(), 0);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
