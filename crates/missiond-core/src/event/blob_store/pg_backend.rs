//! PostgreSQL-backed [`BlobStore`] — the default for missiond single-node.
//!
//! Blobs are stored in the `blob_storage` table (migration
//! `20260419000002_blob_storage.sql`). Keeping them in the same database as
//! `event_log` means a single backup stream covers both and the log row +
//! blob insert can be verified against the same transaction boundary.

#![cfg(feature = "postgres")]

use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use super::claim_check::{hex_encode_checksum, sha256};
use super::{BlobBackend, BlobResult, BlobStore, BlobStoreError, PayloadRef};

/// URI prefix for PG-backed blobs. Concatenated with the UUID.
const URI_PREFIX: &str = "pg:";

/// PostgreSQL-backed blob store.
#[derive(Clone)]
pub struct PgBlobStore {
    pool: PgPool,
}

impl PgBlobStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn parse_uri(uri: &str) -> BlobResult<Uuid> {
        let raw = uri
            .strip_prefix(URI_PREFIX)
            .ok_or_else(|| BlobStoreError::Other(format!("invalid pg uri: {uri}")))?;
        Uuid::parse_str(raw)
            .map_err(|e| BlobStoreError::Other(format!("invalid uuid in uri {uri}: {e}")))
    }
}

#[async_trait]
impl BlobStore for PgBlobStore {
    async fn put(&self, bytes: &[u8]) -> BlobResult<PayloadRef> {
        let checksum = sha256(bytes);
        let checksum_hex = hex_encode_checksum(&checksum);
        let size: i32 = bytes
            .len()
            .try_into()
            .map_err(|_| BlobStoreError::Other(format!("blob too large: {}", bytes.len())))?;

        // Letting DB default gen_random_uuid() stamp the id so we don't
        // depend on the uuid crate at call sites.
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO blob_storage (data, size, checksum) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(bytes)
        .bind(size)
        .bind(&checksum_hex)
        .fetch_one(&self.pool)
        .await?;

        Ok(PayloadRef {
            backend: BlobBackend::PgTable,
            uri: format!("{URI_PREFIX}{id}"),
            size: bytes.len() as u64,
            checksum,
        })
    }

    async fn get(&self, payload_ref: &PayloadRef) -> BlobResult<Vec<u8>> {
        if payload_ref.backend != BlobBackend::PgTable {
            return Err(BlobStoreError::Other(format!(
                "PgBlobStore cannot fetch {:?} blobs",
                payload_ref.backend
            )));
        }
        let id = Self::parse_uri(&payload_ref.uri)?;

        let row: Option<(Vec<u8>, String)> =
            sqlx::query_as("SELECT data, checksum FROM blob_storage WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;

        let (data, stored_hex) = row.ok_or_else(|| BlobStoreError::NotFound {
            uri: payload_ref.uri.clone(),
        })?;

        // Verify the stored row vs the checksum in the ref — protects against
        // silent corruption (DB page tear, manual edits, etc.).
        let actual_hex = hex_encode_checksum(&sha256(&data));
        let expected_hex = hex_encode_checksum(&payload_ref.checksum);
        if stored_hex != expected_hex || actual_hex != expected_hex {
            return Err(BlobStoreError::ChecksumMismatch {
                expected: expected_hex,
                actual: actual_hex,
            });
        }
        Ok(data)
    }

    async fn delete(&self, payload_ref: &PayloadRef) -> BlobResult<()> {
        if payload_ref.backend != BlobBackend::PgTable {
            return Err(BlobStoreError::Other(format!(
                "PgBlobStore cannot delete {:?} blobs",
                payload_ref.backend
            )));
        }
        let id = Self::parse_uri(&payload_ref.uri)?;
        sqlx::query("DELETE FROM blob_storage WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    fn backend(&self) -> BlobBackend {
        BlobBackend::PgTable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pure helpers that don't need a live DB. End-to-end tests live in
    // `tests/event_log_integration.rs` which spins up testcontainers.

    #[test]
    fn parse_uri_round_trip() {
        let id = Uuid::new_v4();
        let uri = format!("{URI_PREFIX}{id}");
        assert_eq!(PgBlobStore::parse_uri(&uri).unwrap(), id);
    }

    #[test]
    fn parse_uri_rejects_missing_prefix() {
        assert!(PgBlobStore::parse_uri(&Uuid::new_v4().to_string()).is_err());
    }

    #[test]
    fn parse_uri_rejects_malformed() {
        assert!(PgBlobStore::parse_uri("pg:not-a-uuid").is_err());
    }
}
