//! Seq authority — PostgreSQL `BIGSERIAL` is the single source of truth for
//! event sequence numbers.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-3 commit
//! seq-authority:
//!
//! > source         "DB BIGSERIAL — 全局严格单调,单点分配"
//! > crash-recovery "DB 自己保存 max(seq),无应用层对账"
//! > invariant      "seq 只增不减;已分配 seq 终生不变"
//!
//! # Why a separate module?
//!
//! The actual seq assignment happens inside [`super::log_writer`] when the
//! INSERT returns the freshly allocated `seq`. There is no algorithm to host
//! here — the DB owns it. This module exists to give readers of the code a
//! named anchor that mirrors the lisp component, and to park the type alias
//! + invariant documentation outside the already-long `log_writer.rs`.
//!
//! If the bus ever gains a second seq authority (e.g. a distributed variant
//! with Redpanda, see frozen lisp §4.5 deferred `distributed-bus`), the
//! trait / adapter would live here and `log_writer.rs` would call it
//! instead of embedding the INSERT RETURNING semantics.
//!
//! For the in-memory parity bus ([`crate::event::in_memory::InMemoryLog`]),
//! the authority is an `AtomicI64::fetch_add` inside its writer task — same
//! invariant (monotonic, single-writer), different mechanism. See
//! `intent-event-bus-execution.lisp` DC023.

use crate::event::log::Seq;

/// The value type returned by the seq authority. Re-exported here for
/// documentation clarity; the canonical newtype lives on
/// [`crate::event::log::Seq`].
pub type EventSeq = Seq;
