//! Observability domain — module entry (D001 per memory pillar v0.4.23).
//!
//! Module `system-support` + `llm-support` + `conversation-logs` (partial)
//! converge on a single `ObservabilityStore` trait that covers:
//!   - Gemini request log + file cache
//!   - Incident ledger + token ledger
//!   - Watermarks (reconcile / watcher / gemini_cli / consumer) + labels (EAV)
//!   - Backfill progress + router_chat archive
//!   - Image description cache (v0.4.23 Stage 2C.5: VisionStore merged)
//!   - Message translation cache (v0.4.23 Stage 2C.5: VisionStore merged)
//!
//! References:
//!   - Trait: `crate::db::traits::ObservabilityStore`
//!   - PG impl: `crate::db::pg::observability`
//!   - SQLite impl: `crate::db::sqlite::observability`
//!   - Shared row types: `crate::db::shared::{BackfillPhaseStatus, ...}`

#[allow(unused_imports)]
pub use crate::db::shared::BackfillPhaseStatus;
