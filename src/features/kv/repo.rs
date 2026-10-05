//! kv storage (the `kv_*` tables). Handlers depend on the `KvRepo` trait so tests can mock it;
//! `SqliteKvRepo` is the real implementation.
//!
//! Keys arrive already normalized (see `rules::normalize_key`). Every user id written here
//! must already have a row in the shared `users` table (`framework::UserRepo::ensure`).

use async_trait::async_trait;
use serenity::all::UserId;
use sqlx::SqlitePool;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait KvRepo: Send + Sync {
    async fn get(&self, user: UserId, key: &str) -> Result<Option<String>, sqlx::Error>;
    /// Insert or replace, bumping `updated_at`.
    async fn set(&self, user: UserId, key: &str, value: &str) -> Result<(), sqlx::Error>;
    /// The user's keys containing `filter` (all of them for `None`), sorted.
    async fn keys<'a>(
        &self,
        user: UserId,
        filter: Option<&'a str>,
    ) -> Result<Vec<String>, sqlx::Error>;
    async fn count(&self, user: UserId) -> Result<i64, sqlx::Error>;
    /// `Ok(false)` if the key didn't exist.
    async fn delete(&self, user: UserId, key: &str) -> Result<bool, sqlx::Error>;
    /// Up to `limit` keys containing `partial`, those starting with it first, then
    /// alphabetical. For autocomplete.
    async fn suggest(
        &self,
        user: UserId,
        partial: &str,
        limit: i64,
    ) -> Result<Vec<String>, sqlx::Error>;
}

pub struct SqliteKvRepo {
    pool: SqlitePool,
}

impl SqliteKvRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn db_id(user: UserId) -> i64 {
    i64::from(user)
}

// `instr` rather than `LIKE`, so `%` and `_` in a query match literally.
#[async_trait]
impl KvRepo for SqliteKvRepo {
    async fn get(&self, user: UserId, key: &str) -> Result<Option<String>, sqlx::Error> {
        let user = db_id(user);
        sqlx::query_scalar!(
            "SELECT value FROM kv_values WHERE user_id = ? AND name = ?",
            user,
            key
        )
        .fetch_optional(&self.pool)
        .await
    }

    async fn set(&self, user: UserId, key: &str, value: &str) -> Result<(), sqlx::Error> {
        let user = db_id(user);
        sqlx::query!(
            "INSERT INTO kv_values (user_id, name, value) VALUES (?, ?, ?)
             ON CONFLICT (user_id, name)
             DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
            user,
            key,
            value
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn keys<'a>(
        &self,
        user: UserId,
        filter: Option<&'a str>,
    ) -> Result<Vec<String>, sqlx::Error> {
        let user = db_id(user);
        sqlx::query_scalar!(
            "SELECT name FROM kv_values
             WHERE user_id = ?1 AND (?2 IS NULL OR instr(name, ?2) > 0)
             ORDER BY name",
            user,
            filter
        )
        .fetch_all(&self.pool)
        .await
    }

    async fn count(&self, user: UserId) -> Result<i64, sqlx::Error> {
        let user = db_id(user);
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!: i64" FROM kv_values WHERE user_id = ?"#,
            user
        )
        .fetch_one(&self.pool)
        .await
    }

