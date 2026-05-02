//! XJPCode Briefing Worker — periodically writes ~/.xjpcode/xjpcode.md.

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::state::AppState;

use super::{BackgroundWorker, WorkerContext, WorkerKind};

pub(crate) struct XjpcodeBriefingWorker;

impl BackgroundWorker for XjpcodeBriefingWorker {
    const KIND: WorkerKind = WorkerKind::Local;

    fn name(&self) -> &'static str {
        "xjpcode_briefing"
    }

    async fn run(self, state: Arc<AppState>, mut ctx: WorkerContext) {
        tokio::time::sleep(Duration::from_secs(15)).await;
        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            ctx.wait_if_paused().await;
            match generate_and_write(&state).await {
                Ok(_) => ctx.record_success(),
                Err(e) => {
                    warn!(error = %e, "briefing failed");
                    ctx.record_failure();
                }
            }
        }
    }
}

async fn generate_and_write(state: &AppState) -> anyhow::Result<()> {
    let running = state
        .store
        .list_board_tasks(Some("running"), false)
        .await
        .unwrap_or_default();
    let failed = state
        .store
        .list_board_tasks(Some("failed"), false)
        .await
        .unwrap_or_default();
    let incidents = state.store.list_incidents(5).await.unwrap_or_default();
    let projects = state.store.list_projects().await.unwrap_or_default();

    let markdown = render_markdown(&running, &failed, &incidents, &projects);

    let path = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("no home dir"))?
        .join(".xjpcode/xjpcode.md");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = markdown.len();
    std::fs::write(&path, markdown)?;
    info!(path = %path.display(), bytes, "xjpcode briefing written");
    Ok(())
}

fn render_markdown(
    running: &[missiond_core::types::BoardTask],
    failed: &[missiond_core::types::BoardTask],
    incidents: &[missiond_core::types::IncidentRow],
    projects: &[missiond_core::types::ProjectConfig],
) -> String {
    let mut out = String::new();
    out.push_str("# XJPCode Briefing\n\n");
    out.push_str(&format!(
        "_Generated: {}_\n\n",
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    ));

    out.push_str("## Active Board Tasks\n\n");

    let mut running_sorted: Vec<&missiond_core::types::BoardTask> = running.iter().collect();
    running_sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out.push_str(&format!("**Running ({}):**\n", running_sorted.len()));
    if running_sorted.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for t in running_sorted.iter().take(5) {
            let id_str = t.id.as_str();
            let short = &id_str[..8.min(id_str.len())];
            let phase = t.flow_phase.as_deref().unwrap_or("-");
            out.push_str(&format!(
                "- [{}] {} — {} / {}\n",
                short, t.title, t.priority, phase
            ));
        }
    }
    out.push('\n');

    let mut failed_sorted: Vec<&missiond_core::types::BoardTask> = failed.iter().collect();
    failed_sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out.push_str(&format!("**Recently Failed ({}):**\n", failed_sorted.len()));
    if failed_sorted.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for t in failed_sorted.iter().take(3) {
            let id_str = t.id.as_str();
            let short = &id_str[..8.min(id_str.len())];
            out.push_str(&format!(
                "- [{}] {} — updated {}\n",
                short, t.title, t.updated_at
            ));
        }
    }
    out.push('\n');

    out.push_str(&format!("## Recent Incidents ({})\n\n", incidents.len()));
    if incidents.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for i in incidents.iter().take(5) {
            out.push_str(&format!(
                "- [{}] {} — {}\n",
                i.severity, i.title, i.created_at
            ));
        }
    }
    out.push('\n');

    out.push_str(&format!("## Active Projects ({})\n\n", projects.len()));
    if projects.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for p in projects.iter().take(10) {
            let intent_marker = if p.intent_path.is_some() {
                " (+intent)"
            } else {
                ""
            };
            out.push_str(&format!("- `{}` — {}{}\n", p.id, p.path, intent_marker));
        }
    }
    out.push('\n');

    out.push_str("---\n\n");
    out.push_str("Source: missiond xjpcode_briefing_worker. Update interval: 60s.\n");
    out
}
