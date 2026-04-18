//! Documentation persistence: projects, doc pages, history, attachments.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

use application::ports::{
    AttachmentRepository, DocPageHistoryRepository, DocPageRepository, ProjectRepository, RepoError,
};
use domain::entities::{Attachment, DocPage, DocPageVersion, Project};

// ---------------- Project ----------------

pub struct PgProjectRepository {
    pool: PgPool,
}

impl PgProjectRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(FromRow)]
struct ProjectRow {
    id: String,
    slug: String,
    title: String,
    description: String,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ProjectRow> for Project {
    fn from(r: ProjectRow) -> Self {
        Project {
            id: r.id,
            slug: r.slug,
            title: r.title,
            description: r.description,
            tags: r.tags,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[async_trait]
impl ProjectRepository for PgProjectRepository {
    async fn upsert(&self, p: &Project) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO projects (id, slug, title, description, tags, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (id) DO UPDATE SET
                slug = EXCLUDED.slug,
                title = EXCLUDED.title,
                description = EXCLUDED.description,
                tags = EXCLUDED.tags,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&p.id)
        .bind(&p.slug)
        .bind(&p.title)
        .bind(&p.description)
        .bind(&p.tags)
        .bind(p.created_at)
        .bind(p.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn get_by_id(&self, id: &str) -> Result<Option<Project>, RepoError> {
        let row: Option<ProjectRow> = sqlx::query_as(
            "SELECT id, slug, title, description, tags, created_at, updated_at FROM projects WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }

    async fn get_by_slug(&self, slug: &str) -> Result<Option<Project>, RepoError> {
        let row: Option<ProjectRow> = sqlx::query_as(
            "SELECT id, slug, title, description, tags, created_at, updated_at FROM projects WHERE slug = $1",
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }

    async fn find_by_title_like(&self, query: &str) -> Result<Vec<Project>, RepoError> {
        let pattern = format!("%{query}%");
        let rows: Vec<ProjectRow> = sqlx::query_as(
            r#"SELECT id, slug, title, description, tags, created_at, updated_at
               FROM projects
               WHERE title ILIKE $1 OR slug ILIKE $1
               ORDER BY updated_at DESC"#,
        )
        .bind(pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list(&self) -> Result<Vec<Project>, RepoError> {
        let rows: Vec<ProjectRow> = sqlx::query_as(
            "SELECT id, slug, title, description, tags, created_at, updated_at FROM projects ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}

// ---------------- DocPage ----------------

pub struct PgDocPageRepository {
    pool: PgPool,
}

impl PgDocPageRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(FromRow)]
struct PageRow {
    id: String,
    project_id: String,
    slug: String,
    title: String,
    content: String,
    version: i32,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<PageRow> for DocPage {
    fn from(r: PageRow) -> Self {
        DocPage {
            id: r.id,
            project_id: r.project_id,
            slug: r.slug,
            title: r.title,
            content: r.content,
            version: r.version,
            tags: r.tags,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[async_trait]
impl DocPageRepository for PgDocPageRepository {
    async fn upsert(&self, p: &DocPage) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO doc_pages (id, project_id, slug, title, content, version, tags, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (id) DO UPDATE SET
                slug = EXCLUDED.slug,
                title = EXCLUDED.title,
                content = EXCLUDED.content,
                version = EXCLUDED.version,
                tags = EXCLUDED.tags,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&p.id)
        .bind(&p.project_id)
        .bind(&p.slug)
        .bind(&p.title)
        .bind(&p.content)
        .bind(p.version)
        .bind(&p.tags)
        .bind(p.created_at)
        .bind(p.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM doc_pages WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn get_by_id(&self, id: &str) -> Result<Option<DocPage>, RepoError> {
        let row: Option<PageRow> = sqlx::query_as(
            "SELECT id, project_id, slug, title, content, version, tags, created_at, updated_at FROM doc_pages WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }

    async fn get_by_slug(
        &self,
        project_id: &str,
        slug: &str,
    ) -> Result<Option<DocPage>, RepoError> {
        let row: Option<PageRow> = sqlx::query_as(
            "SELECT id, project_id, slug, title, content, version, tags, created_at, updated_at FROM doc_pages WHERE project_id = $1 AND slug = $2",
        )
        .bind(project_id)
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }

    async fn find_by_title_like(
        &self,
        project_id: &str,
        query: &str,
    ) -> Result<Vec<DocPage>, RepoError> {
        let pattern = format!("%{query}%");
        let rows: Vec<PageRow> = sqlx::query_as(
            r#"SELECT id, project_id, slug, title, content, version, tags, created_at, updated_at
               FROM doc_pages
               WHERE project_id = $1 AND (title ILIKE $2 OR slug ILIKE $2)
               ORDER BY updated_at DESC"#,
        )
        .bind(project_id)
        .bind(pattern)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_by_project(&self, project_id: &str) -> Result<Vec<DocPage>, RepoError> {
        let rows: Vec<PageRow> = sqlx::query_as(
            "SELECT id, project_id, slug, title, content, version, tags, created_at, updated_at FROM doc_pages WHERE project_id = $1 ORDER BY updated_at DESC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}

// ---------------- DocPageVersion (history) ----------------

pub struct PgDocPageHistoryRepository {
    pool: PgPool,
}

impl PgDocPageHistoryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(FromRow)]
struct VersionRow {
    id: String,
    page_id: String,
    version: i32,
    title: String,
    content: String,
    tags: Vec<String>,
    author: String,
    summary: String,
    created_at: DateTime<Utc>,
}

impl From<VersionRow> for DocPageVersion {
    fn from(r: VersionRow) -> Self {
        DocPageVersion {
            id: r.id,
            page_id: r.page_id,
            version: r.version,
            title: r.title,
            content: r.content,
            tags: r.tags,
            author: r.author,
            summary: r.summary,
            created_at: r.created_at,
        }
    }
}

#[async_trait]
impl DocPageHistoryRepository for PgDocPageHistoryRepository {
    async fn insert(&self, v: &DocPageVersion) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO doc_page_versions (id, page_id, version, title, content, tags, author, summary, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (page_id, version) DO NOTHING
            "#,
        )
        .bind(&v.id)
        .bind(&v.page_id)
        .bind(v.version)
        .bind(&v.title)
        .bind(&v.content)
        .bind(&v.tags)
        .bind(&v.author)
        .bind(&v.summary)
        .bind(v.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn list_by_page(&self, page_id: &str) -> Result<Vec<DocPageVersion>, RepoError> {
        let rows: Vec<VersionRow> = sqlx::query_as(
            "SELECT id, page_id, version, title, content, tags, author, summary, created_at FROM doc_page_versions WHERE page_id = $1 ORDER BY version DESC",
        )
        .bind(page_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn get_version(
        &self,
        page_id: &str,
        version: i32,
    ) -> Result<Option<DocPageVersion>, RepoError> {
        let row: Option<VersionRow> = sqlx::query_as(
            "SELECT id, page_id, version, title, content, tags, author, summary, created_at FROM doc_page_versions WHERE page_id = $1 AND version = $2",
        )
        .bind(page_id)
        .bind(version)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }
}

// ---------------- Attachments ----------------

pub struct PgAttachmentRepository {
    pool: PgPool,
}

impl PgAttachmentRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(FromRow)]
struct AttRow {
    id: String,
    page_id: Option<String>,
    note_id: Option<String>,
    filename: String,
    mime_type: String,
    size_bytes: i64,
    storage_key: String,
    uploaded_at: DateTime<Utc>,
}

impl From<AttRow> for Attachment {
    fn from(r: AttRow) -> Self {
        Attachment {
            id: r.id,
            page_id: r.page_id,
            note_id: r.note_id,
            filename: r.filename,
            mime_type: r.mime_type,
            size_bytes: r.size_bytes,
            storage_key: r.storage_key,
            uploaded_at: r.uploaded_at,
        }
    }
}

#[async_trait]
impl AttachmentRepository for PgAttachmentRepository {
    async fn insert(&self, a: &Attachment) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO attachments (id, page_id, note_id, filename, mime_type, size_bytes, storage_key, uploaded_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(&a.id)
        .bind(&a.page_id)
        .bind(&a.note_id)
        .bind(&a.filename)
        .bind(&a.mime_type)
        .bind(a.size_bytes)
        .bind(&a.storage_key)
        .bind(a.uploaded_at)
        .execute(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM attachments WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Attachment>, RepoError> {
        let row: Option<AttRow> = sqlx::query_as(
            "SELECT id, page_id, note_id, filename, mime_type, size_bytes, storage_key, uploaded_at FROM attachments WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(row.map(Into::into))
    }

    async fn list_by_page(&self, page_id: &str) -> Result<Vec<Attachment>, RepoError> {
        let rows: Vec<AttRow> = sqlx::query_as(
            "SELECT id, page_id, note_id, filename, mime_type, size_bytes, storage_key, uploaded_at FROM attachments WHERE page_id = $1 ORDER BY uploaded_at DESC",
        )
        .bind(page_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}
