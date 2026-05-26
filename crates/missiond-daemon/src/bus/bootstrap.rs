//! `BusServices` — daemon-side aggregate that owns every event-bus subsystem.
//!
//! Construction is a single async call at daemon boot:
//!
//! ```ignore
//! let bus = BusServices::bootstrap(store.pool(), &control_manager).await?;
//! let handle = bus.start(shutdown_rx).await?;
//! state.bus = bus;
//! // later on shutdown:
//! handle.shutdown().await;
//! ```
//!
//! `BusServices` is cheap to `Arc::clone` around state; all inner fields are
//! already `Arc` so clone is a handful of atomic increments.
//!
//! Wiring decisions:
//!   * PostgreSQL is the only MissionD runtime database backend and owns the
//!     event_log table.
//!   * The control gate is adapted from `watch::Receiver<ControlTree>` via
//!     [`crate::bus::control_gate_adapter::ControlTreeGate`].
//!   * Metrics use the shared `AtomicBusMetrics`; Phase 8 can swap a
//!     Prometheus backend per DC025.
//!   * BlobStore defaults to PgBlobStore (DC006). If in the future we wire
//!     LocalFileBlobStore for >1 MB payloads, this module is the place.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use missiond_core::event::{
    blob_store::{BlobStore, PgBlobStore},
    dispatcher::{
        register_all_domains, DispatchError, DispatchMetrics, Dispatcher, DispatcherBuilder,
        TailSource,
    },
    events::{
        BoardEvent, ExecutionEvent, IncidentEvent, LlmEvent, MemoryEvent, MessageEvent,
        ObservabilityEvent, QuestionEvent, SessionEvent, SlotEvent, SystemEvent, TaskEvent,
        WorkerEvent,
    },
    log::{
        writer::spawn_log_writer_with_metrics, AppendAck, AppendError, AppendOpts, Log,
        LogWriterHandle, Seq,
    },
    metrics::{
        emitter::ObservabilityAppender, spawn_bus_metrics_emitter, AtomicBusMetrics, BusMetrics,
        BusMetricsEmitterHandle,
    },
    subscription::{
        cursor_store::{CursorStore, PgCursorStore},
        failure::{DlqSink, PgDlqSink},
    },
    DomainEvent,
};
use sqlx::PgPool;
use sqlx::Row;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::bus::control_gate_adapter::ControlTreeGate;
use crate::bus::ws_bridge::WsBridgeHealth;
use crate::control_tree::ControlManager;
use crate::handlers::knowledge::evidence_collector::EventRefResolver;

/// Aggregate of every event-bus subsystem. Cheap to share via `Arc`.
pub struct BusServices {
    pub log: Arc<LogWriterHandle>,
    pub blob_store: Arc<dyn BlobStore>,
    pub cursor_store: Arc<dyn CursorStore>,
    pub dispatcher: Dispatcher,
    pub control_gate: ControlTreeGate,
    pub metrics: Arc<AtomicBusMetrics>,
    /// Dead-letter queue sink used by Phase 7 subscribers. Shared so every
    /// `bus.subscribe::<T>` call can hand the same backend to its
    /// `FailureRouter`.
    pub dlq: Arc<dyn DlqSink>,
    /// Tail source shared with the dispatcher loop. Stored so `start()` can
    /// spawn the tail task without re-constructing it.
    tail_source: Arc<dyn TailSource>,
    /// DB pool — kept around for retention / orphan-cleanup wiring in Phase 8.
    #[allow(dead_code)]
    pg_pool: PgPool,
    pub ws_bridge_health: Arc<WsBridgeHealth>,
    /// wave-16 / task 07 — passive in-memory cache of recently-published
    /// `ExecutionEvent::PlanNodeStateChanged` ids, keyed by deterministic
    /// correlation tuple. Populated by `spawn_event_ref_cache_sub` so
    /// downstream evidence call sites that no longer carry the live `Seq`
    /// can recover an event id post-hoc. See `EventRefResolver` docstring
    /// for the lookup contract (cache miss → `EventRef::unavailable(...)`).
    pub event_ref_resolver: Arc<EventRefResolver>,
}

