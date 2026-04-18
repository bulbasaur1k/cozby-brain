//! S3 / MinIO attachment store via `rust-s3`.
//!
//! Works with any S3-compatible endpoint. Bucket must be pre-created
//! (use MinIO console or `mc mb local/bucket`).

use async_trait::async_trait;
use s3::creds::Credentials;
use s3::{Bucket, Region};

use application::ports::{AttachmentStore, StorageError};

pub struct S3AttachmentStore {
    bucket: Bucket,
}

impl S3AttachmentStore {
    pub fn new(
        endpoint_url: String,
        region_name: String,
        access_key: &str,
        secret_key: &str,
        bucket_name: String,
    ) -> Result<Self, StorageError> {
        let region = Region::Custom {
            region: region_name,
            endpoint: endpoint_url,
        };
        let creds = Credentials::new(Some(access_key), Some(secret_key), None, None, None)
            .map_err(|e| StorageError::Other(format!("credentials: {e}")))?;
        let bucket = Bucket::new(&bucket_name, region, creds)
            .map_err(|e| StorageError::Other(format!("bucket init: {e}")))?
            .with_path_style();
        Ok(Self { bucket })
    }

    /// Probe bucket existence (does not auto-create). Returns Ok regardless —
    /// real errors will surface on first put. Bucket must exist beforehand.
    pub async fn ensure_bucket(&self) -> Result<(), StorageError> {
        match self.bucket.head_object("/__cozby_probe").await {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::debug!(bucket = %self.bucket.name(), error = %e, "bucket probe (non-fatal)");
                Ok(())
            }
        }
    }
}

#[async_trait]
impl AttachmentStore for S3AttachmentStore {
    fn name(&self) -> &str {
        "s3"
    }

    async fn put(
        &self,
        key: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<String, StorageError> {
        self.bucket
            .put_object_with_content_type(key, &bytes, content_type)
            .await
            .map_err(|e| StorageError::Transport(e.to_string()))?;
        Ok(key.to_string())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        let resp = self.bucket.get_object(key).await.map_err(|e| {
            let msg = e.to_string();
            if msg.contains("NoSuchKey") || msg.contains("404") {
                StorageError::NotFound
            } else {
                StorageError::Transport(msg)
            }
        })?;
        Ok(resp.bytes().to_vec())
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.bucket
            .delete_object(key)
            .await
            .map_err(|e| StorageError::Transport(e.to_string()))?;
        Ok(())
    }

    async fn presigned_url(
        &self,
        key: &str,
        ttl_secs: u64,
    ) -> Result<Option<String>, StorageError> {
        let url = self
            .bucket
            .presign_get(key, ttl_secs as u32, None)
            .await
            .map_err(|e| StorageError::Transport(e.to_string()))?;
        Ok(Some(url))
    }
}
