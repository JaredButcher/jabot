-- Framework-owned: one row per Discord user the bot has seen.
CREATE TABLE users (
    id         INTEGER PRIMARY KEY NOT NULL, -- Discord user id
    created_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Secret Santa tables (feature-owned, prefixed ss_).

-- Secret Santa-specific per-user data.
CREATE TABLE ss_users (
    user_id     INTEGER PRIMARY KEY NOT NULL REFERENCES users(id),
    global_wish TEXT
);

CREATE TABLE ss_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    name        TEXT    NOT NULL,
    description TEXT,
    host_id     INTEGER NOT NULL REFERENCES users(id),
    status      INTEGER NOT NULL DEFAULT 0 -- 0 preparing, 1 running, 2 finished
);

CREATE TABLE ss_participants (
    event_id    INTEGER NOT NULL REFERENCES ss_events(id) ON DELETE CASCADE,
    user_id     INTEGER NOT NULL REFERENCES users(id),
    event_wish  TEXT,
    assignee_id INTEGER REFERENCES users(id), -- who this participant gives a gift to
    PRIMARY KEY (event_id, user_id)
);
