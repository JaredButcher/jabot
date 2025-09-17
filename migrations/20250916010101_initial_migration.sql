-- Secret Santa Event Management Tables

-- Table for storing Discord users who participate in secret santa events
CREATE TABLE event_users (
    id INTEGER PRIMARY KEY NOT NULL,
    global_wish TEXT
);

-- Table for storing secret santa events
CREATE TABLE events (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    host_id INTEGER NOT NULL,
    status INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (host_id) REFERENCES event_users(id)
);

-- Table for tracking participants in specific events
CREATE TABLE event_participants (
    user_id INTEGER NOT NULL,
    event_id INTEGER NOT NULL,
    joined BOOLEAN NOT NULL DEFAULT FALSE,
    event_wish TEXT,
    assignee_id INTEGER,
    PRIMARY KEY (user_id, event_id),
    FOREIGN KEY (user_id) REFERENCES event_users(id),
    FOREIGN KEY (event_id) REFERENCES events(id),
    FOREIGN KEY (assignee_id) REFERENCES event_users(id)
);