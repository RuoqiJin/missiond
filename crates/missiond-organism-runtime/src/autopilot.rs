use async_trait::async_trait;
use missiond_core::event::{BoardEvent, SlotEvent};
use missiond_kernel::{ActivationMode, Cell, CellCtx, CommandEnvelope, Effect, EventEnvelope};
use serde_json::{json, Value};

pub const AUTOPILOT_GENOME_ID: &str = "missiond-autopilot";
pub const AUTOPILOT_ORGAN_ID: &str = "autopilot";

pub const CMD_NOTIFY_AUTOPILOT_DISPATCH: &str = "NotifyAutopilotDispatch";
pub const CMD_RUN_AUTOPILOT_TICK: &str = "RunAutopilotTick";
pub const CMD_RUN_BOARD_DISPATCH: &str = "RunBoardDispatch";

pub const BOARD_TASK_CREATED: &str = "BoardEvent::TaskCreated";
pub const BOARD_UPDATED_OPEN: &str = "BoardEvent::Updated(status=open)";
pub const BOARD_STATUS_CHANGED_OPEN: &str = "BoardEvent::StatusChanged(new_status=open)";
pub const SLOT_BECAME_IDLE: &str = "SlotEvent::BecameIdle";
pub const SYSTEM_TICK_AUTOPILOT_60S: &str = "SystemTick::Autopilot60s";
pub const COMMAND_NOTIFY_AUTOPILOT_DISPATCH: &str = "Command::NotifyAutopilotDispatch";

pub const AUTOPILOT_RECEPTORS: &[&str] = &[
    BOARD_TASK_CREATED,
    BOARD_UPDATED_OPEN,
    BOARD_STATUS_CHANGED_OPEN,
    SLOT_BECAME_IDLE,
    SYSTEM_TICK_AUTOPILOT_60S,
    COMMAND_NOTIFY_AUTOPILOT_DISPATCH,
];

#[derive(Debug, Clone, Default)]
pub struct AutopilotCell;

impl AutopilotCell {
    pub fn is_wakeup_effect(effect: &Effect) -> bool {
        matches!(
            effect,
            Effect::Command(command) if command.command_type == CMD_NOTIFY_AUTOPILOT_DISPATCH
        )
    }

    fn notify_command(event: &EventEnvelope) -> Effect {
        Effect::Command(
            CommandEnvelope::new(
                CMD_NOTIFY_AUTOPILOT_DISPATCH,
                json!({
                    "event_id": event.id,
                    "event_kind": event.kind,
                }),
            )
            .with_idempotency_key(format!("autopilot-notify:{}", event.id))
            .with_capability("autopilot.dispatch.notify"),
        )
    }

    fn tick_command(event: &EventEnvelope) -> Effect {
        Effect::Command(
            CommandEnvelope::new(
                CMD_RUN_AUTOPILOT_TICK,
                json!({
                    "event_id": event.id,
                    "event_kind": event.kind,
                }),
            )
            .with_idempotency_key(format!("autopilot-tick:{}", event.id))
            .with_capability("autopilot.tick"),
        )
    }

    fn dispatch_command(event: &EventEnvelope) -> Effect {
        Effect::Command(
            CommandEnvelope::new(
                CMD_RUN_BOARD_DISPATCH,
                json!({
                    "event_id": event.id,
                    "event_kind": event.kind,
                }),
            )
            .with_idempotency_key(format!("autopilot-dispatch:{}", event.id))
            .with_capability("autopilot.board.dispatch"),
        )
    }
}

#[async_trait]
impl Cell for AutopilotCell {
    fn id(&self) -> &'static str {
        "autopilot-cell"
    }

    fn tissue(&self) -> &'static str {
        "autopilot"
    }

    fn receptors(&self) -> &'static [&'static str] {
        AUTOPILOT_RECEPTORS
    }

    async fn on_event(&self, _ctx: &CellCtx, event: &EventEnvelope) -> Vec<Effect> {
        match event.kind.as_str() {
            BOARD_TASK_CREATED
            | BOARD_UPDATED_OPEN
            | BOARD_STATUS_CHANGED_OPEN
            | SLOT_BECAME_IDLE => {
                vec![Self::notify_command(event)]
            }
            SYSTEM_TICK_AUTOPILOT_60S => vec![Self::tick_command(event)],
            COMMAND_NOTIFY_AUTOPILOT_DISPATCH => vec![Self::dispatch_command(event)],
            _ => vec![Effect::Noop],
        }
    }
}

