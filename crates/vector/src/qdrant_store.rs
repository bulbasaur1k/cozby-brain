//! Qdrant vector store — gRPC adapter via `qdrant-client`.

use async_trait::async_trait;
use qdrant_client::qdrant::{
    point_id, value, CreateCollectionBuilder, DeletePointsBuilder, Distance, PointStruct,
    PointsIdsList, SearchPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::OnceCell;

use application::ports::{RepoError, SimilarNote, VectorStore};

pub struct QdrantVectorStore {
    client: Arc<Qdrant>,
    collection: String,
    initialized: OnceCell<()>,
}

impl QdrantVectorStore {
    pub fn new(client: Arc<Qdrant>, collection: String) -> Self {
        Self {
            client,
            collection,
            initialized: OnceCell::new(),
        }
    }

    async fn ensure_collection(&self, dimension: u64) -> Result<(), RepoError> {
        self.initialized
            .get_or_try_init(|| async {
                if self
                    .client
                    .collection_exists(&self.collection)
                    .await
                    .unwrap_or(false)
                {
                    tracing::debug!(collection = %self.collection, "qdrant collection exists");
                    return Ok(());
                }
                tracing::info!(collection = %self.collection, dimension, "creating qdrant collection");
                self.client
                    .create_collection(
                        CreateCollectionBuilder::new(&self.collection)
                            .vectors_config(VectorParamsBuilder::new(dimension, Distance::Cosine)),
                    )
                    .await
                    .map_err(|e| RepoError::Vector(e.to_string()))?;
                Ok(())
            })
            .await
            .copied()
    }
}

#[async_trait]
impl VectorStore for QdrantVectorStore {
    async fn upsert(
        &self,
        id: &str,
        vector: Vec<f32>,
        title: &str,
        tags: &[String],
    ) -> Result<(), RepoError> {
        self.ensure_collection(vector.len() as u64).await?;
        let payload = json!({ "title": title, "tags": tags });
        let payload_map: std::collections::HashMap<String, qdrant_client::qdrant::Value> = payload
            .as_object()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect();
        let point = PointStruct::new(id.to_string(), vector, payload_map);
        self.client
            .upsert_points(UpsertPointsBuilder::new(&self.collection, vec![point]).wait(true))
            .await
            .map_err(|e| RepoError::Vector(e.to_string()))?;
        Ok(())
    }

    async fn search(
        &self,
        vector: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<SimilarNote>, RepoError> {
        if !self
            .client
            .collection_exists(&self.collection)
            .await
            .unwrap_or(false)
        {
            return Ok(vec![]);
        }
        let results = self
            .client
            .search_points(
                SearchPointsBuilder::new(&self.collection, vector, limit as u64)
                    .with_payload(true)
                    .score_threshold(0.5_f32),
            )
            .await
            .map_err(|e| RepoError::Vector(e.to_string()))?;
        Ok(results
            .result
            .into_iter()
            .map(|hit| {
                let title = hit
                    .payload
                    .get("title")
                    .and_then(|v| match &v.kind {
                        Some(value::Kind::StringValue(s)) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let id = match hit.id {
                    Some(pid) => match pid.point_id_options {
                        Some(point_id::PointIdOptions::Uuid(u)) => u,
                        Some(point_id::PointIdOptions::Num(n)) => n.to_string(),
                        None => String::new(),
                    },
                    None => String::new(),
                };
                SimilarNote {
                    id,
                    title,
                    score: hit.score,
                }
            })
            .collect())
    }

    async fn delete(&self, id: &str) -> Result<(), RepoError> {
        if !self
            .client
            .collection_exists(&self.collection)
            .await
            .unwrap_or(false)
        {
            return Ok(());
        }
        let ids = PointsIdsList {
            ids: vec![id.to_string().into()],
        };
        self.client
            .delete_points(
                DeletePointsBuilder::new(&self.collection)
                    .points(ids)
                    .wait(true),
            )
            .await
            .map_err(|e| RepoError::Vector(e.to_string()))?;
        Ok(())
    }
}
