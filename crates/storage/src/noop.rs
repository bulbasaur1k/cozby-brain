use async_trait::async_trait;

use application::ports::{AttachmentStore, StorageError};

/// Storage stub used when MinIO/S3 is not configured.
pub struct NoopAttachmentStore;

#[async_trait]
impl AttachmentStore for NoopAttachmentStore {
    fn name(&self) -> &str {
        "noop"
    }

    async fn put(
        &self,
        _key: &str,
        _content_type: &str,
        _bytes: Vec<u8>,
    ) -> Result<String, StorageError> {
        Err(StorageError::NotConfigured)
    }

    async fn get(&self, _key: &str) -> Result<Vec<u8>, StorageError> {
        Err(StorageError::NotConfigured)
    }

    async fn delete(&self, _key: &str) -> Result<(), StorageError> {
        Err(StorageError::NotConfigured)
    }

    async fn presigned_url(
        &self,
        _key: &str,
        _ttl_secs: u64,
    ) -> Result<Option<String>, StorageError> {
        Err(StorageError::NotConfigured)
    }
}
