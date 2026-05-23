use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use missiond_core::event::events::SystemEvent;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::{PgPool, Row};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::bus::BusServices;

pub const REVIEW_PROMPT: &str =
    "看一下 missonD 的 SSOT lisp,如果让你对这个程序 在架构层面进行优化、改进，你会选择改进哪里？为什么？";
pub const PLAN_PROMPT: &str = "调查并设计执行方案。目标是这些问题全部解决";
pub const IMPLEMENT_PREFIX: &str = "PLEASE IMPLEMENT THIS PLAN:\n";

const DEFAULT_NAME: &str = "Codex 三步复刻";
const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 200;
const MAX_STDIO_LINE_CHARS: usize = 32_000;
const TURN_TIMEOUT_SECS: u64 = 4 * 60 * 60;

type CodexIoSink = Arc<dyn Fn(&'static str, String) + Send + Sync + 'static>;

#[derive(Clone)]
pub(crate) struct CodexReplayService {
    pool: PgPool,
    bus: Arc<BusServices>,
    active: Arc<Mutex<HashMap<String, CampaignRuntime>>>,
    codex_bin: String,
}

struct CampaignRuntime {
    cancel: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    handle: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Clone)]
struct CampaignConfig {
    id: String,
    project_root: String,
    max_cycles: Option<i32>,
    interval_seconds: i32,
}

#[derive(Debug)]
struct ThreadContext {
    thread_id: String,
    model: String,
    reasoning_effort: Option<String>,
}

#[derive(Debug, Default)]
struct TurnOutcome {
    text: String,
    selected_options: Vec<Value>,
}

#[derive(Debug)]
enum CycleError {
    Failed(String),
    Blocked(String),
}

impl CycleError {
    fn status(&self) -> &'static str {
        match self {
            Self::Failed(_) => "failed",
            Self::Blocked(_) => "blocked",
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Failed(message) | Self::Blocked(message) => message,
        }
    }
}

