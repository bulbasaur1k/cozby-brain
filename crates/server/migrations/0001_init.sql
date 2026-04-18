CREATE TABLE IF NOT EXISTS notes (
    id         TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    tags       TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS notes_title_idx ON notes (title);
CREATE INDEX IF NOT EXISTS notes_tags_idx  ON notes USING GIN (tags);
CREATE INDEX IF NOT EXISTS notes_updated_idx ON notes (updated_at DESC);