pub fn event_from_board(seq: Option<i64>, event: &BoardEvent) -> EventEnvelope {
    let kind = match event {
        BoardEvent::TaskCreated { .. } => BOARD_TASK_CREATED,
        BoardEvent::Updated { status, .. } if status.eq_ignore_ascii_case("open") => {
            BOARD_UPDATED_OPEN
        }
        BoardEvent::StatusChanged { new_status, .. } if new_status.eq_ignore_ascii_case("open") => {
            BOARD_STATUS_CHANGED_OPEN
        }
        BoardEvent::Updated { .. } => "BoardEvent::Updated",
        BoardEvent::StatusChanged { .. } => "BoardEvent::StatusChanged",
        BoardEvent::NoteAdded { .. } => "BoardEvent::NoteAdded",
        BoardEvent::Claimed { .. } => "BoardEvent::Claimed",
        BoardEvent::Deleted { .. } => "BoardEvent::Deleted",
    };
    envelope(
        kind,
        Some("board"),
        Some("missiond.event.bus"),
        seq,
        serde_json::to_value(event).unwrap_or(Value::Null),
    )
}

pub fn event_from_slot(seq: Option<i64>, event: &SlotEvent) -> EventEnvelope {
    let kind = match event {
        SlotEvent::BecameIdle { .. } => SLOT_BECAME_IDLE,
        SlotEvent::StateChanged { .. } => "SlotEvent::StateChanged",
        SlotEvent::TaskDispatched { .. } => "SlotEvent::TaskDispatched",
        SlotEvent::Stuck { .. } => "SlotEvent::Stuck",
    };
    envelope(
        kind,
        Some("slot"),
        Some("missiond.event.bus"),
        seq,
        serde_json::to_value(event).unwrap_or(Value::Null),
    )
}

pub fn system_tick_event(tick_id: impl Into<String>) -> EventEnvelope {
    let tick_id = tick_id.into();
    let mut event = envelope(
        SYSTEM_TICK_AUTOPILOT_60S,
        Some("system"),
        Some("missiond.autopilot.scheduler"),
        None,
        json!({ "tick_id": tick_id }),
    );
    event.id = tick_id;
    event
}

pub fn notify_command_event(command_id: impl Into<String>) -> EventEnvelope {
    let command_id = command_id.into();
    let mut event = envelope(
        COMMAND_NOTIFY_AUTOPILOT_DISPATCH,
        Some("autopilot"),
        Some("missiond.autopilot.organ"),
        None,
        json!({ "command_id": command_id }),
    );
    event.id = command_id;
    event
}

fn envelope(
    kind: impl Into<String>,
    domain: Option<&str>,
    source: Option<&str>,
    seq: Option<i64>,
    payload: Value,
) -> EventEnvelope {
    let mut event = EventEnvelope::new(kind, payload);
    event.domain = domain.map(str::to_string);
    event.source = source.map(str::to_string);
    if let Some(seq) = seq {
        event.meta.insert("event_seq".to_string(), seq.to_string());
    }
    event
}

pub fn activation_from_env() -> ActivationMode {
    std::env::var("MISSIOND_GENOME_AUTOPILOT_MODE")
        .ok()
        .and_then(|value| ActivationMode::parse(value.trim()))
        .unwrap_or(ActivationMode::Shadow)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn effects_for(event: EventEnvelope) -> Vec<Effect> {
        AutopilotCell
            .on_event(
                &CellCtx {
                    genome_id: AUTOPILOT_GENOME_ID.to_string(),
                    activation: ActivationMode::Shadow,
                },
                &event,
            )
            .await
    }

    #[tokio::test]
    async fn board_wakeup_events_emit_notify_command() {
        let event = event_from_board(
            None,
            &BoardEvent::TaskCreated {
                task_id: "t".to_string(),
                title: "x".to_string(),
                category: "dev".to_string(),
            },
        );
        let effects = effects_for(event).await;
        assert!(AutopilotCell::is_wakeup_effect(&effects[0]));
    }

    #[tokio::test]
    async fn closed_board_update_is_noop() {
        let event = event_from_board(
            None,
            &BoardEvent::Updated {
                task_id: "t".to_string(),
                status: "done".to_string(),
                category: "dev".to_string(),
            },
        );
        let effects = effects_for(event).await;
        assert_eq!(effects, vec![Effect::Noop]);
    }

    #[tokio::test]
    async fn slot_idle_emits_notify_command() {
        let event = event_from_slot(
            None,
            &SlotEvent::BecameIdle {
                slot_id: "slot-a".to_string(),
            },
        );
        let effects = effects_for(event).await;
        assert!(AutopilotCell::is_wakeup_effect(&effects[0]));
    }

    #[tokio::test]
    async fn tick_emits_autopilot_tick_command() {
        let effects = effects_for(system_tick_event("tick-1")).await;
        assert!(matches!(
            &effects[0],
            Effect::Command(command) if command.command_type == CMD_RUN_AUTOPILOT_TICK
        ));
    }

    #[tokio::test]
    async fn notify_command_event_emits_dispatch_command() {
        let effects = effects_for(notify_command_event("notify-1")).await;
        assert!(matches!(
            &effects[0],
            Effect::Command(command) if command.command_type == CMD_RUN_BOARD_DISPATCH
        ));
    }
}
