//! DocActor — manages projects, pages, and version history.
//!
//! Fuzzy resolution: given user-readable project/page names from LLM, resolves
//! to actual entities or auto-creates them. Handles 4 operations per write:
//! create / append / section / replace — each stored in history.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use application::ports::{DocPageHistoryRepository, DocPageRepository, ProjectRepository};
use domain::entities::{DocPage, DocPageVersion, Project};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocOp {
    Create,
    Append,
    Replace,
    Section,
}

pub enum DocMsg {
    // Projects
    CreateProject(
        String,      // slug (or empty — derived from title)
        String,      // title
        String,      // description
        Vec<String>, // tags
        RpcReplyPort<Result<Project, String>>,
    ),
    ListProjects(RpcReplyPort<Vec<Project>>),
    GetProject(String, RpcReplyPort<Option<Project>>),
    /// Fuzzy lookup by human-readable name (title / slug substring).
    ResolveProject(String, RpcReplyPort<Option<Project>>),
    DeleteProject(String, RpcReplyPort<Result<(), String>>),

    // Pages
    ListPages(
        String, // project_id
        RpcReplyPort<Vec<DocPage>>,
    ),
    GetPage(String, RpcReplyPort<Option<DocPage>>),
    GetPageBySlug(
        String, // project_id
        String, // slug
        RpcReplyPort<Option<DocPage>>,
    ),
    DeletePage(String, RpcReplyPort<Result<(), String>>),
    /// Global keyword search across all projects (title OR content ILIKE).
    SearchPages(
        String, // query
        usize,  // limit
        RpcReplyPort<Vec<DocPage>>,
    ),

    // History
    ListPageHistory(String, RpcReplyPort<Vec<DocPageVersion>>),
    GetPageVersion(
        String, // page_id
        i32,    // version
        RpcReplyPort<Option<DocPageVersion>>,
    ),

    // Ingest (from LLM classifier)
    /// Apply a doc update. Auto-creates project/page if missing.
    /// Returns the resulting page (current version after the operation).
    IngestDoc(
        String,         // project hint (slug or title)
        String,         // page hint (slug or title)
        String,         // content
        Vec<String>,    // tags
        DocOp,          // operation
        Option<String>, // section_title (for op=Section)
        String,         // author ("user" / "llm" / "system")
        RpcReplyPort<Result<DocPage, String>>,
    ),
}

pub struct DocActor {
    pub project_repo: Arc<dyn ProjectRepository>,
    pub page_repo: Arc<dyn DocPageRepository>,
    pub history_repo: Arc<dyn DocPageHistoryRepository>,
}

impl Actor for DocActor {
    type Msg = DocMsg;
    /// Cache of project_id → Project. Pages are not cached (may be large).
    type State = HashMap<String, Project>;
    type Arguments = ();

    async fn pre_start(
        &self,
        _: ActorRef<Self::Msg>,
        _: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let all = self.project_repo.list().await.unwrap_or_default();
        let mut map = HashMap::with_capacity(all.len());
        for p in all {
            map.insert(p.id.clone(), p);
        }
        tracing::info!(count = map.len(), "doc actor: loaded projects");
        Ok(map)
    }

    async fn handle(
        &self,
        _: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            DocMsg::CreateProject(slug, title, description, tags, reply) => {
                let result = self
                    .create_project_impl(slug, title, description, tags, state)
                    .await;
                let _ = reply.send(result);
            }
            DocMsg::ListProjects(reply) => {
                let mut all: Vec<Project> = state.values().cloned().collect();
                all.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                let _ = reply.send(all);
            }
            DocMsg::GetProject(id, reply) => {
                let _ = reply.send(state.get(&id).cloned());
            }
            DocMsg::ResolveProject(hint, reply) => {
                let resolved = self.resolve_project_fuzzy(&hint, state).await;
                let _ = reply.send(resolved);
            }
            DocMsg::DeleteProject(id, reply) => {
                let result = match self.project_repo.delete(&id).await {
                    Ok(()) => {
                        state.remove(&id);
                        Ok(())
                    }
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(result);
            }
            DocMsg::ListPages(project_id, reply) => {
                let pages = self
                    .page_repo
                    .list_by_project(&project_id)
                    .await
                    .unwrap_or_default();
                let _ = reply.send(pages);
            }
            DocMsg::GetPage(id, reply) => {
                let p = self.page_repo.get_by_id(&id).await.unwrap_or(None);
                let _ = reply.send(p);
            }
            DocMsg::GetPageBySlug(pid, slug, reply) => {
                let p = self.page_repo.get_by_slug(&pid, &slug).await.unwrap_or(None);
                let _ = reply.send(p);
            }
            DocMsg::DeletePage(id, reply) => {
                let result = self
                    .page_repo
                    .delete(&id)
                    .await
                    .map_err(|e| e.to_string());
                let _ = reply.send(result);
            }
            DocMsg::SearchPages(query, limit, reply) => {
                let pages = self
                    .page_repo
                    .search_all(&query, limit)
                    .await
                    .unwrap_or_default();
                let _ = reply.send(pages);
            }
            DocMsg::ListPageHistory(page_id, reply) => {
                let versions = self
                    .history_repo
                    .list_by_page(&page_id)
                    .await
                    .unwrap_or_default();
                let _ = reply.send(versions);
            }
            DocMsg::GetPageVersion(page_id, version, reply) => {
                let v = self
                    .history_repo
                    .get_version(&page_id, version)
                    .await
                    .unwrap_or(None);
                let _ = reply.send(v);
            }
            DocMsg::IngestDoc(proj, page, content, tags, op, section_title, author, reply) => {
                let result = self
                    .ingest_doc_impl(&proj, &page, content, tags, op, section_title, &author, state)
                    .await;
                let _ = reply.send(result);
            }
        }
        Ok(())
    }
}