/// Handle returned by [`BusServices::start`]. Dropping it signals shutdown.
pub struct BusStartHandle {
    shutdown_tx: watch::Sender<bool>,
    dispatcher_join: Option<JoinHandle<Result<DispatchMetrics, DispatchError>>>,
    metrics_handle: Option<BusMetricsEmitterHandle>,
}

impl BusStartHandle {
    /// Gracefully stop the dispatcher + metrics emitter and wait for the
    /// tasks to exit. Idempotent.
    pub async fn shutdown(mut self) -> Result<DispatchMetrics> {
        let _ = self.shutdown_tx.send(true);
        if let Some(m) = self.metrics_handle.take() {
            m.shutdown().await;
        }
        let metrics = match self.dispatcher_join.take() {
            Some(h) => match h.await {
                Ok(Ok(m)) => m,
                Ok(Err(DispatchError::Shutdown)) => DispatchMetrics::default(),
                Ok(Err(e)) => {
                    warn!(error = %e, "dispatcher loop exited with error");
                    DispatchMetrics::default()
                }
                Err(e) => {
                    warn!(error = %e, "dispatcher task panicked");
                    DispatchMetrics::default()
                }
            },
            None => DispatchMetrics::default(),
        };
        Ok(metrics)
    }
}

impl Drop for BusStartHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl BusServices {
    /// Build every subsystem. Safe to call on any tokio runtime — the
    /// `LogWriter` task is spawned here but the dispatcher tail + metrics
    /// emitter wait for [`Self::start`].
    pub async fn bootstrap(pool: PgPool, control_manager: &ControlManager) -> Result<Arc<Self>> {
        info!("bus: bootstrap starting");

        // Blob store (Postgres-backed per DC006).
        let blob_store: Arc<dyn BlobStore> = Arc::new(PgBlobStore::new(pool.clone()));

        // Metrics collector — shared by LogWriter, dispatcher, subscriptions,
        // health snapshots, and the periodic Observability emitter.
        let metrics = Arc::new(AtomicBusMetrics::new());

        // Log writer: one task drains the append channel.
        let log_handle =
            spawn_log_writer_with_metrics(pool.clone(), blob_store.clone(), metrics.clone());
        let log = Arc::new(log_handle);

        // Cursor store — used by Phase 4 subscribers (Phase 7 will wire them).
        let cursor_store: Arc<dyn CursorStore> = Arc::new(PgCursorStore::new(pool.clone()));

        // Dead-letter queue sink — Phase 7 subscribers route retry-exhausted
        // failures here instead of blocking the subscription forever.
        let dlq: Arc<dyn DlqSink> = Arc::new(PgDlqSink::new(pool.clone()));

        // Dispatcher — registers every domain in `Domain::ALL` up front so
        // `topic::<T>()` never panics. The domain set started at 12 and is
        // extensible.
        let dispatcher = register_all_domains(DispatcherBuilder::new()).build();

        // Control gate — adapter over the daemon's ControlManager.
        let control_gate = ControlTreeGate::new(control_manager.subscribe());

        // Tail source backing the dispatcher. Uses the Postgres tail reader.
        let tail_source: Arc<dyn TailSource> = Arc::new(
            missiond_core::event::dispatcher::PgTailSource::new(pool.clone(), blob_store.clone()),
        );

        info!("bus: bootstrap complete");

        // wave-16 / task 07 — resolver is a plain in-memory cache; the
        // subscriber that populates it is started later by
        // `bus::v2_subscribers::start_v2_subscribers` (it needs the
        // shutdown receiver from main.rs).
        let event_ref_resolver = Arc::new(EventRefResolver::new());
        let ws_bridge_health = Arc::new(WsBridgeHealth::new());

        Ok(Arc::new(Self {
            log,
            blob_store,
            cursor_store,
            dispatcher,
            control_gate,
            metrics,
            dlq,
            tail_source,
            pg_pool: pool,
            ws_bridge_health,
            event_ref_resolver,
        }))
    }

