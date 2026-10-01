-- tell feature tables (feature-owned, prefixed tell_).

-- One token per user. Requests to the tell endpoint authenticate with it, and it identifies
-- whom to DM.
CREATE TABLE tell_tokens (
    id         INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, -- in the Revoke button's custom id; never reused
    user_id    INTEGER NOT NULL UNIQUE REFERENCES users(id),
    token      TEXT    NOT NULL UNIQUE,
    created_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);
