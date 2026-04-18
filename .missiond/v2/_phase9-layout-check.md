# Phase 9 · Layout Alignment Check

Produced 2026-04-19 on branch `refactor/event-bus-v2`. Maps every frozen lisp §4.2 component to its physical location in the code tree. Any "missing" or "ghost" entry is flagged; everything resolved.

Frozen lisp reference: `/Users/jinchen/Projects/missiond/.missiond/v2/intent-event-bus.lisp`
Execution log:           `/Users/jinchen/Projects/missiond/.missiond/v2/intent-event-bus-execution.lisp`

---

## 1. `component event-types` (§4.2.a schema)

Target in lisp: `crates/missiond-core/src/event/`

| Lisp element                  | Actual code                                                                        | Status |
|-------------------------------|------------------------------------------------------------------------------------|--------|
| `trait DomainEvent`           | `crates/missiond-core/src/event/event_trait.rs`                                    | OK     |
| `enum Domain` + `Domain::ALL` | `crates/missiond-core/src/event/domain.rs`                                         | OK     |
| 12 domain enums               | `crates/missiond-core/src/event/events/{slot,board,task,question,llm,worker,memory,message,session,system,observability,incident}.rs` | OK     |
| `topic-discovery`             | `Domain::ALL: [Domain; 12]` const in `domain.rs` — compile-time                    | OK     |
| Provider labels               | `events/llm.rs::Provider` (+ `LegacyXxx` variants kept per DC004)                  | OK     |

All 12 domains present, one file per enum. 55 variants total (49 v1 + 6 Phase-1 additions: `SlotEvent::Stuck`, `ObservabilityEvent::{HealthSnapshot,BusMetric,SlowConsumer,RetentionReport}`, `IncidentEvent::{Reported,Resolved,StaleSubscription}`). Phase 8 added `RetentionReport` + `StaleSubscription` (DC043 ephemeral audit, I010 orphan cleanup).

---

## 2. `component event-log` (§4.2.b storage)

Target in lisp: `crates/missiond-core/src/event/log.rs`
Actual target:  `crates/missiond-core/src/event/log/` (directory — intentional split)

| Lisp element                                       | Actual code                                                            | Status   |
|----------------------------------------------------|------------------------------------------------------------------------|----------|
| `append-api` (`log.append`, AppendOpts, AppendAck) | `event/log/mod.rs` (trait) + `event/log/writer.rs` (PG impl)           | OK       |
| Writer task `LogWriter`                            | `event/log/writer.rs::LogWriter + spawn_log_writer`                    | OK       |
| `writer-semantics` (batch ≤100 / 10ms)             | `writer.rs::LogWriter::drain_batch` + `APPEND_CHANNEL_CAPACITY=4096`   | OK       |
| `seq-authority` (BIGSERIAL)                        | Migration `20260419000000_event_log.sql`                               | OK       |
| `dedup-semantics`                                  | `writer.rs::PgWriterBackend::find_existing_seq` + UNIQUE index         | OK       |
| `persistence-policy` (ephemeral fast-path)         | `writer.rs::append` early-return with `AtomicI64` volatile_counter     | OK       |
| `backpressure` (bounded channel)                   | `writer.rs` tokio mpsc cap 4096, `TrySendError::Full → Backpressure`   | OK       |
| `retention`                                        | `event/log/retention.rs::cleanup_once + CleanupReport`                 | OK       |
| `LogReader` (catch-up pull API)                    | `event/log/reader.rs::LogReader + LoggedEvent`                         | OK       |
| `LogReadable` split (dyn-compat)                   | `event/log/mod.rs::LogReadable` blanket impl (DC016)                   | OK       |
| Claim-check layer                                  | `event/blob_store/{mod,pg_backend,local_file_backend,claim_check}.rs`  | OK       |
| `struct PayloadRef`                                | `event/blob_store/mod.rs::PayloadRef` (DC007 hex checksum)             | OK       |
| BlobBackends (blob-table / local-file)             | `pg_backend.rs::PgBlobStore` + `local_file_backend.rs::LocalFileBlobStore` | OK   |

