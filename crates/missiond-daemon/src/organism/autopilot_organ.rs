use std::path::PathBuf;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Utc;
use missiond_core::event::{BoardEvent, IncidentEvent, SlotEvent};
use missiond_core::types::{IncidentSeverity, IncidentSource, MissionIncident};
use missiond_kernel::{ActivationMode, Effect, RuntimeBudgets};
use missiond_organism_runtime::autopilot::{
    activation_from_env, event_from_board, event_from_slot, notify_command_event,
    system_tick_event, AutopilotCell, AUTOPILOT_GENOME_ID, CMD_NOTIFY_AUTOPILOT_DISPATCH,
    CMD_RUN_AUTOPILOT_TICK, CMD_RUN_BOARD_DISPATCH,
};
use missiond_organism_runtime::{CellRuntime, EffectInterpreter, RuntimeError, RuntimeOutcome};
use serde_json::json;
use tracing::{debug, warn};

use crate::state::AppState;

const SHADOW_PATH: &str = ".missiond/v3/runtime/genome/autopilot-shadow.json";
const ACTIVATION_PATH: &str = ".missiond/v3/runtime/genome/activation.json";

pub(crate) fn autopilot_genome_mode() -> ActivationMode {
    if std::env::var("MISSIOND_GENOME_AUTOPILOT_MODE").is_ok() {
        return activation_from_env();
    }
    std::fs::read_to_string(ACTIVATION_PATH)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| {
            json.get("mode")
                .and_then(|mode| mode.as_str())
                .and_then(ActivationMode::parse)
        })
        .unwrap_or(ActivationMode::Shadow)
}

pub(crate) fn autopilot_organ_active() -> bool {
    autopilot_genome_mode() == ActivationMode::Active
}

pub(crate) async fn process_autopilot_board_event(
    state: &AppState,
    event: &BoardEvent,
    legacy_should_notify: bool,
) {
    let envelope = event_from_board(None, event);
    match run_cell(state, envelope).await {
        Ok(outcome) => {
            record_shadow_result(state, "board", &outcome, legacy_should_notify).await;
        }
        Err(error) => {
            report_runtime_error(state, "board", &error).await;
        }
    }
}

pub(crate) async fn process_autopilot_slot_event(
    state: &AppState,
    event: &SlotEvent,
    legacy_should_notify: bool,
) {
    let envelope = event_from_slot(None, event);
    match run_cell(state, envelope).await {
        Ok(outcome) => {
            record_shadow_result(state, "slot", &outcome, legacy_should_notify).await;
        }
        Err(error) => {
            report_runtime_error(state, "slot", &error).await;
        }
    }
}

pub(crate) async fn run_autopilot_tick(state: &AppState) -> Result<()> {
    match autopilot_genome_mode() {
        ActivationMode::Active => {
            let event = system_tick_event(format!("autopilot-tick-{}", Utc::now().timestamp()));
            run_cell(state, event).await.map(|_| ()).map_err(Into::into)
        }
        ActivationMode::Shadow | ActivationMode::Rollback => {
            let event =
                system_tick_event(format!("autopilot-shadow-tick-{}", Utc::now().timestamp()));
            match run_cell(state, event).await {
                Ok(outcome) => record_shadow_result(state, "tick", &outcome, false).await,
                Err(error) => report_runtime_error(state, "tick", &error).await,
            }
            crate::autopilot::autopilot_tick(state).await
        }
    }
}

pub(crate) async fn run_autopilot_board_dispatch(state: &AppState) -> Result<()> {
    match autopilot_genome_mode() {
        ActivationMode::Active => {
            let event = notify_command_event(format!(
                "autopilot-dispatch-{}",
                Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ));
            run_cell(state, event).await.map(|_| ()).map_err(Into::into)
        }
        ActivationMode::Shadow | ActivationMode::Rollback => {
            let event = notify_command_event(format!(
                "autopilot-shadow-dispatch-{}",
                Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ));
            match run_cell(state, event).await {
                Ok(outcome) => record_shadow_result(state, "dispatch", &outcome, false).await,
                Err(error) => report_runtime_error(state, "dispatch", &error).await,
            }
            crate::autopilot::dispatch_board_tasks(state).await
        }
    }
}

async fn run_cell(
    state: &AppState,
    event: missiond_kernel::EventEnvelope,
) -> Result<RuntimeOutcome, RuntimeError> {
    let mode = autopilot_genome_mode();
    record_activation_snapshot(mode).await;
    let interpreter = AutopilotEffectInterpreter {
        state: state.clone(),
    };
    let mut runtime = CellRuntime::new(mode, RuntimeBudgets::default(), interpreter);
    runtime
        .handle_event(AUTOPILOT_GENOME_ID, &AutopilotCell, &event)
        .await
}

