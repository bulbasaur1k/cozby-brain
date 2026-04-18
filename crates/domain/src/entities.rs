use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Note {
    pub fn new(title: String, content: String, tags: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            content,
            tags,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_update(mut self, title: String, content: String, tags: Vec<String>) -> Self {
        self.title = title;
        self.content = content;
        self.tags = tags;
        self.updated_at = Utc::now();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub title: String,
    pub done: bool,
    pub due_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl Todo {
    pub fn new(title: String, due_at: Option<DateTime<Utc>>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            done: false,
            due_at,
            created_at: Utc::now(),
            completed_at: None,
        }
    }

    pub fn complete(mut self) -> Self {
        self.done = true;
        self.completed_at = Some(Utc::now());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub id: String,
    pub text: String,
    pub remind_at: DateTime<Utc>,
    pub fired: bool,
    pub created_at: DateTime<Utc>,
}

impl Reminder {
    pub fn new(text: String, remind_at: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            text,
            remind_at,
            fired: false,
            created_at: Utc::now(),
        }
    }
}

// ---------------- Learning ----------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LessonStatus {
    Pending,
    Delivered,
    Learned,
    Skipped,
}

impl LessonStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            LessonStatus::Pending => "pending",
            LessonStatus::Delivered => "delivered",
            LessonStatus::Learned => "learned",
            LessonStatus::Skipped => "skipped",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "delivered" => Some(Self::Delivered),
            "learned" => Some(Self::Learned),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningTrack {
    pub id: String,
    pub title: String,
    pub source_ref: String,
    pub total_lessons: i32,
    pub current_lesson: i32,
    pub pace_hours: i32,
    pub last_delivered_at: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl LearningTrack {
    pub fn new(title: String, source_ref: String, pace_hours: i32, tags: Vec<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            source_ref,
            total_lessons: 0,
            current_lesson: 0,
            pace_hours,
            last_delivered_at: None,
            tags,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub id: String,
    pub track_id: String,
    pub lesson_num: i32,
    pub title: String,
    pub content: String,
    pub status: LessonStatus,
    pub delivered_at: Option<DateTime<Utc>>,
    pub learned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Lesson {
    pub fn new(track_id: String, lesson_num: i32, title: String, content: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            track_id,
            lesson_num,
            title,
            content,
            status: LessonStatus::Pending,
            delivered_at: None,
            learned_at: None,
            created_at: Utc::now(),
        }
    }
}

// ---------------- Documentation ----------------

/// Top-level documentation namespace. E.g. "cozby-brain", "personal-finance".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    /// Short url-safe identifier — unique. E.g. "cozby-brain".
    pub slug: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Project {
    pub fn new(slug: String, title: String, description: String, tags: Vec<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            slug,
            title,
            description,
            tags,
            created_at: now,
            updated_at: now,
        }
    }
}

/// A page of documentation within a project. Markdown content + version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocPage {
    pub id: String,
    pub project_id: String,
    /// Unique slug within project. E.g. "architecture", "api-reference".
    pub slug: String,
    pub title: String,
    /// Current markdown content (latest version).
    pub content: String,
    /// Monotonic version — increments on every edit.
    pub version: i32,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DocPage {
    pub fn new(
        project_id: String,
        slug: String,
        title: String,
        content: String,
        tags: Vec<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            project_id,
            slug,
            title,
            content,
            version: 1,
            tags,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Snapshot of a DocPage before an edit. Stored so we can view history and
/// restore. Lightweight: stores content-before, edit author, and optional
/// summary of what changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocPageVersion {
    pub id: String,
    pub page_id: String,
    /// Version number of the CONTENT stored here (= previous page.version).
    pub version: i32,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    /// Who/what created this revision: "user", "llm", "system".
    pub author: String,
    /// Short summary of the change applied to get to the NEXT version.
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

impl DocPageVersion {
    pub fn from_page(page: &DocPage, author: String, summary: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            page_id: page.id.clone(),
            version: page.version,
            title: page.title.clone(),
            content: page.content.clone(),
            tags: page.tags.clone(),
            author,
            summary,
            created_at: Utc::now(),
        }
    }
}

/// Attachment metadata. The blob lives in MinIO/S3, `storage_key` is the path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    /// Linked to either a doc page OR a note (one of these is set).
    pub page_id: Option<String>,
    pub note_id: Option<String>,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub storage_key: String,
    pub uploaded_at: DateTime<Utc>,
}

impl Attachment {
    pub fn new(
        filename: String,
        mime_type: String,
        size_bytes: i64,
        storage_key: String,
        page_id: Option<String>,
        note_id: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            page_id,
            note_id,
            filename,
            mime_type,
            size_bytes,
            storage_key,
            uploaded_at: Utc::now(),
        }
    }
}