Filesystem divergence: lisp says `log.rs` (single file); code uses `log/` dir. No contract change — all types still live under `crate::event::log::*`. Acceptable directory expansion, consistent with `blob_store/` (which is also a dir in lisp's §4.2.b).

---

## 3. `component topic-dispatcher` (§4.2.c routing)

Target in lisp: `crates/missiond-core/src/event/dispatcher.rs`
Actual target:  `crates/missiond-core/src/event/dispatcher/` (directory)

| Lisp element                              | Actual code                                                      | Status |
|-------------------------------------------|------------------------------------------------------------------|--------|
| `scope-invariant` (live fan-out only)     | `dispatcher/mod.rs::Dispatcher::run` — no per-sub state          | OK     |
| `tail-mechanism` (long-poll SELECT)       | `dispatcher/tail.rs::{PgTailSource, run_tail}` (DC011 poll)      | OK     |
| `topic-registry`                          | `dispatcher/registry.rs::TopicRegistry` + `register_all_domains` | OK     |
| `per-topic broadcast::Sender<Arc<T>>`     | `dispatcher/topic.rs::Topic<T>` buffer=1024 (DC015)              | OK     |
| `control-gate` trait                      | `dispatcher/control_gate.rs::ControlGate + NeverPaused + CtlDomain` | OK   |
| Stateless Dispatcher (O(1))               | `last_dispatched_seq: AtomicI64` only (DC012)                    | OK     |

Same dir-vs-file divergence as log — five files under `dispatcher/`; contract intact.

---

## 4. `component subscription-api` + `subscription-lifecycle` + `cursor-store` + combinators (§4.3 egress)

Target in lisp: not explicitly spelled; convention `crates/missiond-core/src/event/subscription/`

| Lisp element                          | Actual code                                                       | Status |
|---------------------------------------|-------------------------------------------------------------------|--------|
| `bus.subscribe::<T>` entry            | `subscription/api.rs::subscribe`                                  | OK     |
| `SubscriptionOpts`                    | `subscription/options.rs::SubscriptionOpts + StartFrom`           | OK     |
| `FailurePolicy`                       | `subscription/failure.rs::{FailurePolicy, FailureRouter, DlqSink, PgDlqSink}` | OK |
| `PauseBehavior`                       | `subscription/options.rs::PauseBehavior` (FreezeAndCatchUp alias, D001) | OK |
| `CursorFlush` (BatchOr1s default)     | `subscription/options.rs::CursorFlush` + flusher task in `api.rs` | OK     |
| Two-phase lifecycle                   | `subscription/lifecycle.rs::Lifecycle<T>` (bootstrap → live)      | OK     |
| `event_subscriptions` cursor table    | `subscription/cursor_store.rs::{CursorStore, PgCursorStore, InMemoryCursorStore}` | OK |
| 6 combinators                         | `subscription/combinators.rs::{filter, map, debounce, rate_limit, coalesce, batch}` | OK |
| `Ack<T>` handshake (DC018 drop-as-nack) | `subscription/mod.rs::Ack + FlushSignal::Nack`                  | OK     |
| DLQ for Retry-exhausted (DC020)       | `failure.rs::FailureRouter` routes `Retry{max}` overflow → DLQ    | OK     |

Two non-lisp files (`lifecycle.rs`, `combinators.rs`) — both are sub-components of the subscription pillar, not ghosts.

---

## 5. `cross-cutting` components (§4.4)

| Lisp element                                | Actual code                                              | Status |
|---------------------------------------------|----------------------------------------------------------|--------|
| `causation-loop-guard` (MAX_DEPTH=10)       | `event/guards/causation.rs::check_causation`             | OK     |
| `observability` metrics                     | `event/metrics/{mod,emitter}.rs::{BusMetrics, AtomicBusMetrics, BusMetricsEmitter}` | OK |
| `fault-isolation`                           | Enforced in writer/dispatcher/subscription; chaos tests validate | OK |
| `testing-story` (InMemoryBus)               | `event/in_memory/{mod,log,blob_store,cursor_store,control_gate}.rs` | OK |
| `chaos-test-matrix` (9 scenarios + 3 sanity) | `tests/event_chaos.rs` — 12 tests, all passing          | OK     |

---

## 6. Daemon-side glue (not in frozen lisp — wiring layer)

These live in `crates/missiond-daemon/src/bus/` and are specifically called out in `intent-event-bus-execution.lisp` decisions DC010/DC030/DC034/DC036/DC037/DC041/DC042/DC044/DC045/DC046.

| Daemon file                    | Purpose                                                                 | Lisp anchor                     |
|--------------------------------|-------------------------------------------------------------------------|---------------------------------|
| `bus/bootstrap.rs`             | `BusServices` aggregator (log + blob + cursor + dispatcher + gate + metrics + dlq) + publish_* helpers + `default_append_opts` | DC030 DC034 DC035 DC043 |
| `bus/control_gate_adapter.rs`  | `impl ControlGate for ControlTreeGate` over `watch::Receiver<ControlTree>` | DC010 DC033                  |
| `bus/v2_subscribers.rs`        | `start_v2_subscribers` — 8 router consumers + incident reactor + intent_analyst | DC036 DC037 DC038 DC040 DC046 |
| `bus/ws_bridge.rs`             | `spawn_ws_bridge` + `v2_logged_to_v1_wire_format` — tails event_log, emits v1-compatible JSON | DC041 DC042 I004 |
| `bus/retention_cron.rs`        | `spawn_retention_cron` — daily TTL cleanup + orphan cursor sweep + RetentionReport/StaleSubscription emission | I010 |

All 5 files accounted for; no ghost files in `bus/`. `compat.rs` was deleted at Phase 8 (dual-emit tombstones gone).

---

## 7. Test files

| Test file                                                     | Count        | Scope                                           |
|---------------------------------------------------------------|--------------|-------------------------------------------------|
| `missiond-core/tests/event_chaos.rs`                          | 12 tests     | Phase 5 chaos matrix (no Docker)                |
| `missiond-core/tests/event_log_integration.rs`                | 6 (#[ignore])| Phase 2 — real PG round-trips                   |
| `missiond-core/tests/event_dispatcher_integration.rs`         | 2 (#[ignore]) + 1 compile-time | Phase 3 — 12-domain fan-out     |
| `missiond-core/tests/event_subscription_integration.rs`       | 3 (#[ignore])| Phase 4 — cursor / DLQ / replay                 |
| `missiond-daemon/tests/e2e_bus_golden_path.rs`                | 1 (#[ignore])| Phase 9 — MCP → log → WS frame (NEW)            |

Unit tests under `src/event/**/tests`: 254 total (per `cargo test -p missiond-core --lib`).

---

## 8. Ghost check — anything in code not declared by frozen lisp?

| Candidate                                    | Analysis                                                       |
|----------------------------------------------|----------------------------------------------------------------|
| `event/subscription/lifecycle.rs`            | Implementation artifact of §4.3 two-phase (bootstrap → live). Declared by lisp's `subscription-lifecycle` component — not a ghost. |
| `event/subscription/combinators.rs`          | Declared by lisp's `subscription-combinators` section. Not a ghost. |
| `event/log/retention.rs`                     | Declared by lisp §4.2.b `retention` sub-section. Not a ghost.  |
| `event/log/reader.rs`                        | Implied by §4.2.b `Consumer catch-up pull target` invariant. Not a ghost. |
| `event/dispatcher/tail.rs` / `topic.rs` / `registry.rs` | Implementation split of the one lisp `topic-dispatcher` component. Not ghosts. |
| `event/guards/causation.rs`                  | Declared by §4.4 `causation-loop-guard`. Not a ghost.          |
| `event/metrics/{mod,emitter}.rs`             | Declared by §4.4 `observability`. Not a ghost.                 |
| `event/in_memory/{log,blob_store,cursor_store,control_gate}.rs` | Declared by §4.4 `testing-story` + `decided-options :in-memory-bus`. Not ghosts. |
| `bus/v2_subscribers.rs` (daemon)             | Wiring only, outside `missiond-core::event`. Execution log DC036 anchors it. |
| `bus/ws_bridge.rs` (daemon)                  | Wiring to legacy WS contract. Execution log DC041/DC042 + I004 anchor it. |
| `bus/retention_cron.rs` (daemon)             | Cron task that invokes `event::log::retention::cleanup_once`. Execution log I010 anchor. |

No orphan modules. Every file has a lisp or execution-log anchor.

---

## 9. Missing / deferred (acknowledged gaps)

From the execution log; not code gaps but deliberate deferrals:

- **D001 — FreezeAndCatchUp**: variant exists in `PauseBehavior` enum; runtime behavior aliases `DropAndLiveResume`. Tracked by I009, optional future work.
- **D002 — Prometheus backend**: `AtomicBusMetrics` → `ObservabilityEvent::BusMetric` emission pipeline is complete; exporter HTTP endpoint not wired. Non-blocking.
- **D003 / D007 — EmbeddingEvent / AstSyncEvent**: not in 12-domain enum list; remain as worker-internal MPSC queues (1-producer/1-consumer, no persistence need). Explicitly outside bus scope per DC047 reclassification.

None of these are ghosts; none are missing components — they are annotated deferrals inside the frozen-contract boundary.

---

## Verdict

Layout matches frozen lisp §4.2 to the file level (modulo intentional directory expansion for `log/`, `dispatcher/`, `subscription/`, `blob_store/`, `in_memory/`, `guards/`, `metrics/`). No ghost modules; no missing components. Daemon-side wiring (`bus/*`) is all lisp-or-exec-log anchored.
