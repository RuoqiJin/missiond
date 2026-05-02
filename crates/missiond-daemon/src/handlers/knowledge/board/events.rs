use super::*;

/// Publish BoardTaskCreated event.
pub(super) fn publish_board_created(state: &AppState, task: &missiond_core::types::BoardTask) {
    let ev = BoardEvent::TaskCreated {
        task_id: task.id.to_string(),
        title: task.title.clone(),
        category: task.category.clone(),
    };
    crate::engine::master_control::notify_board_event_direct(&ev);
    let bus = Arc::clone(&state.bus);
    tokio::spawn(async move {
        let _ = bus.publish_board(ev).await;
    });
}

/// Publish BoardTaskUpdated event (generic field update).
pub(super) fn publish_board_update(state: &AppState, task: &missiond_core::types::BoardTask) {
    let ev = BoardEvent::Updated {
        task_id: task.id.to_string(),
        status: format!("{:?}", task.status),
        category: task.category.clone(),
    };
    crate::engine::master_control::notify_board_event_direct(&ev);
    let bus = Arc::clone(&state.bus);
    tokio::spawn(async move {
        let _ = bus.publish_board(ev).await;
    });
}

/// Publish BoardTaskStatusChanged event.
pub(super) fn publish_board_status_changed(
    state: &AppState,
    task: &missiond_core::types::BoardTask,
    old_status: &str,
) {
    let ev = BoardEvent::StatusChanged {
        task_id: task.id.to_string(),
        old_status: old_status.to_string(),
        new_status: format!("{:?}", task.status),
    };
    crate::engine::master_control::notify_board_event_direct(&ev);
    let bus = Arc::clone(&state.bus);
    tokio::spawn(async move {
        let _ = bus.publish_board(ev).await;
    });
}
