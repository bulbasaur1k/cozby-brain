CREATE TABLE IF NOT EXISTS learning_tracks (
    id                 TEXT PRIMARY KEY,
    title              TEXT NOT NULL,
    source_ref         TEXT NOT NULL,
    total_lessons      INTEGER NOT NULL DEFAULT 0,
    current_lesson     INTEGER NOT NULL DEFAULT 0,
    pace_hours         INTEGER NOT NULL DEFAULT 24,
    last_delivered_at  TIMESTAMPTZ,
    tags               TEXT[] NOT NULL DEFAULT '{}',
    created_at         TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS lessons (
    id            TEXT PRIMARY KEY,
    track_id      TEXT NOT NULL REFERENCES learning_tracks(id) ON DELETE CASCADE,
    lesson_num    INTEGER NOT NULL,
    title         TEXT NOT NULL,
    content       TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    delivered_at  TIMESTAMPTZ,
    learned_at    TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS lessons_track_idx  ON lessons (track_id, lesson_num);
CREATE INDEX IF NOT EXISTS lessons_status_idx ON lessons (status);
