-- Nodes: композитные сущности («узлы»), связывающие несколько записей
-- (note/todo/reminder/doc_page/link) в один смысловой объект. Целевой кейс —
-- дежурство (duty) / инцидент: узел собирает runbook-доки, ссылки, задачи.

CREATE TABLE IF NOT EXISTS nodes (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,                  -- 'duty' | 'incident' | 'topic' | 'project' | 'release'
    title      TEXT NOT NULL,
    summary    TEXT NOT NULL DEFAULT '',       -- LLM-сжатое описание узла (для match / RAG)
    status     TEXT NOT NULL DEFAULT 'active', -- 'active' | 'resolved' | 'archived'
    tags       TEXT[] NOT NULL DEFAULT '{}',
    metadata   JSONB NOT NULL DEFAULT '{}',    -- произвольные поля (project, oncall, severity, ...)
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS nodes_kind_idx   ON nodes (kind);
CREATE INDEX IF NOT EXISTS nodes_status_idx ON nodes (status);
CREATE INDEX IF NOT EXISTS nodes_tags_idx   ON nodes USING GIN (tags);

CREATE TABLE IF NOT EXISTS node_members (
    id          TEXT PRIMARY KEY,
    node_id     TEXT NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    member_kind TEXT NOT NULL,           -- 'note' | 'todo' | 'reminder' | 'doc_page' | 'link'
    member_id   TEXT NOT NULL,           -- id сущности; для 'link' — сам URL
    role        TEXT NOT NULL DEFAULT '',-- 'runbook' | 'dashboard' | 'contact' | 'action' | ''
    label       TEXT NOT NULL DEFAULT '',-- человекочитаемая подпись (для link/доков)
    created_at  TIMESTAMPTZ NOT NULL,
    UNIQUE (node_id, member_kind, member_id)
);

CREATE INDEX IF NOT EXISTS node_members_node_idx   ON node_members (node_id);
CREATE INDEX IF NOT EXISTS node_members_member_idx ON node_members (member_kind, member_id);