impl DocActor {
    async fn create_project_impl(
        &self,
        slug: String,
        title: String,
        description: String,
        tags: Vec<String>,
        state: &mut HashMap<String, Project>,
    ) -> Result<Project, String> {
        let slug = if slug.is_empty() {
            slugify(&title)
        } else {
            slugify(&slug)
        };
        if slug.is_empty() {
            return Err("slug cannot be empty".into());
        }
        // If exists — return existing
        if let Some(existing) = state.values().find(|p| p.slug == slug) {
            return Ok(existing.clone());
        }
        let p = Project::new(slug, title, description, tags);
        self.project_repo
            .upsert(&p)
            .await
            .map_err(|e| e.to_string())?;
        state.insert(p.id.clone(), p.clone());
        tracing::info!(id = %p.id, slug = %p.slug, "project created");
        Ok(p)
    }

    /// Fuzzy: exact slug → exact title → ILIKE title/slug.
    async fn resolve_project_fuzzy(
        &self,
        hint: &str,
        state: &HashMap<String, Project>,
    ) -> Option<Project> {
        let slug = slugify(hint);
        // 1. Exact slug
        if let Some(p) = state.values().find(|p| p.slug == slug) {
            return Some(p.clone());
        }
        // 2. Exact title (case-insensitive)
        let lower = hint.to_lowercase();
        if let Some(p) = state
            .values()
            .find(|p| p.title.to_lowercase() == lower)
        {
            return Some(p.clone());
        }
        // 3. DB ILIKE fallback (covers edge cases not in cache)
        if let Ok(list) = self.project_repo.find_by_title_like(hint).await {
            if let Some(p) = list.into_iter().next() {
                return Some(p);
            }
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    async fn ingest_doc_impl(
        &self,
        project_hint: &str,
        page_hint: &str,
        content: String,
        tags: Vec<String>,
        op: DocOp,
        section_title: Option<String>,
        author: &str,
        state: &mut HashMap<String, Project>,
    ) -> Result<DocPage, String> {
        // 1. Resolve or create project
        let project = match self.resolve_project_fuzzy(project_hint, state).await {
            Some(p) => p,
            None => {
                self.create_project_impl(
                    slugify(project_hint),
                    project_hint.to_string(),
                    String::new(),
                    Vec::new(),
                    state,
                )
                .await?
            }
        };

        // 2. Resolve page — exact slug → title fuzzy → none
        let page_slug_hint = slugify(page_hint);
        let existing = self
            .page_repo
            .get_by_slug(&project.id, &page_slug_hint)
            .await
            .map_err(|e| e.to_string())?;
        let existing = if existing.is_some() {
            existing
        } else {
            // try fuzzy by title within project
            let matches = self
                .page_repo
                .find_by_title_like(&project.id, page_hint)
                .await
                .map_err(|e| e.to_string())?;
            matches.into_iter().next()
        };

        // 3. Apply operation
        let page = match (existing, op.clone()) {
            (None, _) | (Some(_), DocOp::Create) => {
                // Create new page (ignore `existing` if op=Create — allow new page with same title? Actually let's reject if same slug exists)
                let page_slug = if page_slug_hint.is_empty() {
                    slugify(page_hint)
                } else {
                    page_slug_hint
                };
                let page = DocPage::new(
                    project.id.clone(),
                    page_slug,
                    page_hint.to_string(),
                    content,
                    tags,
                );
                self.page_repo
                    .upsert(&page)
                    .await
                    .map_err(|e| e.to_string())?;
                tracing::info!(
                    page_id = %page.id,
                    project = %project.slug,
                    "doc page created"
                );
                page
            }
            (Some(existing), op) => {
                // Save snapshot of OLD content to history before modifying
                let summary = match &op {
                    DocOp::Append => "append".to_string(),
                    DocOp::Replace => "replace".to_string(),
                    DocOp::Section => format!(
                        "section: {}",
                        section_title.as_deref().unwrap_or("")
                    ),
                    DocOp::Create => "duplicate-create".to_string(),
                };
                let version_snapshot =
                    DocPageVersion::from_page(&existing, author.to_string(), summary);
                self.history_repo
                    .insert(&version_snapshot)
                    .await
                    .map_err(|e| e.to_string())?;

                let mut updated = existing.clone();
                updated.version = existing.version + 1;
                updated.updated_at = Utc::now();

                match op {
                    DocOp::Append => {
                        updated.content =
                            format!("{}\n\n{}", existing.content.trim_end(), content.trim());
                    }
                    DocOp::Section => {
                        let heading = section_title
                            .as_deref()
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "Section".to_string());
                        updated.content = format!(
                            "{}\n\n## {}\n\n{}",
                            existing.content.trim_end(),
                            heading,
                            content.trim()
                        );
                    }
                    DocOp::Replace | DocOp::Create => {
                        updated.content = content;
                    }
                }
                // Merge tags (union)
                for t in tags {
                    if !updated.tags.contains(&t) {
                        updated.tags.push(t);
                    }
                }
                self.page_repo
                    .upsert(&updated)
                    .await
                    .map_err(|e| e.to_string())?;
                tracing::info!(
                    page_id = %updated.id,
                    project = %project.slug,
                    version = updated.version,
                    op = ?op,
                    "doc page updated"
                );
                updated
            }
        };
        Ok(page)
    }
}

/// Lowercase, spaces → `-`, strip non-alphanumeric/cyrillic.
/// Keeps Cyrillic letters so Russian slugs are readable.
pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = false;
    for c in s.trim().chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            last_dash = false;
        } else if (c.is_whitespace() || matches!(c, '-' | '_' | '/' | '.')) && !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}
