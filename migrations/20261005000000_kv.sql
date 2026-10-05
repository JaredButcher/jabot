-- kv feature tables (feature-owned, prefixed kv_).

-- One row per stored value. Keys are per user and stored lowercased.
CREATE TABLE kv_values (
    user_id    INTEGER NOT NULL REFERENCES users(id),
    name       TEXT    NOT NULL, -- the key; `key` is an SQL keyword
    value      TEXT    NOT NULL, -- message content, verbatim
    updated_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, name)
) WITHOUT ROWID;