struct AutopilotEffectInterpreter {
    state: AppState,
}

#[async_trait]
impl EffectInterpreter for AutopilotEffectInterpreter {
    async fn interpret(&self, effect: &Effect) -> std::result::Result<(), RuntimeError> {
        match effect {
            Effect::Command(command) if command.command_type == CMD_NOTIFY_AUTOPILOT_DISPATCH => {
                self.state.board_dispatch_notify.notify_one();
                Ok(())
            }
            Effect::Command(command) if command.command_type == CMD_RUN_AUTOPILOT_TICK => {
                crate::autopilot::autopilot_tick(&self.state)
                    .await
                    .map_err(|err| RuntimeError::Interpreter(err.to_string()))
            }
            Effect::Command(command) if command.command_type == CMD_RUN_BOARD_DISPATCH => {
                crate::autopilot::dispatch_board_tasks(&self.state)
                    .await
                    .map_err(|err| RuntimeError::Interpreter(err.to_string()))
            }
            Effect::Log(log) => {
                debug!(level = %log.level, message = %log.message, "autopilot organism log effect");
                Ok(())
            }
            Effect::Metric(metric) => {
                debug!(metric = %metric.name, value = metric.value, "autopilot organism metric effect");
                Ok(())
            }
            Effect::Noop => Ok(()),
            other => Err(RuntimeError::Interpreter(format!(
                "unsupported autopilot effect: {other:?}"
            ))),
        }
    }
}

async fn record_shadow_result(
    state: &AppState,
    source: &str,
    outcome: &RuntimeOutcome,
    legacy_should_notify: bool,
) {
    if outcome.mode != ActivationMode::Shadow {
        return;
    }
    let expected_notify = outcome.effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::Command(command) if command.command_type == CMD_NOTIFY_AUTOPILOT_DISPATCH
        )
    });
    let matched = expected_notify == legacy_should_notify;
    let entry = json!({
        "recorded_at": Utc::now().to_rfc3339(),
        "source": source,
        "mode": outcome.mode.as_str(),
        "legacy_should_notify": legacy_should_notify,
        "expected_notify": expected_notify,
        "matched": matched,
        "effect_count": outcome.effects.len(),
        "diagnostics": &outcome.diagnostics,
    });
    if let Err(error) = append_shadow_record(entry).await {
        warn!(error = %error, "failed to record autopilot shadow result");
    }
    if !matched {
        report_runtime_error(
            state,
            "shadow-comparator",
            &RuntimeError::Interpreter(format!(
                "autopilot shadow mismatch: legacy={legacy_should_notify} genome={expected_notify}"
            )),
        )
        .await;
    }
}

async fn append_shadow_record(entry: serde_json::Value) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let path = PathBuf::from(SHADOW_PATH);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut rows = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Vec<serde_json::Value>>(&text).unwrap_or_default(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(anyhow!(error)),
        };
        rows.push(entry);
        std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(&rows)?))?;
        Ok(())
    })
    .await
    .map_err(|err| anyhow!(err))?
}

async fn record_activation_snapshot(mode: ActivationMode) {
    let entry = json!({
        "schema_version": "missiond.autopilot-activation.v1",
        "organ": "autopilot",
        "mode": mode.as_str(),
        "updated_at": Utc::now().to_rfc3339(),
        "rollback_behavior": "legacy-direct-path",
        "snapshot_path": ACTIVATION_PATH,
    });
    if let Err(error) = write_json_file(ACTIVATION_PATH, entry).await {
        warn!(error = %error, "failed to record autopilot activation snapshot");
    }
}

async fn write_json_file(path: &'static str, payload: serde_json::Value) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            path,
            format!("{}\n", serde_json::to_string_pretty(&payload)?),
        )?;
        Ok(())
    })
    .await
    .map_err(|err| anyhow!(err))?
}

async fn report_runtime_error(state: &AppState, source: &str, error: &RuntimeError) {
    warn!(source = %source, error = %error, "autopilot organism runtime fell back to legacy path");
    let incident = MissionIncident {
        id: format!(
            "autopilot-organ-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ),
        severity: IncidentSeverity::Warning,
        source: IncidentSource::HealthCheck,
        title: "Autopilot organism fallback".to_string(),
        description: format!("{source}: {error}"),
        server_id: None,
        raw_payload: json!({
            "source": source,
            "error": error.to_string(),
            "activation": autopilot_genome_mode().as_str(),
        }),
        created_at: Utc::now().to_rfc3339(),
    };
    if let Err(publish_error) = state
        .bus
        .publish_incident(IncidentEvent::Reported { incident })
        .await
    {
        warn!(error = %publish_error, "failed to publish autopilot organism incident");
    }
}
