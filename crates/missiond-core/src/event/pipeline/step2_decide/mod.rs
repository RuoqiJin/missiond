//! Pipeline step 2 · decide — payload placement (claim-check) and
//! persistence policy (ephemeral vs durable).
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2 step-2 decide:
//!
//! > 决定 payload 存放方式 + 是否持久化
//!
//! Two sub-components:
//!
//! * [`claim_check`] — threshold constants + SHA-256/hex helpers. Payloads
//!   ≤ [`claim_check::CLAIM_CHECK_THRESHOLD`] (8 KB) go inline into
//!   `event_log.payload_inline`; larger payloads are uploaded to a
//!   [`crate::event::blob_store::BlobStore`] and only a
//!   [`crate::event::blob_store::PayloadRef`] lands in the row.
//! * [`persistence_policy`] — classify the append as `Durable` (30-day TTL)
//!   or `Ephemeral` (3-day TTL) based on the `AppendOpts::ephemeral` flag.
//!   Observability / self-emitted events default to ephemeral
//!   (frozen lisp §4.4).
//!
//! The physical blob backends (`PgBlobStore`, `LocalFileBlobStore`,
//! `InMemoryBlobStore`) stay under [`crate::event::blob_store`] — they are
//! services this step **uses**, not part of the step itself.

pub mod claim_check;
pub mod persistence_policy;

pub use claim_check::{hex_decode_checksum, hex_encode_checksum, sha256, CLAIM_CHECK_THRESHOLD};
pub use persistence_policy::PersistencePolicy;
