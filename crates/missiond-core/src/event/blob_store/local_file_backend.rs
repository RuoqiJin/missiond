//! Local file [`BlobStore`] backend — suited for outlier payloads (>1 MB)
//! where PG BYTEA becomes a drag.
//!
//! Layout: `<root>/<first 2 hex chars of uuid>/<uuid>.bin`.  The two-char
//! prefix keeps directory entry counts bounded (256 shards).

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;
use uuid::Uuid;

use super::claim_check::{hex_encode_checksum, sha256};
use super::{BlobBackend, BlobResult, BlobStore, BlobStoreError, PayloadRef};

const URI_PREFIX: &str = "file:";

/// Local-filesystem blob store rooted at a configurable directory.
#[derive(Clone)]
pub struct LocalFileBlobStore {
    root: PathBuf,
}

impl LocalFileBlobStore {
    /// Construct a new store rooted at `root` (will be created if missing).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, id: Uuid) -> PathBuf {
        let s = id.simple().to_string();
        let prefix = &s[..2];
        self.root.join(prefix).join(format!("{s}.bin"))
    }

    fn uri_for(path: &Path) -> String {
        format!("{URI_PREFIX}{}", path.display())
    }

    fn parse_uri(uri: &str) -> BlobResult<PathBuf> {
        uri.strip_prefix(URI_PREFIX)
            .map(PathBuf::from)
            .ok_or_else(|| BlobStoreError::Other(format!("invalid local-file uri: {uri}")))
    }
}

#[async_trait]
impl BlobStore for LocalFileBlobStore {
    async fn put(&self, bytes: &[u8]) -> BlobResult<PayloadRef> {
        let id = Uuid::new_v4();
        let path = self.path_for(id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&path, bytes).await?;

        let checksum = sha256(bytes);
        Ok(PayloadRef {
            backend: BlobBackend::LocalFile,
            uri: Self::uri_for(&path),
            size: bytes.len() as u64,
            checksum,
        })
    }

    async fn get(&self, payload_ref: &PayloadRef) -> BlobResult<Vec<u8>> {
        if payload_ref.backend != BlobBackend::LocalFile {
            return Err(BlobStoreError::Other(format!(
                "LocalFileBlobStore cannot fetch {:?} blobs",
                payload_ref.backend
            )));
        }
        let path = Self::parse_uri(&payload_ref.uri)?;
        let bytes = match fs::read(&path).await {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(BlobStoreError::NotFound {
                    uri: payload_ref.uri.clone(),
                });
            }
            Err(e) => return Err(BlobStoreError::Io(e)),
        };

        let actual = sha256(&bytes);
        if actual != payload_ref.checksum {
            return Err(BlobStoreError::ChecksumMismatch {
                expected: hex_encode_checksum(&payload_ref.checksum),
                actual: hex_encode_checksum(&actual),
            });
        }
        Ok(bytes)
    }

    async fn delete(&self, payload_ref: &PayloadRef) -> BlobResult<()> {
        if payload_ref.backend != BlobBackend::LocalFile {
            return Err(BlobStoreError::Other(format!(
                "LocalFileBlobStore cannot delete {:?} blobs",
                payload_ref.backend
            )));
        }
        let path = Self::parse_uri(&payload_ref.uri)?;
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(BlobStoreError::Io(e)),
        }
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
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFileBlobStore::new(dir.path().to_path_buf());
        let payload = b"hello phase 2 storage layer".to_vec();
        let r = store.put(&payload).await.unwrap();
        assert_eq!(r.backend, BlobBackend::LocalFile);
        assert_eq!(r.size as usize, payload.len());
        let fetched = store.get(&r).await.unwrap();
        assert_eq!(fetched, payload);
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFileBlobStore::new(dir.path().to_path_buf());
        let r = store.put(b"x").await.unwrap();
        store.delete(&r).await.unwrap();
        // Second delete must not fail.
        store.delete(&r).await.unwrap();
        // After delete, get must yield NotFound.
        let err = store.get(&r).await.unwrap_err();
        assert!(matches!(err, BlobStoreError::NotFound { .. }));
    }

    #[tokio::test]
    async fn get_detects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFileBlobStore::new(dir.path().to_path_buf());
        let r = store.put(b"original").await.unwrap();
        // Corrupt the file on disk.
        let path = LocalFileBlobStore::parse_uri(&r.uri).unwrap();
        tokio::fs::write(&path, b"tampered").await.unwrap();
        let err = store.get(&r).await.unwrap_err();
        assert!(
            matches!(err, BlobStoreError::ChecksumMismatch { .. }),
            "expected checksum mismatch, got {err:?}",
        );
    }

    #[tokio::test]
    async fn backend_rejects_wrong_ref() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFileBlobStore::new(dir.path().to_path_buf());
        let bogus = PayloadRef {
            backend: BlobBackend::PgTable,
            uri: "pg:00000000-0000-0000-0000-000000000000".into(),
            size: 0,
            checksum: sha256(b""),
        };
        assert!(store.get(&bogus).await.is_err());
    }

    #[test]
    fn uri_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalFileBlobStore::new(dir.path().to_path_buf());
        let id = Uuid::new_v4();
        let path = store.path_for(id);
        let uri = LocalFileBlobStore::uri_for(&path);
        let parsed = LocalFileBlobStore::parse_uri(&uri).unwrap();
        assert_eq!(parsed, path);
    }
}
