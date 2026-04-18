//! BlobStore — Claim-Check for large event payloads.
//!
//! Frozen lisp `.missiond/v2/intent-event-bus.lisp` §4.2.b claim-check.
//!
//! Two backends:
//!   * [`pg_backend::PgBlobStore`]   — default, stores into the `blob_storage`
//!     table in the same PostgreSQL instance as the event_log.
//!   * [`local_file_backend::LocalFileBlobStore`] — for unusually large
//!     payloads (>1 MB) where a BYTEA column is inefficient.
//!
//! Both implementations share a single [`BlobStore`] trait so the LogWriter
//! stays backend-agnostic.

pub mod claim_check;
pub mod local_file_backend;
pub mod pg_backend;

pub use claim_check::{CLAIM_CHECK_THRESHOLD, hex_decode_checksum, hex_encode_checksum, sha256};
pub use local_file_backend::LocalFileBlobStore;
pub use pg_backend::PgBlobStore;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Which backend physically stored a blob. Encoded inside [`PayloadRef`] so
/// the LogWriter/reader know where to look without probing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlobBackend {
    /// PostgreSQL `blob_storage` table — default.
    #[serde(rename = "pg_table")]
    PgTable,
    /// Local file system under `.missiond/blobs/`.
    #[serde(rename = "local_file")]
    LocalFile,
}

/// Durable handle stored in `event_log.payload_ref`. Captures enough
/// information to locate and verify the payload later.
///
/// On-the-wire JSON shape (`serde_json::to_string`):
/// ```json
/// {"backend":"pg_table","uri":"pg:...","size":12345,"checksum":"<64 hex chars>"}
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadRef {
    pub backend: BlobBackend,
    pub uri: String,
    pub size: u64,
    pub checksum: [u8; 32],
}

/// serde shim that keeps `checksum` hex-encoded on the wire.
#[derive(Serialize, Deserialize)]
struct PayloadRefWire<'a> {
    backend: BlobBackend,
    uri: std::borrow::Cow<'a, str>,
    size: u64,
    checksum: std::borrow::Cow<'a, str>,
}

impl Serialize for PayloadRef {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let wire = PayloadRefWire {
            backend: self.backend,
            uri: std::borrow::Cow::Borrowed(&self.uri),
            size: self.size,
            checksum: std::borrow::Cow::Owned(hex_encode_checksum(&self.checksum)),
        };
        wire.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for PayloadRef {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let wire = PayloadRefWire::deserialize(de)?;
        let checksum = hex_decode_checksum(&wire.checksum)
            .ok_or_else(|| serde::de::Error::custom("invalid hex checksum"))?;
        Ok(Self {
            backend: wire.backend,
            uri: wire.uri.into_owned(),
            size: wire.size,
            checksum,
        })
    }
}

impl PayloadRef {
    /// Encode to the compact JSON string written into `event_log.payload_ref`.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).expect("PayloadRef serialization is infallible")
    }

    /// Parse the string previously returned by [`Self::to_json_string`].
    pub fn from_json_str(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// Structured errors returned by any [`BlobStore`] implementation.
#[derive(Debug, thiserror::Error)]
pub enum BlobStoreError {
    #[error("blob not found: uri={uri}")]
    NotFound { uri: String },

    #[error("checksum mismatch: expected={expected}, actual={actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("db: {0}")]
    #[cfg(feature = "postgres")]
    Db(#[from] sqlx::Error),

    #[error("{0}")]
    Other(String),
}

pub type BlobResult<T> = Result<T, BlobStoreError>;

/// Backend-agnostic interface the LogWriter uses to push/pull large payloads.
///
/// Implementations must be `Send + Sync` and `Clone` (the handle is cheap —
/// typically an `Arc` internally). The `put` / `get` contract:
///
/// * `put(bytes)` stores the full payload, computes sha256, returns a
///   [`PayloadRef`] that includes the URI + the checksum.
/// * `get(payload_ref)` returns the raw bytes previously stored. Implementations
///   MUST verify the checksum before returning to catch silent corruption.
/// * `delete(payload_ref)` removes the blob if still present (idempotent).
#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Upload the provided bytes and return a durable pointer.
    async fn put(&self, bytes: &[u8]) -> BlobResult<PayloadRef>;

    /// Fetch the full payload previously stored under `payload_ref`.
    async fn get(&self, payload_ref: &PayloadRef) -> BlobResult<Vec<u8>>;

    /// Idempotent delete — returns `Ok(())` even if the blob is already gone.
    async fn delete(&self, payload_ref: &PayloadRef) -> BlobResult<()>;

    /// Which backend kind this store writes to — useful for metrics.
    fn backend(&self) -> BlobBackend;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_ref_json_round_trip() {
        let r = PayloadRef {
            backend: BlobBackend::PgTable,
            uri: "pg:11111111-1111-1111-1111-111111111111".into(),
            size: 42,
            checksum: sha256(b"hello"),
        };
        let s = r.to_json_string();
        assert!(s.contains("pg_table"));
        let back = PayloadRef::from_json_str(&s).expect("parse");
        assert_eq!(r, back);
    }

    #[test]
    fn payload_ref_serializes_checksum_as_hex() {
        let r = PayloadRef {
            backend: BlobBackend::LocalFile,
            uri: "file:.missiond/blobs/ab/cd".into(),
            size: 1,
            checksum: sha256(b"x"),
        };
        let s = r.to_json_string();
        // 64 hex chars inside double-quotes
        assert!(s.contains("\"checksum\":\""), "missing hex checksum: {s}");
    }

    #[test]
    fn payload_ref_reject_invalid_checksum() {
        let bad = r#"{"backend":"pg_table","uri":"pg:x","size":1,"checksum":"not hex"}"#;
        assert!(PayloadRef::from_json_str(bad).is_err());
    }
}