    async fn delete(&self, user: UserId, key: &str) -> Result<bool, sqlx::Error> {
        let user = db_id(user);
        let result = sqlx::query!(
            "DELETE FROM kv_values WHERE user_id = ? AND name = ?",
            user,
            key
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn suggest(
        &self,
        user: UserId,
        partial: &str,
        limit: i64,
    ) -> Result<Vec<String>, sqlx::Error> {
        let user = db_id(user);
        sqlx::query_scalar!(
            "SELECT name FROM kv_values
             WHERE user_id = ?1 AND (?2 = '' OR instr(name, ?2) > 0)
             ORDER BY ?2 != '' AND instr(name, ?2) != 1, name
             LIMIT ?3",
            user,
            partial,
            limit
        )
        .fetch_all(&self.pool)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{SqliteUserRepo, UserRepo};

    async fn repo(pool: SqlitePool) -> SqliteKvRepo {
        SqliteUserRepo::new(pool.clone())
            .ensure(&[UserId::new(1), UserId::new(2)])
            .await
            .unwrap();
        SqliteKvRepo::new(pool)
    }

    async fn with_keys(pool: SqlitePool, keys: &[&str]) -> SqliteKvRepo {
        let repo = repo(pool).await;
        for key in keys {
            repo.set(UserId::new(1), key, "v").await.unwrap();
        }
        repo
    }

    #[sqlx::test]
    async fn set_inserts_then_replaces(pool: SqlitePool) {
        let repo = repo(pool.clone()).await;
        let user = UserId::new(1);

        repo.set(user, "pasta", "**old**").await.unwrap();
        sqlx::query!("UPDATE kv_values SET updated_at = '2000-01-01 00:00:00'")
            .execute(&pool)
            .await
            .unwrap();
        repo.set(user, "pasta", "new\n- list").await.unwrap();

        assert_eq!(
            repo.get(user, "pasta").await.unwrap().as_deref(),
            Some("new\n- list")
        );
        let updated_at = sqlx::query_scalar!("SELECT updated_at FROM kv_values")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_ne!(updated_at, "2000-01-01 00:00:00");
        assert_eq!(repo.get(user, "other").await.unwrap(), None);
    }

    #[sqlx::test]
    async fn users_have_separate_keys(pool: SqlitePool) {
        let repo = repo(pool).await;
        repo.set(UserId::new(1), "pasta", "mine").await.unwrap();
        repo.set(UserId::new(2), "pasta", "theirs").await.unwrap();
        repo.set(UserId::new(2), "pizza", "theirs").await.unwrap();

        assert_eq!(
            repo.get(UserId::new(1), "pasta").await.unwrap().as_deref(),
            Some("mine")
        );
        assert_eq!(repo.keys(UserId::new(1), None).await.unwrap(), ["pasta"]);
        assert_eq!(repo.count(UserId::new(1)).await.unwrap(), 1);
        assert_eq!(repo.count(UserId::new(2)).await.unwrap(), 2);
    }

    #[sqlx::test]
    async fn keys_match_substrings_literally(pool: SqlitePool) {
        let repo = with_keys(pool, &["pasta", "spam", "50%", "a_b", "axb", "zed"]).await;
        let user = UserId::new(1);

        assert_eq!(
            repo.keys(user, None).await.unwrap(),
            ["50%", "a_b", "axb", "pasta", "spam", "zed"]
        );
        assert_eq!(
            repo.keys(user, Some("pa")).await.unwrap(),
            ["pasta", "spam"]
        );
        assert_eq!(repo.keys(user, Some("%")).await.unwrap(), ["50%"]);
        assert_eq!(repo.keys(user, Some("_")).await.unwrap(), ["a_b"]);
        assert!(repo.keys(user, Some("nothing")).await.unwrap().is_empty());
    }

    #[sqlx::test]
    async fn delete_reports_whether_the_key_existed(pool: SqlitePool) {
        let repo = with_keys(pool, &["pasta"]).await;
        let user = UserId::new(1);

        assert!(repo.delete(user, "pasta").await.unwrap());
        assert!(!repo.delete(user, "pasta").await.unwrap());
        assert_eq!(repo.get(user, "pasta").await.unwrap(), None);
    }

    #[sqlx::test]
    async fn suggest_puts_prefix_matches_first(pool: SqlitePool) {
        let repo = with_keys(pool, &["spam", "pasta", "apart", "paella", "zed"]).await;
        let user = UserId::new(1);

        assert_eq!(
            repo.suggest(user, "pa", 25).await.unwrap(),
            ["paella", "pasta", "apart", "spam"]
        );
        assert_eq!(
            repo.suggest(user, "pa", 2).await.unwrap(),
            ["paella", "pasta"]
        );
        assert_eq!(
            repo.suggest(user, "", 3).await.unwrap(),
            ["apart", "paella", "pasta"]
        );
        assert!(
            repo.suggest(UserId::new(2), "", 25)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
