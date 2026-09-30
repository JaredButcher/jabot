//! The framework-owned `users` table: one row per Discord user the bot has seen. Feature
//! tables reference `users(id)`.

use async_trait::async_trait;
use serenity::all::UserId;
use sqlx::SqlitePool;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait UserRepo: Send + Sync {
    /// Insert any of `users` not already present. Idempotent.
    async fn ensure(&self, users: &[UserId]) -> Result<(), sqlx::Error>;
}

pub struct SqliteUserRepo {
    pool: SqlitePool,
}

impl SqliteUserRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepo for SqliteUserRepo {
    async fn ensure(&self, users: &[UserId]) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        for &user in users {
            let id = i64::from(user);
            sqlx::query!("INSERT OR IGNORE INTO users (id) VALUES (?)", id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test]
    async fn ensure_is_idempotent(pool: SqlitePool) {
        let repo = SqliteUserRepo::new(pool.clone());

        repo.ensure(&[UserId::new(1), UserId::new(2)])
            .await
            .unwrap();
        repo.ensure(&[UserId::new(2), UserId::new(3)])
            .await
            .unwrap();

        let ids: Vec<i64> = sqlx::query_scalar!("SELECT id FROM users ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(ids, [1, 2, 3]);
    }
}
