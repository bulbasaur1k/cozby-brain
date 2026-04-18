CREATE TABLE IF NOT EXISTS todos (
    id           TEXT PRIMARY KEY,
    title        TEXT NOT NULL,
    done         BOOLEAN NOT NULL DEFAULT FALSE,
    due_at       TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS todos_done_idx ON todos (done);
CREATE INDEX IF NOT EXISTS todos_due_idx  ON todos (due_at);

CREATE TABLE IF NOT EXISTS reminders (
    id         TEXT PRIMARY KEY,
    text       TEXT NOT NULL,
    remind_at  TIMESTAMPTZ NOT NULL,
    fired      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS reminders_pending_idx ON reminders (fired, remind_at);
