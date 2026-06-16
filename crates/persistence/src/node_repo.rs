use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::types::Json;
use sqlx::{FromRow, PgPool};

use application::ports::{NodeRepository, RepoError};
use domain::entities::{Node, NodeMember};

pub struct PgNodeRepository {
    pool: PgPool,
}

impl PgNodeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(FromRow)]
struct NodeRow {
    id: String,
    kind: String,
    title: String,
    summary: String,
    status: String,
    tags: Vec<String>,
    metadata: Json<serde_json::Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<NodeRow> for Node {
    fn from(r: NodeRow) -> Self {
        Node {
            id: r.id,
            kind: r.kind,
            title: r.title,
            summary: r.summary,
            status: r.status,
            tags: r.tags,
            metadata: r.metadata.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(FromRow)]
struct MemberRow {
    id: String,
    node_id: String,
    member_kind: String,
    member_id: String,
    role: String,
    label: String,
    created_at: DateTime<Utc>,
}

impl From<MemberRow> for NodeMember {
    fn from(r: MemberRow) -> Self {
        NodeMember {
            id: r.id,
            node_id: r.node_id,
            member_kind: r.member_kind,
            member_id: r.member_id,
            role: r.role,
            label: r.label,
            created_at: r.created_at,
        }
    }
}

const NODE_COLS: &str =
    "id, kind, title, summary, status, tags, metadata, created_at, updated_at";
const MEMBER_COLS: &str =
    "id, node_id, member_kind, member_id, role, label, created_at";

#[async_trait]
impl NodeRepository for PgNodeRepository {
    async fn upsert(&self, node: &Node) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO nodes (id, kind, title, summary, status, tags, metadata, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (id) DO UPDATE SET
                kind       = EXCLUDED.kind,
                title      = EXCLUDED.title,
                summary    = EXCLUDED.summary,
                status     = EXCLUDED.status,
                tags       = EXCLUDED.tags,
                metadata   = EXCLUDED.metadata,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&node.id)
        .bind(&node.kind)
        .bind(&node.title)
        .bind(&node.summary)
        .bind(&node.status)
        .bind(&node.tags)
        .bind(Json(&node.metadata))
        .bind(node.created_at)
        .bind(node.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM nodes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Node>, RepoError> {
        let row: Option<NodeRow> =
            sqlx::query_as(&format!("SELECT {NODE_COLS} FROM nodes WHERE id = $1"))
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }

    async fn list(&self) -> Result<Vec<Node>, RepoError> {
        let rows: Vec<NodeRow> =
            sqlx::query_as(&format!("SELECT {NODE_COLS} FROM nodes ORDER BY updated_at DESC"))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_by_kind(&self, kind: &str) -> Result<Vec<Node>, RepoError> {
        let rows: Vec<NodeRow> = sqlx::query_as(&format!(
            "SELECT {NODE_COLS} FROM nodes WHERE kind = $1 ORDER BY updated_at DESC"
        ))
        .bind(kind)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn add_member(&self, member: &NodeMember) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO node_members (id, node_id, member_kind, member_id, role, label, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (node_id, member_kind, member_id) DO UPDATE SET
                role  = EXCLUDED.role,
                label = EXCLUDED.label
            "#,
        )
        .bind(&member.id)
        .bind(&member.node_id)
        .bind(&member.member_kind)
        .bind(&member.member_id)
        .bind(&member.role)
        .bind(&member.label)
        .bind(member.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn remove_member(&self, id: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM node_members WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn list_members(&self, node_id: &str) -> Result<Vec<NodeMember>, RepoError> {
        let rows: Vec<MemberRow> = sqlx::query_as(&format!(
            "SELECT {MEMBER_COLS} FROM node_members WHERE node_id = $1 ORDER BY created_at ASC"
        ))
        .bind(node_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn find_nodes_for_member(
        &self,
        member_kind: &str,
        member_id: &str,
    ) -> Result<Vec<Node>, RepoError> {
        let rows: Vec<NodeRow> = sqlx::query_as(&format!(
            r#"
            SELECT {NODE_COLS} FROM nodes n
            WHERE n.id IN (
                SELECT node_id FROM node_members
                WHERE member_kind = $1 AND member_id = $2
            )
            ORDER BY n.updated_at DESC
            "#
        ))
        .bind(member_kind)
        .bind(member_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}