impl From<anyhow::Error> for CycleError {
    fn from(value: anyhow::Error) -> Self {
        Self::Failed(format!("{value:#}"))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionArgs {
    action: String,
    campaign_id: Option<String>,
    run_id: Option<String>,
    project_root: Option<String>,
    max_cycles: Option<i32>,
    interval_seconds: Option<i32>,
    limit: Option<i64>,
}

impl CodexReplayService {
    pub(crate) fn new(pool: PgPool, bus: Arc<BusServices>) -> Self {
        Self {
            pool,
            bus,
            active: Arc::new(Mutex::new(HashMap::new())),
            codex_bin: default_codex_bin(),
        }
    }

    pub(crate) async fn handle_action(&self, args: Value) -> Result<Value> {
        let args: ActionArgs = serde_json::from_value(args)?;
        match args.action.as_str() {
            "start_campaign" => self.start_campaign(args).await,
            "run_once" => self.run_once(args).await,
            "pause_campaign" => self.pause_campaign(args).await,
            "resume_campaign" => self.resume_campaign(args).await,
            "stop_campaign" => self.stop_campaign(args).await,
            "status" => self.status(args).await,
            "list_runs" => self.list_runs(args).await,
            "get_run" => self.get_run(args).await,
            other => Err(anyhow!("unknown codex replay action: {other}")),
        }
    }

    async fn start_campaign(&self, args: ActionArgs) -> Result<Value> {
        let id = args
            .campaign_id
            .unwrap_or_else(|| format!("codex-replay-{}", Uuid::new_v4()));
        let project_root = args.project_root.unwrap_or_else(|| {
            current_dir_string().unwrap_or_else(|| "/Users/jinchen/Projects/missiond".to_string())
        });
        let interval_seconds = args.interval_seconds.unwrap_or(0).clamp(0, 86_400);
        let max_cycles = args.max_cycles.map(|v| v.max(1));

        sqlx::query(
            r#"
            INSERT INTO codex_replay_campaigns
              (id, name, project_root, status, current_phase, max_cycles, interval_seconds)
            VALUES ($1, $2, $3, 'running', 'queued', $4, $5)
            ON CONFLICT(id) DO UPDATE
            SET status = 'running',
                current_phase = 'queued',
                project_root = EXCLUDED.project_root,
                max_cycles = EXCLUDED.max_cycles,
                interval_seconds = EXCLUDED.interval_seconds,
                last_error = NULL,
                completed_at = NULL,
                updated_at = now()
            "#,
        )
        .bind(&id)
        .bind(DEFAULT_NAME)
        .bind(&project_root)
        .bind(max_cycles)
        .bind(interval_seconds)
        .execute(&self.pool)
        .await?;

        self.spawn_campaign(CampaignConfig {
            id: id.clone(),
            project_root,
            max_cycles,
            interval_seconds,
        })
        .await?;

        self.status_for_campaign(Some(id), DEFAULT_LIMIT).await
    }

    async fn run_once(&self, mut args: ActionArgs) -> Result<Value> {
        args.max_cycles = Some(1);
        self.start_campaign(args).await
    }

    async fn pause_campaign(&self, args: ActionArgs) -> Result<Value> {
        let campaign_id = self.resolve_campaign_id(args.campaign_id).await?;
        if let Some(runtime) = self.active.lock().await.get(&campaign_id) {
            runtime.paused.store(true, Ordering::Release);
        }
        self.set_campaign_status(&campaign_id, "paused", "paused", None)
            .await?;
        self.record_event(
            &campaign_id,
            None,
            None,
            "paused",
            "codex_replay_campaign_paused",
            "Campaign paused by operator.",
            json!({}),
        )
        .await;
        self.status_for_campaign(Some(campaign_id), DEFAULT_LIMIT)
            .await
    }

    async fn resume_campaign(&self, args: ActionArgs) -> Result<Value> {
        let campaign_id = self.resolve_campaign_id(args.campaign_id).await?;
        if let Some(runtime) = self.active.lock().await.get(&campaign_id) {
            runtime.paused.store(false, Ordering::Release);
        } else {
            let cfg = self.load_campaign_config(&campaign_id).await?;
            self.spawn_campaign(cfg).await?;
        }
        self.set_campaign_status(&campaign_id, "running", "queued", None)
            .await?;
        self.record_event(
            &campaign_id,
            None,
            None,
            "queued",
            "codex_replay_campaign_resumed",
            "Campaign resumed by operator.",
            json!({}),
        )
        .await;
        self.status_for_campaign(Some(campaign_id), DEFAULT_LIMIT)
            .await
    }

    async fn stop_campaign(&self, args: ActionArgs) -> Result<Value> {
        let campaign_id = self.resolve_campaign_id(args.campaign_id).await?;
        if let Some(runtime) = self.active.lock().await.remove(&campaign_id) {
            runtime.cancel.store(true, Ordering::Release);
            runtime.handle.abort();
        }
        self.set_campaign_status(&campaign_id, "stopped", "stopped", None)
            .await?;
        self.record_event(
            &campaign_id,
            None,
            None,
            "stopped",
            "codex_replay_campaign_stopped",
            "Campaign stopped by operator.",
            json!({}),
        )
        .await;
        self.status_for_campaign(Some(campaign_id), DEFAULT_LIMIT)
            .await
    }

    async fn status(&self, args: ActionArgs) -> Result<Value> {
        self.status_for_campaign(args.campaign_id, bounded_limit(args.limit))
            .await
    }

    async fn list_runs(&self, args: ActionArgs) -> Result<Value> {
        let campaign_id = self.resolve_campaign_id(args.campaign_id).await?;
        let limit = bounded_limit(args.limit);
        let runs = self.query_runs(Some(&campaign_id), limit).await?;
        Ok(json!({
            "schema": "missiond.codex-replay.runs.v1",
            "campaignId": campaign_id,
            "runs": runs
        }))
    }

    async fn get_run(&self, args: ActionArgs) -> Result<Value> {
        let run_id = args.run_id.ok_or_else(|| anyhow!("runId is required"))?;
        let run = sqlx::query_scalar::<_, Option<Value>>(
            r#"
            SELECT to_jsonb(r)
            FROM (
              SELECT *
              FROM codex_replay_runs
              WHERE id = $1
            ) r
            "#,
        )
        .bind(&run_id)
        .fetch_one(&self.pool)
        .await?;
        let events = self
            .query_events(None, Some(&run_id), bounded_limit(args.limit))
            .await?;
        Ok(json!({
            "schema": "missiond.codex-replay.run.v1",
            "run": run,
            "events": events
        }))
    }

    async fn spawn_campaign(&self, cfg: CampaignConfig) -> Result<()> {
        let mut active = self.active.lock().await;
        if active.contains_key(&cfg.id) {
            return Ok(());
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let service = self.clone();
        let run_cfg = cfg.clone();
        let cancel_for_task = Arc::clone(&cancel);
        let paused_for_task = Arc::clone(&paused);
        let handle = tokio::spawn(async move {
            service
                .run_campaign(run_cfg, cancel_for_task, paused_for_task)
                .await;
        });
        active.insert(
            cfg.id,
            CampaignRuntime {
                cancel,
                paused,
                handle,
            },
        );
        Ok(())
    }

    async fn run_campaign(
        self,
        cfg: CampaignConfig,
        cancel: Arc<AtomicBool>,
        paused: Arc<AtomicBool>,
    ) {
        let mut completed_this_process = 0_i32;
        loop {
            if cancel.load(Ordering::Acquire) {
                break;
            }

            while paused.load(Ordering::Acquire) && !cancel.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            if cancel.load(Ordering::Acquire) {
                break;
            }

            if let Some(max) = cfg.max_cycles {
                if completed_this_process >= max {
                    let _ = self
                        .set_campaign_status(&cfg.id, "completed", "completed", None)
                        .await;
                    break;
                }
            }

            let cycle_no = match self.next_cycle_no(&cfg.id).await {
                Ok(cycle_no) => cycle_no,
                Err(err) => {
                    let _ = self
                        .set_campaign_status(
                            &cfg.id,
                            "failed",
                            "cycle_allocate_failed",
                            Some(&err.to_string()),
                        )
                        .await;
                    break;
                }
            };

            let run_id = format!("codex-replay-run-{}", Uuid::new_v4());
            if let Err(err) = self.insert_run(&cfg, &run_id, cycle_no).await {
                let _ = self
                    .set_campaign_status(
                        &cfg.id,
                        "failed",
                        "run_insert_failed",
                        Some(&err.to_string()),
                    )
                    .await;
                break;
            }

            let outcome = self.run_cycle(&cfg, &run_id, cycle_no).await;
            match outcome {
                Ok(()) => {
                    completed_this_process += 1;
                    let _ = sqlx::query(
                        r#"
                        UPDATE codex_replay_campaigns
                        SET completed_cycles = completed_cycles + 1,
                            current_phase = 'cycle_completed',
                            updated_at = now()
                        WHERE id = $1
                        "#,
                    )
                    .bind(&cfg.id)
                    .execute(&self.pool)
                    .await;
                }
                Err(err) => {
                    let phase = err.status();
                    let message = err.message().to_string();
                    let _ = self
                        .set_run_terminal(&run_id, phase, phase, Some(&message), None)
                        .await;
                    let _ = self
                        .set_campaign_status(&cfg.id, phase, phase, Some(&message))
                        .await;
                    self.record_event(
                        &cfg.id,
                        Some(&run_id),
                        Some(cycle_no),
                        phase,
                        &format!("codex_replay_run_{phase}"),
                        &message,
                        json!({ "status": phase }),
                    )
                    .await;
                    break;
                }
            }

            if let Some(max) = cfg.max_cycles {
                if completed_this_process >= max {
                    let _ = self
                        .set_campaign_status(&cfg.id, "completed", "completed", None)
                        .await;
                    break;
                }
            }

            if cfg.interval_seconds > 0 {
                tokio::time::sleep(Duration::from_secs(cfg.interval_seconds as u64)).await;
            }
        }

        self.active.lock().await.remove(&cfg.id);
    }

    async fn run_cycle(
        &self,
        cfg: &CampaignConfig,
        run_id: &str,
        cycle_no: i32,
    ) -> Result<(), CycleError> {
        self.record_event(
            &cfg.id,
            Some(run_id),
            Some(cycle_no),
            "starting_thread",
            "codex_replay_run_started",
            "Starting Codex app-server replay cycle.",
            json!({ "projectRoot": cfg.project_root, "codexBin": self.codex_bin }),
        )
        .await;
        self.set_run_phase(run_id, "running", "starting_thread", None)
            .await?;

        let mut client = CodexAppServerClient::connect(
            &self.codex_bin,
            Some(self.codex_io_sink(&cfg.id, run_id, cycle_no)),
        )
        .await?;
        let thread = client.start_thread(&cfg.project_root).await?;
        self.update_run_thread(run_id, &thread).await?;

        self.set_run_phase(run_id, "running", "review_turn_default", None)
            .await?;
        let review_turn_id = client
            .start_turn(&thread, "default", REVIEW_PROMPT, &cfg.project_root)
            .await?;
        self.update_turn_id(run_id, "review_turn_id", &review_turn_id)
            .await?;
        self.wait_turn_with_timeout(
            &mut client,
            cfg,
            run_id,
            cycle_no,
            &review_turn_id,
            "review_turn_default",
            false,
        )
        .await?;

        self.set_run_phase(run_id, "running", "plan_turn_plan_mode", None)
            .await?;
        let plan_turn_id = client
            .start_turn(&thread, "plan", PLAN_PROMPT, &cfg.project_root)
            .await?;
        self.update_turn_id(run_id, "plan_turn_id", &plan_turn_id)
            .await?;
        let plan_outcome = self
            .wait_turn_with_timeout(
                &mut client,
                cfg,
                run_id,
                cycle_no,
                &plan_turn_id,
                "plan_turn_plan_mode",
                true,
            )
            .await?;
        self.update_selected_options(run_id, &plan_outcome.selected_options)
            .await?;

        let plan_text = extract_proposed_plan(&plan_outcome.text).ok_or_else(|| {
            CycleError::Blocked(
                "plan turn completed without a valid <proposed_plan> block".to_string(),
            )
        })?;
        self.update_plan_text(run_id, &plan_text).await?;
        self.record_event(
            &cfg.id,
            Some(run_id),
            Some(cycle_no),
            "awaiting_plan",
            "codex_replay_plan_captured",
            "Captured proposed plan from Plan Mode output.",
            json!({ "planChars": plan_text.chars().count() }),
        )
        .await;

        self.set_run_phase(run_id, "running", "implement_turn_default", None)
            .await?;
        let implement_prompt = format!("{IMPLEMENT_PREFIX}{plan_text}");
        let implement_turn_id = client
            .start_turn(&thread, "default", &implement_prompt, &cfg.project_root)
            .await?;
        self.update_turn_id(run_id, "implement_turn_id", &implement_turn_id)
            .await?;
        let implement_outcome = self
            .wait_turn_with_timeout(
                &mut client,
                cfg,
                run_id,
                cycle_no,
                &implement_turn_id,
                "implement_turn_default",
                false,
            )
            .await?;

        self.set_run_terminal(
            run_id,
            "completed",
            "completed",
            None,
            Some(&truncate_chars(&implement_outcome.text, 24_000)),
        )
        .await?;
        self.record_event(
            &cfg.id,
            Some(run_id),
            Some(cycle_no),
            "completed",
            "codex_replay_run_completed",
            "Codex replay cycle completed.",
            json!({ "threadId": thread.thread_id }),
        )
        .await;
        Ok(())
    }

    async fn wait_turn_with_timeout(
        &self,
        client: &mut CodexAppServerClient,
        cfg: &CampaignConfig,
        run_id: &str,
        cycle_no: i32,
        turn_id: &str,
        phase: &str,
        allow_recommended_input: bool,
    ) -> Result<TurnOutcome, CycleError> {
        let fut = client.wait_for_turn(
            turn_id,
            allow_recommended_input,
            |event_kind, message, payload| {
                let service = self.clone();
                let campaign_id = cfg.id.clone();
                let run_id = run_id.to_string();
                let phase = phase.to_string();
                let event_kind = event_kind.to_string();
                let message = message.to_string();
                let payload = payload.clone();
                tokio::spawn(async move {
                    service
                        .record_event(
                            &campaign_id,
                            Some(&run_id),
                            Some(cycle_no),
                            &phase,
                            &event_kind,
                            &message,
                            payload,
                        )
                        .await;
                });
            },
        );
        match tokio::time::timeout(Duration::from_secs(TURN_TIMEOUT_SECS), fut).await {
            Ok(Ok(outcome)) => Ok(outcome),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(CycleError::Failed(format!(
                "turn {turn_id} timed out after {TURN_TIMEOUT_SECS}s"
            ))),
        }
    }

    fn codex_io_sink(&self, campaign_id: &str, run_id: &str, cycle_no: i32) -> CodexIoSink {
        let service = self.clone();
        let campaign_id = campaign_id.to_string();
        let run_id = run_id.to_string();
        Arc::new(move |direction, line| {
            let service = service.clone();
            let campaign_id = campaign_id.clone();
            let run_id = run_id.clone();
            let payload = stdio_event_payload(direction, &line);
            let message = summarize_stdio_line(direction, &line);
            let event_kind = match direction {
                "stdin" => "codex_replay_pty_input",
                "stderr" => "codex_replay_pty_stderr",
                _ => "codex_replay_pty_output",
            }
            .to_string();
            tokio::spawn(async move {
                service
                    .record_event(
                        &campaign_id,
                        Some(&run_id),
                        Some(cycle_no),
                        "app_server_stdio",
                        &event_kind,
                        &message,
                        payload,
                    )
                    .await;
            });
        })
    }

    async fn insert_run(&self, cfg: &CampaignConfig, run_id: &str, cycle_no: i32) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO codex_replay_runs
              (id, campaign_id, cycle_no, project_root, status, phase, started_at)
            VALUES ($1, $2, $3, $4, 'running', 'queued', now())
            "#,
        )
        .bind(run_id)
        .bind(&cfg.id)
        .bind(cycle_no)
        .bind(&cfg.project_root)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            UPDATE codex_replay_campaigns
            SET last_run_id = $2,
                current_phase = 'queued',
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(&cfg.id)
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_run_thread(&self, run_id: &str, thread: &ThreadContext) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE codex_replay_runs
            SET thread_id = $2,
                model = $3,
                reasoning_effort = $4,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(&thread.thread_id)
        .bind(&thread.model)
        .bind(&thread.reasoning_effort)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_turn_id(&self, run_id: &str, column: &str, turn_id: &str) -> Result<()> {
        let sql = match column {
            "review_turn_id" => {
                "UPDATE codex_replay_runs SET review_turn_id = $2, updated_at = now() WHERE id = $1"
            }
            "plan_turn_id" => {
                "UPDATE codex_replay_runs SET plan_turn_id = $2, updated_at = now() WHERE id = $1"
            }
            "implement_turn_id" => {
                "UPDATE codex_replay_runs SET implement_turn_id = $2, updated_at = now() WHERE id = $1"
            }
            _ => return Err(anyhow!("invalid turn id column: {column}")),
        };
        sqlx::query(sql)
            .bind(run_id)
            .bind(turn_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_run_phase(
        &self,
        run_id: &str,
        status: &str,
        phase: &str,
        error: Option<&str>,
    ) -> Result<()> {
        let campaign_id = self.campaign_id_for_run(run_id).await?;
        sqlx::query(
            r#"
            UPDATE codex_replay_runs
            SET status = $2,
                phase = $3,
                last_error = $4,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(status)
        .bind(phase)
        .bind(error)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            UPDATE codex_replay_campaigns
            SET current_phase = $2,
                status = CASE WHEN status = 'paused' THEN status ELSE 'running' END,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(&campaign_id)
        .bind(phase)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_run_terminal(
        &self,
        run_id: &str,
        status: &str,
        phase: &str,
        message: Option<&str>,
        final_message: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE codex_replay_runs
            SET status = $2,
                phase = $3,
                blocked_reason = CASE WHEN $2 = 'blocked' THEN $4 ELSE blocked_reason END,
                last_error = CASE WHEN $2 = 'failed' THEN $4 ELSE last_error END,
                final_message = COALESCE($5, final_message),
                completed_at = now(),
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(status)
        .bind(phase)
        .bind(message)
        .bind(final_message)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_selected_options(&self, run_id: &str, selected: &[Value]) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE codex_replay_runs
            SET selected_options_json = $2,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(json!(selected))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_plan_text(&self, run_id: &str, plan_text: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE codex_replay_runs
            SET plan_text = $2,
                phase = 'awaiting_plan',
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(run_id)
        .bind(plan_text)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_campaign_status(
        &self,
        campaign_id: &str,
        status: &str,
        phase: &str,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE codex_replay_campaigns
            SET status = $2,
                current_phase = $3,
                last_error = $4,
                completed_at = CASE WHEN $2 IN ('completed','failed','blocked','stopped') THEN now() ELSE completed_at END,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(campaign_id)
        .bind(status)
        .bind(phase)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_event(
        &self,
        campaign_id: &str,
        run_id: Option<&str>,
        cycle_no: Option<i32>,
        phase: &str,
        event_kind: &str,
        message: &str,
        payload: Value,
    ) {
        let _ = sqlx::query(
            r#"
            INSERT INTO codex_replay_events
              (campaign_id, run_id, cycle_no, phase, event_kind, message, payload)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(campaign_id)
        .bind(run_id)
        .bind(cycle_no)
        .bind(phase)
        .bind(event_kind)
        .bind(message)
        .bind(&payload)
        .execute(&self.pool)
        .await;

        let payload_json = serde_json::to_string(&json!({
            "campaignId": campaign_id,
            "runId": run_id,
            "cycleNo": cycle_no,
            "phase": phase,
            "eventKind": event_kind,
            "payload": payload
        }))
        .unwrap_or_else(|_| "{}".to_string());
        let _ = self
            .bus
            .publish_system(SystemEvent::ExternalServiceEvent {
                service_id: "missiond.codex_replay".to_string(),
                event_id: format!(
                    "{campaign_id}:{event_kind}:{}",
                    chrono::Utc::now().timestamp_millis()
                ),
                event_kind: event_kind.to_string(),
                summary: message.to_string(),
                trace_id: run_id.map(str::to_string),
                payload_json,
            })
            .await;
    }

    async fn status_for_campaign(&self, campaign_id: Option<String>, limit: i64) -> Result<Value> {
        let campaign = self.query_campaign(campaign_id.as_deref()).await?;
        let resolved_campaign_id = campaign
            .as_ref()
            .and_then(|c| c.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let runs = self
            .query_runs(resolved_campaign_id.as_deref(), limit)
            .await
            .unwrap_or_else(|_| json!([]));
        let events = if let Some(id) = resolved_campaign_id.as_deref() {
            self.query_events(Some(id), None, limit)
                .await
                .unwrap_or_else(|_| json!([]))
        } else {
            json!([])
        };
        let active_campaigns = self.active.lock().await.keys().cloned().collect::<Vec<_>>();
        Ok(json!({
            "schema": "missiond.codex-replay.status.v1",
            "activeCampaign": campaign,
            "runs": runs,
            "events": events,
            "activeRuntimeCampaignIds": active_campaigns,
            "codexBin": self.codex_bin,
            "prompts": {
                "review": REVIEW_PROMPT,
                "plan": PLAN_PROMPT,
                "implementPrefix": IMPLEMENT_PREFIX
            }
        }))
    }

    async fn query_campaign(&self, campaign_id: Option<&str>) -> Result<Option<Value>> {
        Ok(sqlx::query_scalar::<_, Value>(
            r#"
            SELECT to_jsonb(c)
            FROM (
              SELECT *
              FROM codex_replay_campaigns
              WHERE ($1::text IS NULL OR id = $1)
              ORDER BY updated_at DESC
              LIMIT 1
            ) c
            "#,
        )
        .bind(campaign_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn query_runs(&self, campaign_id: Option<&str>, limit: i64) -> Result<Value> {
        Ok(sqlx::query_scalar::<_, Value>(
            r#"
            SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY r.cycle_no DESC), '[]'::jsonb)
            FROM (
              SELECT *
              FROM codex_replay_runs
              WHERE ($1::text IS NULL OR campaign_id = $1)
              ORDER BY cycle_no DESC
              LIMIT $2
            ) r
            "#,
        )
        .bind(campaign_id)
        .bind(limit)
        .fetch_one(&self.pool)
        .await?)
    }

    async fn query_events(
        &self,
        campaign_id: Option<&str>,
        run_id: Option<&str>,
        limit: i64,
    ) -> Result<Value> {
        Ok(sqlx::query_scalar::<_, Value>(
            r#"
            SELECT COALESCE(jsonb_agg(to_jsonb(e) ORDER BY e.id DESC), '[]'::jsonb)
            FROM (
              SELECT *
              FROM codex_replay_events
              WHERE ($1::text IS NULL OR campaign_id = $1)
                AND ($2::text IS NULL OR run_id = $2)
              ORDER BY id DESC
              LIMIT $3
            ) e
            "#,
        )
        .bind(campaign_id)
        .bind(run_id)
        .bind(limit)
        .fetch_one(&self.pool)
        .await?)
    }

    async fn resolve_campaign_id(&self, campaign_id: Option<String>) -> Result<String> {
        if let Some(id) = campaign_id {
            return Ok(id);
        }
        let id = sqlx::query_scalar::<_, Option<String>>(
            "SELECT id FROM codex_replay_campaigns ORDER BY updated_at DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        id.flatten()
            .ok_or_else(|| anyhow!("no codex replay campaign exists"))
    }

    async fn load_campaign_config(&self, campaign_id: &str) -> Result<CampaignConfig> {
        let row = sqlx::query(
            r#"
            SELECT id, project_root, max_cycles, interval_seconds
            FROM codex_replay_campaigns
            WHERE id = $1
            "#,
        )
        .bind(campaign_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow!("campaign not found: {campaign_id}"))?;
        Ok(CampaignConfig {
            id: row.get("id"),
            project_root: row.get("project_root"),
            max_cycles: row.try_get("max_cycles").ok(),
            interval_seconds: row.get("interval_seconds"),
        })
    }

    async fn next_cycle_no(&self, campaign_id: &str) -> Result<i32> {
        let latest = sqlx::query_scalar::<_, Option<i32>>(
            "SELECT MAX(cycle_no) FROM codex_replay_runs WHERE campaign_id = $1",
        )
        .bind(campaign_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(latest.unwrap_or(0) + 1)
    }

    async fn campaign_id_for_run(&self, run_id: &str) -> Result<String> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT campaign_id FROM codex_replay_runs WHERE id = $1",
        )
        .bind(run_id)
        .fetch_one(&self.pool)
        .await?
        .ok_or_else(|| anyhow!("run not found: {run_id}"))
    }
}

struct CodexAppServerClient {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
    next_id: i64,
    pending: VecDeque<Value>,
    io_sink: Option<CodexIoSink>,
}

impl CodexAppServerClient {
    async fn connect(codex_bin: &str, io_sink: Option<CodexIoSink>) -> Result<Self> {
        let mut child = Command::new(codex_bin)
            .arg("app-server")
            .arg("--listen")
            .arg("stdio://")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawn {codex_bin} app-server --listen stdio://"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("codex app-server stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("codex app-server stdout unavailable"))?;
        if let (Some(stderr), Some(sink)) = (child.stderr.take(), io_sink.clone()) {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        sink("stderr", trimmed.to_string());
                    }
                }
            });
        }
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout).lines(),
            next_id: 0,
            pending: VecDeque::new(),
            io_sink,
        };
        client
            .request(
                "initialize",
                json!({
                    "clientInfo": { "name": "missiond-codex-replay", "version": env!("CARGO_PKG_VERSION") },
                    "capabilities": {},
                    "protocolVersion": "0.1.0"
                }),
            )
            .await?;
        Ok(client)
    }

    async fn start_thread(&mut self, cwd: &str) -> Result<ThreadContext> {
        let result = self
            .request(
                "thread/start",
                json!({
                    "cwd": cwd,
                    "approvalPolicy": "never",
                    "sandbox": "danger-full-access",
                    "ephemeral": false
                }),
            )
            .await?;
        let thread_id = result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("thread/start response missing thread.id"))?
            .to_string();
        let model = result
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("gpt-5.5")
            .to_string();
        let reasoning_effort = result
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(ThreadContext {
            thread_id,
            model,
            reasoning_effort,
        })
    }

    async fn start_turn(
        &mut self,
        thread: &ThreadContext,
        mode: &str,
        prompt: &str,
        cwd: &str,
    ) -> Result<String> {
        let result = self
            .request(
                "turn/start",
                json!({
                    "threadId": thread.thread_id.clone(),
                    "cwd": cwd,
                    "approvalPolicy": "never",
                    "sandboxPolicy": { "type": "dangerFullAccess" },
                    "collaborationMode": {
                        "mode": mode,
                        "settings": {
                            "model": thread.model.clone(),
                            "reasoning_effort": thread.reasoning_effort.clone(),
                            "developer_instructions": null
                        }
                    },
                    "input": [
                        { "type": "text", "text": prompt }
                    ]
                }),
            )
            .await?;
        result
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("turn/start response missing turn.id"))
    }

    async fn wait_for_turn<F>(
        &mut self,
        turn_id: &str,
        allow_recommended_input: bool,
        mut event_sink: F,
    ) -> Result<TurnOutcome, CycleError>
    where
        F: FnMut(&str, &str, Value),
    {
        let mut outcome = TurnOutcome::default();
        loop {
            let message = self.read_message().await.map_err(CycleError::from)?;
            let method = message.get("method").and_then(Value::as_str);
            match method {
                Some("item/agentMessage/delta") => {
                    if message.pointer("/params/turnId").and_then(Value::as_str) == Some(turn_id) {
                        if let Some(delta) =
                            message.pointer("/params/delta").and_then(Value::as_str)
                        {
                            outcome.text.push_str(delta);
                        }
                    }
                }
                Some("item/plan/delta") => {
                    if message.pointer("/params/turnId").and_then(Value::as_str) == Some(turn_id) {
                        if let Some(delta) =
                            message.pointer("/params/delta").and_then(Value::as_str)
                        {
                            outcome.text.push_str(delta);
                        }
                    }
                }
                Some("item/tool/requestUserInput") => {
                    if message.pointer("/params/turnId").and_then(Value::as_str) != Some(turn_id) {
                        self.pending.push_back(message);
                        continue;
                    }
                    if !allow_recommended_input {
                        return Err(CycleError::Blocked(
                            "request_user_input appeared outside Plan Mode replay step".to_string(),
                        ));
                    }
                    let request_id = message.get("id").cloned().ok_or_else(|| {
                        CycleError::Failed("request_user_input missing JSON-RPC id".to_string())
                    })?;
                    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
                    let selection = select_recommended_answers(&params)
                        .map_err(|reason| CycleError::Blocked(reason.to_string()))?;
                    self.respond(request_id, selection.response.clone())
                        .await
                        .map_err(CycleError::from)?;
                    outcome.selected_options.extend(selection.audit);
                    event_sink(
                        "codex_replay_option_selected",
                        "Selected all explicitly recommended Plan Mode options.",
                        selection.response,
                    );
                }
                Some("item/commandExecution/requestApproval")
                | Some("item/fileChange/requestApproval")
                | Some("item/permissions/requestApproval")
                | Some("mcpServer/elicitation/request") => {
                    if message.pointer("/params/turnId").and_then(Value::as_str) == Some(turn_id) {
                        return Err(CycleError::Blocked(format!(
                            "approval request {method:?} appeared during replay; runner does not guess approvals"
                        )));
                    }
                }
                Some("turn/completed") => {
                    if message.pointer("/params/turn/id").and_then(Value::as_str) != Some(turn_id) {
                        continue;
                    }
                    let status = message
                        .pointer("/params/turn/status")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    if status == "failed" {
                        return Err(CycleError::Failed(format!(
                            "turn {turn_id} failed: {}",
                            message
                                .pointer("/params/turn/error")
                                .map(Value::to_string)
                                .unwrap_or_else(|| "unknown error".to_string())
                        )));
                    }
                    if outcome.text.trim().is_empty() {
                        outcome.text = collect_turn_text(message.pointer("/params/turn"));
                    }
                    return Ok(outcome);
                }
                Some("turn/started") => {
                    if message.pointer("/params/turn/id").and_then(Value::as_str) == Some(turn_id) {
                        event_sink(
                            "codex_replay_turn_started",
                            "Codex turn started.",
                            json!({ "turnId": turn_id }),
                        );
                    }
                }
                Some("thread/status/changed") => {
                    event_sink(
                        "codex_replay_thread_status_changed",
                        "Codex thread status changed.",
                        message.get("params").cloned().unwrap_or_else(|| json!({})),
                    );
                }
                _ => {}
            }
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.write_json(&payload).await?;
        loop {
            let message = self.read_message().await?;
            if message.get("id") == Some(&json!(id)) {
                if let Some(error) = message.get("error") {
                    return Err(anyhow!("codex app-server {method} error: {error}"));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            self.pending.push_back(message);
        }
    }

    async fn respond(&mut self, id: Value, result: Value) -> Result<()> {
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
        .await
    }

    async fn write_json(&mut self, value: &Value) -> Result<()> {
        let line = serde_json::to_string(value)?;
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        self.emit_io("stdin", line);
        Ok(())
    }

    async fn read_message(&mut self) -> Result<Value> {
        if let Some(value) = self.pending.pop_front() {
            return Ok(value);
        }
        loop {
            let Some(line) = self.stdout.next_line().await? else {
                return Err(anyhow!("codex app-server stdout closed"));
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            self.emit_io("stdout", trimmed.to_string());
            let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };
            if value.get("id").is_some() || value.get("method").is_some() {
                return Ok(value);
            }
        }
    }

    fn emit_io(&self, direction: &'static str, line: String) {
        if let Some(sink) = &self.io_sink {
            sink(direction, line);
        }
    }
}

impl Drop for CodexAppServerClient {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[derive(Debug)]
struct RecommendedSelection {
    response: Value,
    audit: Vec<Value>,
}

fn select_recommended_answers(params: &Value) -> Result<RecommendedSelection, &'static str> {
    let questions = params
        .get("questions")
        .and_then(Value::as_array)
        .ok_or("request_user_input missing questions")?;
    let mut answers = Map::new();
    let mut audit = Vec::new();
    for question in questions {
        let id = question
            .get("id")
            .and_then(Value::as_str)
            .ok_or("request_user_input question missing id")?;
        let options = question
            .get("options")
            .and_then(Value::as_array)
            .ok_or("request_user_input question has no options")?;
        let labels = options
            .iter()
            .filter_map(|option| option.get("label").and_then(Value::as_str))
            .filter(|label| is_recommended_label(label))
            .map(str::to_string)
            .collect::<Vec<_>>();
        if labels.is_empty() {
            return Err("Plan Mode question has no explicitly recommended option");
        }
        answers.insert(id.to_string(), json!({ "answers": labels }));
        audit.push(json!({
            "questionId": id,
            "question": question.get("question").cloned().unwrap_or(Value::Null),
            "selectedLabels": answers.get(id).cloned().unwrap_or(Value::Null)
        }));
    }
    Ok(RecommendedSelection {
        response: json!({ "answers": answers }),
        audit,
    })
}

fn is_recommended_label(label: &str) -> bool {
    label.contains("Recommended") || label.contains("推荐")
}

pub(crate) fn extract_proposed_plan(text: &str) -> Option<String> {
    let open = "<proposed_plan>";
    let close = "</proposed_plan>";
    let start = text.rfind(open)? + open.len();
    let end = text[start..].find(close)? + start;
    let mut body = &text[start..end];
    if let Some(stripped) = body.strip_prefix('\n') {
        body = stripped;
    }
    if let Some(stripped) = body.strip_suffix('\n') {
        body = stripped;
    }
    if body.trim().is_empty() {
        return None;
    }
    Some(body.to_string())
}

fn collect_turn_text(turn: Option<&Value>) -> String {
    let mut out = String::new();
    if let Some(turn) = turn {
        collect_text_fields(turn, &mut out);
    }
    out
}

fn collect_text_fields(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
            for value in map.values() {
                collect_text_fields(value, out);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_text_fields(value, out);
            }
        }
        _ => {}
    }
}

fn stdio_event_payload(direction: &str, line: &str) -> Value {
    let truncated = line.chars().count() > MAX_STDIO_LINE_CHARS;
    let parsed = serde_json::from_str::<Value>(line).ok();
    let line = truncate_chars(line, MAX_STDIO_LINE_CHARS);
    json!({
        "direction": direction,
        "line": line,
        "truncated": truncated,
        "isJson": parsed.is_some(),
        "method": parsed
            .as_ref()
            .and_then(|value| value.get("method"))
            .and_then(Value::as_str),
        "id": parsed
            .as_ref()
            .and_then(|value| value.get("id"))
            .cloned()
    })
}

fn summarize_stdio_line(direction: &str, line: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return format!("{direction} raw line");
    };
    let id = value
        .get("id")
        .map(compact_json)
        .map(|value| format!(" id={value}"))
        .unwrap_or_default();
    if let Some(method) = value.get("method").and_then(Value::as_str) {
        return format!("{direction} {method}{id}");
    }
    if value.get("result").is_some() {
        return format!("{direction} response{id}");
    }
    if value.get("error").is_some() {
        return format!("{direction} error{id}");
    }
    format!("{direction} json line{id}")
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn bounded_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn default_codex_bin() -> String {
    if let Ok(bin) = std::env::var("MISSIOND_CODEX_BIN") {
        let bin = bin.trim();
        if !bin.is_empty() {
            return bin.to_string();
        }
    }

    for candidate in codex_binary_candidates() {
        if Path::new(&candidate).is_file() {
            return candidate;
        }
    }

    "codex".to_string()
}

fn codex_binary_candidates() -> Vec<String> {
    let mut candidates = vec![
        "/Applications/Codex.app/Contents/Resources/codex".to_string(),
        "/opt/homebrew/bin/codex".to_string(),
        "/usr/local/bin/codex".to_string(),
    ];
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(format!("{home}/.codex/packages/standalone/current/codex"));
        candidates.push(format!("{home}/.local/bin/codex"));
        candidates.push(format!("{home}/.cargo/bin/codex"));
    }
    candidates
}

fn current_dir_string() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().to_string())
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_constants_are_exact() {
        assert_eq!(
            REVIEW_PROMPT,
            "看一下 missonD 的 SSOT lisp,如果让你对这个程序 在架构层面进行优化、改进，你会选择改进哪里？为什么？"
        );
        assert_eq!(PLAN_PROMPT, "调查并设计执行方案。目标是这些问题全部解决");
        assert_eq!(
            IMPLEMENT_PREFIX.as_bytes(),
            b"PLEASE IMPLEMENT THIS PLAN:\n"
        );
    }

    #[test]
    fn extracts_plan_body_without_wrapper_newlines() {
        let text = "before\n<proposed_plan>\n# Title\nbody\n</proposed_plan>\nafter";
        assert_eq!(
            extract_proposed_plan(text).as_deref(),
            Some("# Title\nbody")
        );
    }

    #[test]
    fn extracts_last_plan_block() {
        let text = "<proposed_plan>\nold\n</proposed_plan>\n<proposed_plan>\nnew\n</proposed_plan>";
        assert_eq!(extract_proposed_plan(text).as_deref(), Some("new"));
    }

    #[test]
    fn selects_recommended_options_only() {
        let selection = select_recommended_answers(&json!({
            "questions": [{
                "id": "scope",
                "question": "Pick",
                "options": [
                    { "label": "协议级复刻 (Recommended)", "description": "stable" },
                    { "label": "界面级点击", "description": "fragile" }
                ]
            }]
        }))
        .unwrap();
        assert_eq!(
            selection.response,
            json!({ "answers": { "scope": { "answers": ["协议级复刻 (Recommended)"] } } })
        );
    }

    #[test]
    fn blocks_without_recommended_option() {
        let err = select_recommended_answers(&json!({
            "questions": [{
                "id": "scope",
                "question": "Pick",
                "options": [
                    { "label": "界面级点击", "description": "fragile" }
                ]
            }]
        }))
        .unwrap_err();
        assert_eq!(
            err,
            "Plan Mode question has no explicitly recommended option"
        );
    }
}