    pub async fn health_snapshot(&self) -> serde_json::Value {
        let head_seq = sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(seq) FROM event_log")
            .fetch_one(&self.pg_pool)
            .await
            .unwrap_or(None)
            .unwrap_or(0);
        let last_dispatched_seq = self.dispatcher.last_dispatched_seq().0;
        self.metrics
            .record_dispatch_lag(head_seq.saturating_sub(last_dispatched_seq));
        let dlq_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM dead_letter_queue")
            .fetch_one(&self.pg_pool)
            .await
            .unwrap_or(0);
        let subscriptions = sqlx::query(
            r#"
            SELECT subscription_name, consumer_name, domain, last_acked_seq, last_seen_at,
                   GREATEST($1::bigint - last_acked_seq, 0) AS lag
            FROM event_subscriptions
            ORDER BY lag DESC, subscription_name ASC
            LIMIT 20
            "#,
        )
        .bind(head_seq)
        .fetch_all(&self.pg_pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            use sqlx::Row;
            serde_json::json!({
                "subscriptionName": row.try_get::<String, _>("subscription_name").unwrap_or_default(),
                "consumerName": row.try_get::<String, _>("consumer_name").unwrap_or_default(),
                "domain": row.try_get::<String, _>("domain").unwrap_or_default(),
                "lastAckedSeq": row.try_get::<i64, _>("last_acked_seq").unwrap_or(0),
                "lastSeenAt": row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_seen_at").ok().flatten().map(|dt| dt.to_rfc3339()),
                "lag": row.try_get::<i64, _>("lag").unwrap_or(0),
            })
        })
        .collect::<Vec<_>>();
        for sub in &subscriptions {
            if let (Some(name), Some(lag)) = (
                sub.get("subscriptionName").and_then(|v| v.as_str()),
                sub.get("lag").and_then(|v| v.as_i64()),
            ) {
                self.metrics.record_subscription_lag(name, lag);
                if lag > 1000 {
                    self.metrics.record_slow_consumer(name);
                }
            }
        }
        let metrics = self.metrics.snapshot();
        serde_json::json!({
            "publish_count": metrics.append_ok,
            "published": metrics.append_ok,
            "appendOk": metrics.append_ok,
            "appendFail": metrics.append_fail,
            "appendBytes": metrics.append_bytes,
            "reject": metrics.reject,
            "dispatchLag": metrics.dispatch_lag,
            "lagged": metrics.lagged,
            "slowConsumer": metrics.slow_consumer,
            "controlGateDropped": metrics.control_gate_dropped,
            "estimatedBacklog": metrics.dispatch_lag.max(0),
            "headSeq": head_seq,
            "lastDispatchedSeq": last_dispatched_seq,
            "dlq": {
                "count": dlq_count,
            },
            "subscriptions": subscriptions,
            "perDomainAppend": metrics.per_domain_append.into_iter().map(|(domain, value)| (domain.as_str().to_string(), value)).collect::<std::collections::HashMap<_, _>>(),
            "perDomainReject": metrics.per_domain_reject.into_iter().map(|(domain, value)| (domain.as_str().to_string(), value)).collect::<std::collections::HashMap<_, _>>(),
            "perDomainDropped": metrics.per_domain_dropped.into_iter().map(|(domain, value)| (domain.as_str().to_string(), value)).collect::<std::collections::HashMap<_, _>>(),
            "perDomainTopicDepth": metrics.per_domain_topic_depth.into_iter().map(|(domain, value)| (domain.as_str().to_string(), value)).collect::<std::collections::HashMap<_, _>>(),
            "perSubscriptionLag": metrics.per_sub_lag,
            "wsBridge": self.ws_bridge_health.snapshot(),
        })
    }

    pub async fn record_operator_health_sample(
        &self,
        workers: serde_json::Value,
        evidence: serde_json::Value,
        pending_questions: i64,
    ) {
        let event_bus = self.health_snapshot().await;
        let worker_items = workers.as_array().cloned().unwrap_or_default();
        let worker_failed = worker_items
            .iter()
            .filter(|worker| {
                worker
                    .pointer("/health/lifecycle")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s == "failed")
            })
            .count() as i64;
        let worker_stale = worker_items
            .iter()
            .filter(|worker| {
                worker
                    .pointer("/health/stale")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            })
            .count() as i64;
        let event_dispatch_lag = event_bus
            .get("dispatchLag")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let dlq_count = event_bus
            .pointer("/dlq/count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let evidence_missing = evidence
            .get("missing")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let snapshot = serde_json::json!({
            "workers": workers,
            "eventBus": event_bus,
            "evidence": evidence,
            "pendingQuestions": pending_questions,
        });
        let _ = sqlx::query(
            r#"
            INSERT INTO operator_health_samples
              (worker_failed, worker_stale, event_dispatch_lag, dlq_count,
               evidence_missing, pending_questions, snapshot)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(worker_failed)
        .bind(worker_stale)
        .bind(event_dispatch_lag)
        .bind(dlq_count)
        .bind(evidence_missing)
        .bind(pending_questions)
        .bind(snapshot)
        .execute(&self.pg_pool)
        .await;
        let _ = sqlx::query(
            "DELETE FROM operator_health_samples WHERE sampled_at < now() - interval '7 days'",
        )
        .execute(&self.pg_pool)
        .await;
    }

    pub async fn operator_trends(&self) -> serde_json::Value {
        let rows = sqlx::query(
            r#"
            SELECT sampled_at, worker_failed, worker_stale, event_dispatch_lag,
                   dlq_count, evidence_missing, pending_questions
            FROM operator_health_samples
            WHERE sampled_at >= now() - interval '24 hours'
            ORDER BY sampled_at DESC
            LIMIT 60
            "#,
        )
        .fetch_all(&self.pg_pool)
        .await
        .unwrap_or_default();
        let mut points = rows
            .into_iter()
            .map(|row| {
                let sampled_at = row
                    .try_get::<chrono::DateTime<chrono::Utc>, _>("sampled_at")
                    .ok()
                    .map(|dt| dt.to_rfc3339());
                serde_json::json!({
                    "sampledAt": sampled_at,
                    "workerFailed": row.try_get::<i64, _>("worker_failed").unwrap_or(0),
                    "workerStale": row.try_get::<i64, _>("worker_stale").unwrap_or(0),
                    "eventDispatchLag": row.try_get::<i64, _>("event_dispatch_lag").unwrap_or(0),
                    "dlqCount": row.try_get::<i64, _>("dlq_count").unwrap_or(0),
                    "evidenceMissing": row.try_get::<i64, _>("evidence_missing").unwrap_or(0),
                    "pendingQuestions": row.try_get::<i64, _>("pending_questions").unwrap_or(0),
                })
            })
            .collect::<Vec<_>>();
        points.reverse();
        let max_for = |field: &str, since_secs: i64| -> i64 {
            let now = chrono::Utc::now();
            points
                .iter()
                .filter(|point| {
                    point
                        .get("sampledAt")
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| {
                            now.signed_duration_since(dt.with_timezone(&chrono::Utc))
                                .num_seconds()
                                <= since_secs
                        })
                        .unwrap_or(false)
                })
                .filter_map(|point| point.get(field).and_then(|v| v.as_i64()))
                .max()
                .unwrap_or(0)
        };
        serde_json::json!({
            "schema": "missiond.operator-health-trends.v1",
            "points": points,
            "windows": {
                "1h": {
                    "eventDispatchLagMax": max_for("eventDispatchLag", 3600),
                    "dlqMax": max_for("dlqCount", 3600),
                    "workerFailedMax": max_for("workerFailed", 3600),
                    "workerStaleMax": max_for("workerStale", 3600),
                    "evidenceMissingMax": max_for("evidenceMissing", 3600),
                    "pendingQuestionsMax": max_for("pendingQuestions", 3600)
                },
                "24h": {
                    "eventDispatchLagMax": max_for("eventDispatchLag", 24 * 3600),
                    "dlqMax": max_for("dlqCount", 24 * 3600),
                    "workerFailedMax": max_for("workerFailed", 24 * 3600),
                    "workerStaleMax": max_for("workerStale", 24 * 3600),
                    "evidenceMissingMax": max_for("evidenceMissing", 24 * 3600),
                    "pendingQuestionsMax": max_for("pendingQuestions", 24 * 3600)
                }
            }
        })
    }

    pub async fn dlq_list(&self, limit: i64) -> Result<serde_json::Value> {
        let rows = sqlx::query(
            r#"
            SELECT id, subscription_name, event_seq, failure_reason,
                   payload_snapshot, created_at
            FROM dead_letter_queue
            ORDER BY created_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pg_pool)
        .await?;
        Ok(serde_json::json!({
            "schema": "missiond.event-bus-dlq.v1",
            "items": rows.into_iter().map(|row| {
                let created_at = row
                    .try_get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                    .ok()
                    .map(|dt| dt.to_rfc3339());
                serde_json::json!({
                    "id": row.try_get::<i64, _>("id").unwrap_or_default(),
                    "subscriptionName": row.try_get::<String, _>("subscription_name").unwrap_or_default(),
                    "eventSeq": row.try_get::<i64, _>("event_seq").unwrap_or_default(),
                    "failureReason": row.try_get::<String, _>("failure_reason").unwrap_or_default(),
                    "payloadSnapshot": row.try_get::<Option<serde_json::Value>, _>("payload_snapshot").ok().flatten(),
                    "createdAt": created_at,
                })
            }).collect::<Vec<_>>()
        }))
    }

    pub async fn dlq_ack(&self, id: i64) -> Result<serde_json::Value> {
        let deleted = sqlx::query("DELETE FROM dead_letter_queue WHERE id = $1")
            .bind(id)
            .execute(&self.pg_pool)
            .await?
            .rows_affected();
        Ok(serde_json::json!({
            "schema": "missiond.event-bus-dlq-action.v1",
            "action": "dlq_ack",
            "id": id,
            "ok": deleted > 0,
            "deleted": deleted,
        }))
    }

    pub async fn dlq_replay(&self, id: i64) -> Result<serde_json::Value> {
        let row =
            sqlx::query("SELECT subscription_name, event_seq FROM dead_letter_queue WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pg_pool)
                .await?;
        let Some(row) = row else {
            return Ok(serde_json::json!({
                "schema": "missiond.event-bus-dlq-action.v1",
                "action": "dlq_replay",
                "id": id,
                "ok": false,
                "error": "not-found",
            }));
        };
        let subscription_name = row.try_get::<String, _>("subscription_name")?;
        let event_seq = row.try_get::<i64, _>("event_seq")?;
        let replay_cursor = event_seq.saturating_sub(1);
        let updated = sqlx::query(
            r#"
            UPDATE event_subscriptions
            SET last_acked_seq = LEAST(last_acked_seq, $2), last_seen_at = now()
            WHERE subscription_name = $1
            "#,
        )
        .bind(&subscription_name)
        .bind(replay_cursor)
        .execute(&self.pg_pool)
        .await?
        .rows_affected();
        let _ = self.dlq_ack(id).await?;
        Ok(serde_json::json!({
            "schema": "missiond.event-bus-dlq-action.v1",
            "action": "dlq_replay",
            "id": id,
            "ok": updated > 0,
            "subscriptionName": subscription_name,
            "eventSeq": event_seq,
            "cursor": replay_cursor,
            "updated": updated,
        }))
    }

    /// Start the dispatcher tail loop and the metrics emitter. Returns a
    /// handle that, when dropped, signals shutdown.
    pub async fn start(
        self: &Arc<Self>,
        external_shutdown: watch::Receiver<bool>,
    ) -> Result<BusStartHandle> {
        // Combined shutdown: triggered by either external shutdown or our
        // own handle drop.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        spawn_shutdown_fuse(external_shutdown, shutdown_tx.clone());

        // Dispatcher loop.
        let dispatcher = self.dispatcher.clone();
        let tail_source = self.tail_source.clone();
        let blob = self.blob_store.clone();
        let gate = self.control_gate.clone();
        let metrics = self.metrics.clone();
        let dispatcher_join = tokio::spawn(async move {
            dispatcher
                .run(tail_source, blob, gate, shutdown_rx, metrics)
                .await
        });

        // Metrics emitter — needs an ObservabilityAppender.
        let appender: Arc<dyn ObservabilityAppender> = Arc::new(LogObservabilityAdapter {
            log: self.log.clone(),
        });
        let metrics_handle = spawn_bus_metrics_emitter(
            self.metrics.clone(),
            appender,
            missiond_core::event::metrics::emitter::METRICS_EMIT_INTERVAL,
        );

        Ok(BusStartHandle {
            shutdown_tx,
            dispatcher_join: Some(dispatcher_join),
            metrics_handle: Some(metrics_handle),
        })
    }

    // ── Publish convenience wrappers ───────────────────────────────────
    //
    // Each of these simply forwards to `Log::append` with the right
    // `AppendOpts`. Producer sites that don't have a dedupe_key or tracing
    // context just call these — the helper sets `producer_id` from the
    // module path so metrics carry enough label info.

    pub async fn publish_slot(&self, ev: SlotEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("slot")).await
    }

    pub async fn publish_board(&self, ev: BoardEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("board")).await
    }

    pub async fn publish_task(&self, ev: TaskEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("task")).await
    }

    pub async fn publish_question(&self, ev: QuestionEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("question")).await
    }

    pub async fn publish_llm(&self, ev: LlmEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("llm")).await
    }

    pub async fn publish_worker(&self, ev: WorkerEvent) -> Result<AppendAck, AppendError> {
        // I007 ephemeral audit: mirror the v1 `is_ephemeral()` decisions for
        // high-volume telemetry so we don't bloat `event_log`.
        let ephemeral = matches!(
            ev,
            WorkerEvent::LlmCall { .. }
                | WorkerEvent::BriefingBatchStarted { .. }
                | WorkerEvent::BriefingSummaryGenerated { .. }
                | WorkerEvent::TranslationStarted { .. }
                | WorkerEvent::NarrationBatchCompleted { .. }
        );
        let mut opts = default_opts("worker");
        opts.ephemeral = ephemeral;
        self.publish(ev, opts).await
    }

    pub async fn publish_memory(&self, ev: MemoryEvent) -> Result<AppendAck, AppendError> {
        let ephemeral = matches!(
            ev,
            MemoryEvent::TurnExtracted { .. } | MemoryEvent::IntentAnalyzed { .. }
        );
        let mut opts = default_opts("memory");
        opts.ephemeral = ephemeral;
        self.publish(ev, opts).await
    }

    pub async fn publish_message(&self, ev: MessageEvent) -> Result<AppendAck, AppendError> {
        let ephemeral = matches!(ev, MessageEvent::ImageInserted { .. });
        let mut opts = default_opts("message");
        opts.ephemeral = ephemeral;
        self.publish(ev, opts).await
    }

    pub async fn publish_session(&self, ev: SessionEvent) -> Result<AppendAck, AppendError> {
        let ephemeral = matches!(ev, SessionEvent::Organized { .. });
        let mut opts = default_opts("session");
        opts.ephemeral = ephemeral;
        self.publish(ev, opts).await
    }

    pub async fn publish_system(&self, ev: SystemEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("system")).await
    }

    pub async fn publish_system_webhook(&self, ev: SystemEvent) -> Result<AppendAck, AppendError> {
        match &ev {
            SystemEvent::ExternalServiceEvent {
                service_id,
                event_id,
                ..
            } => {
                let mut opts = default_opts(&format!("external/{service_id}"));
                opts.dedupe_key = Some(external_service_dedupe_key(service_id, event_id));
                self.publish(ev, opts).await
            }
            _ => self.publish_system(ev).await,
        }
    }

    pub async fn publish_observability(
        &self,
        ev: ObservabilityEvent,
    ) -> Result<AppendAck, AppendError> {
        // Observability events are always ephemeral (frozen lisp §4.4).
        let mut opts = default_opts("observability");
        opts.ephemeral = true;
        self.publish(ev, opts).await
    }

    pub async fn publish_incident(&self, ev: IncidentEvent) -> Result<AppendAck, AppendError> {
        self.publish(ev, default_opts("incident")).await
    }

    pub async fn publish_execution(&self, ev: ExecutionEvent) -> Result<AppendAck, AppendError> {
        // Execution events are durable projections of the on-disk companion
        // log — keep them persistent (default) so audit consumers can replay.
        self.publish(ev, default_opts("execution")).await
    }

    /// Publish an `ExecutionEvent` and return the assigned monotonic `Seq`
    /// (or the underlying `AppendError`). Convenience wrapper for producers
    /// that want the live event id so they can stamp it on a downstream
    /// audit artifact (e.g. evidence-collector `EventRef::new(..., seq)`).
    ///
    /// Use this instead of `publish_execution` when the producer plans to
    /// embed the `Seq` in another structured log entry — the helper hides
    /// the `AppendAck` discriminant matching at the call site so the
    /// producer treats `Committed` / `Volatile` / `AlreadyExists` uniformly:
    /// each variant returns the same `Seq` (`AlreadyExists` reuses the
    /// dedupe-existing seq, which is the right id to correlate against).
    pub async fn publish_execution_with_seq(&self, ev: ExecutionEvent) -> Result<Seq, AppendError> {
        let ack = self.publish_execution(ev).await?;
        Ok(ack.seq())
    }

    /// Core `append` wrapper. Producer sites that need custom `AppendOpts`
    /// (dedupe_key, span context) should call this directly.
    pub async fn publish<E: DomainEvent>(
        &self,
        event: E,
        opts: AppendOpts,
    ) -> Result<AppendAck, AppendError> {
        self.log.append(event, opts).await
    }

    // ── Subscribe convenience ───────────────────────────────────────────
    //
    // Phase 7 consumers call `bus.subscribe::<T>(name, opts).await?` to get a
    // live Subscription<T> pre-wired to the dispatcher topic + cursor store +
    // DLQ. Combinators (`.debounce`, `.coalesce`, …) are exposed on
    // Subscription<T> and stack on top of the returned handle.

    /// Subscribe to a typed domain topic. Returns a `Subscription<T>` pre-
    /// wired to the shared log / topic / cursor store / DLQ. Combinators
    /// attach on top of the returned handle.
    pub async fn subscribe<T>(
        &self,
        name: &str,
        opts: missiond_core::event::subscription::SubscriptionOpts,
    ) -> Result<
        missiond_core::event::subscription::Subscription<T>,
        missiond_core::event::subscription::SubscribeError,
    >
    where
        T: DomainEvent,
    {
        use missiond_core::event::log::LogReadable;
        let log: Arc<dyn LogReadable> = self.log.clone();
        let topic = self.dispatcher.topic::<T>();
        missiond_core::event::subscription::subscribe::<T>(
            name,
            opts,
            log,
            topic,
            self.cursor_store.clone(),
            self.dlq.clone(),
        )
        .await
    }
}

