use async_trait::async_trait;

use application::ports::{RepoError, SimilarNote, VectorStore};

/// Always returns empty results. Used when Qdrant is not configured.
pub struct NoopVectorStore;

#[async_trait]
impl VectorStore for NoopVectorStore {
    async fn upsert(
        &self,
        _id: &str,
        _vector: Vec<f32>,
        _title: &str,
        _tags: &[String],
    ) -> Result<(), RepoError> {
        Ok(())
    }

    async fn search(
        &self,
        _vector: Vec<f32>,
        _limit: usize,
    ) -> Result<Vec<SimilarNote>, RepoError> {
        Ok(vec![])
    }

    async fn delete(&self, _id: &str) -> Result<(), RepoError> {
        Ok(())
    }
}
