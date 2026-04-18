-- Documentation module: projects → pages → versioned history + attachments.

CREATE TABLE IF NOT EXISTS projects (
    id          TEXT PRIMARY KEY,
    slug        TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    tags        TEXT[] NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS projects_slug_idx ON projects (slug);
CREATE INDEX IF NOT EXISTS projects_tags_idx ON projects USING GIN (tags);

CREATE TABLE IF NOT EXISTS doc_pages (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    slug       TEXT NOT NULL,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    version    INTEGER NOT NULL DEFAULT 1,
    tags       TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    UNIQUE (project_id, slug)
);

CREATE INDEX IF NOT EXISTS doc_pages_project_idx ON doc_pages (project_id);
CREATE INDEX IF NOT EXISTS doc_pages_title_idx   ON doc_pages (title);
CREATE INDEX IF NOT EXISTS doc_pages_tags_idx    ON doc_pages USING GIN (tags);

CREATE TABLE IF NOT EXISTS doc_page_versions (
    id         TEXT PRIMARY KEY,
    page_id    TEXT NOT NULL REFERENCES doc_pages(id) ON DELETE CASCADE,
    version    INTEGER NOT NULL,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    tags       TEXT[] NOT NULL DEFAULT '{}',
    author     TEXT NOT NULL DEFAULT 'user',
    summary    TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL,
    UNIQUE (page_id, version)
);

CREATE INDEX IF NOT EXISTS doc_page_versions_page_idx
    ON doc_page_versions (page_id, version DESC);

CREATE TABLE IF NOT EXISTS attachments (
    id           TEXT PRIMARY KEY,
    page_id      TEXT REFERENCES doc_pages(id) ON DELETE CASCADE,
    note_id      TEXT REFERENCES notes(id)     ON DELETE CASCADE,
    filename     TEXT NOT NULL,
    mime_type    TEXT NOT NULL,
    size_bytes   BIGINT NOT NULL,
    storage_key  TEXT NOT NULL,
    uploaded_at  TIMESTAMPTZ NOT NULL,
    CHECK (page_id IS NOT NULL OR note_id IS NOT NULL)
);

CREATE INDEX IF NOT EXISTS attachments_page_idx ON attachments (page_id);
CREATE INDEX IF NOT EXISTS attachments_note_idx ON attachments (note_id);