fn default_opts(producer: &str) -> AppendOpts {
    AppendOpts {
        producer_id: format!("daemon/{producer}"),
        ..Default::default()
    }
}

fn external_service_dedupe_key(service_id: &str, event_id: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!("missiond:external-service:{service_id}:{event_id}").as_bytes(),
    )
}

/// Adapter so the metrics emitter can append via the PG `LogWriterHandle`
/// without the caller needing to know about generic `Log::append`.
struct LogObservabilityAdapter {
    log: Arc<LogWriterHandle>,
}

#[async_trait]
impl ObservabilityAppender for LogObservabilityAdapter {
    async fn append_observability(
        &self,
        event: ObservabilityEvent,
        opts: AppendOpts,
    ) -> Result<Seq, AppendError> {
        let ack = self.log.append(event, opts).await?;
        Ok(ack.seq())
    }
}

/// When `external_shutdown` flips to `true`, also flip `local_shutdown_tx`.
/// Lets the dispatcher observe either signal.
fn spawn_shutdown_fuse(
    mut external_shutdown: watch::Receiver<bool>,
    local_shutdown_tx: watch::Sender<bool>,
) {
    tokio::spawn(async move {
        loop {
            if external_shutdown.changed().await.is_err() {
                return;
            }
            if *external_shutdown.borrow() {
                let _ = local_shutdown_tx.send(true);
                return;
            }
        }
    });
}

// Forward AppendOpts helper field, so producer sites that already have a
// `BusServices` handle can grab a well-formed default.
impl BusServices {
    /// Re-exported helper so callers can build `AppendOpts` without pulling
    /// in a `missiond-core` path for the common case.
    pub fn default_append_opts(producer: &str) -> AppendOpts {
        default_opts(producer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_opts_stamps_producer_id() {
        let o = default_opts("compute/task");
        assert_eq!(o.producer_id, "daemon/compute/task");
        assert!(!o.ephemeral);
        assert!(o.dedupe_key.is_none());
    }

    #[test]
    fn external_service_dedupe_key_is_stable_and_scoped() {
        let a = external_service_dedupe_key("deploy-center", "deploy-center:deploy_events:1");
        let b = external_service_dedupe_key("deploy-center", "deploy-center:deploy_events:1");
        let c = external_service_dedupe_key("auth", "deploy-center:deploy_events:1");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn log_re_export_types_are_visible() {
        // Just ensure the re-exports compile; no behavior.
        let _ = missiond_core::event::log::writer::APPEND_CHANNEL_CAPACITY;
    }
}
