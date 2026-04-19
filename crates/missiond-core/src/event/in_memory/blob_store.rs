//! In-memory [`BlobStore`] backend for tests.
//!
//! Frozen lisp §4.2.b marks in-memory blob handles as **forbidden** for
//! production ("restart = no pointer"). This file is strictly for test
//! fixtures — the chaos and unit tests need a blob store that doesn't
//! require filesystem or PG.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use uuid::Uuid;

use super::super::blob_store::{
    sha256, BlobBackend, BlobResult, BlobStore, BlobStoreError, PayloadRef,
};

/// Volatile in-memory blob store used by tests. Production must use PG or
/// local-file backends (frozen lisp §4.2.b).
#[derive(Debug, Default)]
pub struct InMemoryBlobStore {
    inner: Mutex<HashMap<String, Vec<u8>>>,
}

impl InMemoryBlobStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl BlobStore for InMemoryBlobStore {
    async fn put(&self, bytes: &[u8]) -> BlobResult<PayloadRef> {
        let uri = format!("mem:{}", Uuid::new_v4());
        let checksum = sha256(bytes);
        self.inner.lock().unwrap().insert(uri.clone(), bytes.to_vec());
        // Use LocalFile as the closest existing variant — the URI prefix
        // `mem:` tells the test code this is the in-memory backend.
        Ok(PayloadRef {
            backend: BlobBackend::LocalFile,
            uri,
            size: bytes.len() as u64,
            checksum,
        })
    }

    async fn get(&self, payload_ref: &PayloadRef) -> BlobResult<Vec<u8>> {
        let guard = self.inner.lock().unwrap();
        let bytes = guard
            .get(&payload_ref.uri)
            .cloned()
            .ok_or_else(|| BlobStoreError::NotFound {
                uri: payload_ref.uri.clone(),
            })?;
        let actual = sha256(&bytes);
        if actual != payload_ref.checksum {
            return Err(BlobStoreError::ChecksumMismatch {
                expected: super::super::blob_store::hex_encode_checksum(&payload_ref.checksum),
                actual: super::super::blob_store::hex_encode_checksum(&actual),
            });
        }
        Ok(bytes)
    }

    async fn delete(&self, payload_ref: &PayloadRef) -> BlobResult<()> {
        self.inner.lock().unwrap().remove(&payload_ref.uri);
        Ok(())
    }

    fn backend(&self) -> BlobBackend {
        BlobBackend::LocalFile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_get_round_trip() {
        let store = InMemoryBlobStore::new();
        let payload = b"hello world";
        let r = store.put(payload).await.unwrap();
        assert_eq!(r.size, payload.len() as u64);
        let back = store.get(&r).await.unwrap();
        assert_eq!(back, payload);
    }

    #[tokio::test]
    async fn get_unknown_returns_not_found() {
        let store = InMemoryBlobStore::new();
        let fake = PayloadRef {
            backend: BlobBackend::LocalFile,
            uri: "mem:missing".into(),
            size: 0,
            checksum: [0u8; 32],
        };
        let err = store.get(&fake).await.unwrap_err();
        assert!(matches!(err, BlobStoreError::NotFound { .. }));
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let store = InMemoryBlobStore::new();
        let r = store.put(b"x").await.unwrap();
        store.delete(&r).await.unwrap();
        store.delete(&r).await.unwrap(); // no error the second time
    }
}
