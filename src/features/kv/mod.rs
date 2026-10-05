//! kv: a per-user key-value store. `/k set <key>` stores the user's next DM to the bot under
//! the key; `/k get [key]` shows it again, or lists keys.

mod repo;

pub use repo::{KvRepo, SqliteKvRepo};
