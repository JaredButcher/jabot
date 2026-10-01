//! tell storage (the `tell_*` tables). Handlers depend on the `TellRepo` trait so tests can
//! mock it; `SqliteTellRepo` is the real implementation.
//!
//! Every user id written here must already have a row in the shared `users` table
//! (`framework::UserRepo::ensure`).

use async_trait::async_trait;
use serenity::all::UserId;
use sqlx::SqlitePool;

use super::model::{StoredToken, TokenId};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait TellRepo: Send + Sync {
    /// The user's token, storing `candidate` as their token if they have none.
    async fn get_or_create(
        &self,
        user: UserId,
        candidate: String,
    ) -> Result<StoredToken, sqlx::Error>;
    /// Deletes token `id` if it belongs to `user`. `Ok(false)` if there was no such token.
    async fn revoke(&self, user: UserId, id: TokenId) -> Result<bool, sqlx::Error>;
    /// The owner of `token`, if it exists.
    async fn user_for_token(&self, token: &str) -> Result<Option<UserId>, sqlx::Error>;
}

pub struct SqliteTellRepo {
    pool: SqlitePool,
}

impl SqliteTellRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn user_id(id: i64) -> UserId {
    UserId::new(id as u64)
}

fn db_id(user: UserId) -> i64 {
    i64::from(user)
}

#[async_trait]
impl TellRepo for SqliteTellRepo {
    async fn get_or_create(
        &self,
        user: UserId,
        candidate: String,
    ) -> Result<StoredToken, sqlx::Error> {
        let user = db_id(user);
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "INSERT INTO tell_tokens (user_id, token) VALUES (?, ?) ON CONFLICT (user_id) DO NOTHING",
            user,
            candidate
        )
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query!("SELECT id, token FROM tell_tokens WHERE user_id = ?", user)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(StoredToken {
            id: TokenId(row.id),
            created: row.token == candidate,
            token: row.token,
        })
    }

    async fn revoke(&self, user: UserId, id: TokenId) -> Result<bool, sqlx::Error> {
        let user = db_id(user);
        let result = sqlx::query!(
            "DELETE FROM tell_tokens WHERE id = ? AND user_id = ?",
            id.0,
            user
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn user_for_token(&self, token: &str) -> Result<Option<UserId>, sqlx::Error> {
        let owner = sqlx::query_scalar!("SELECT user_id FROM tell_tokens WHERE token = ?", token)
            .fetch_optional(&self.pool)
            .await?;
        Ok(owner.map(user_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{SqliteUserRepo, UserRepo};

    async fn repo(pool: SqlitePool, users: &[u64]) -> SqliteTellRepo {
        let users: Vec<UserId> = users.iter().copied().map(UserId::new).collect();
        SqliteUserRepo::new(pool.clone())
            .ensure(&users)
            .await
            .unwrap();
        SqliteTellRepo::new(pool)
    }

    #[sqlx::test]
    async fn get_or_create_keeps_the_first_token(pool: SqlitePool) {
        let repo = repo(pool, &[1]).await;

        let first = repo
            .get_or_create(UserId::new(1), "a".into())
            .await
            .unwrap();
        let second = repo
            .get_or_create(UserId::new(1), "b".into())
            .await
            .unwrap();

        assert!(first.created);
        assert_eq!(first.token, "a");
        assert_eq!(
            second,
            StoredToken {
                id: first.id,
                token: "a".into(),
                created: false
            }
        );
    }

    #[sqlx::test]
    async fn finds_the_owner_of_a_token(pool: SqlitePool) {
        let repo = repo(pool, &[1, 2]).await;
        repo.get_or_create(UserId::new(1), "a".into())
            .await
            .unwrap();
        repo.get_or_create(UserId::new(2), "b".into())
            .await
            .unwrap();

        assert_eq!(
            repo.user_for_token("b").await.unwrap(),
            Some(UserId::new(2))
        );
        assert_eq!(repo.user_for_token("c").await.unwrap(), None);
    }

    #[sqlx::test]
    async fn revoke_needs_matching_id_and_user(pool: SqlitePool) {
        let repo = repo(pool, &[1, 2]).await;
        let token = repo
            .get_or_create(UserId::new(1), "a".into())
            .await
            .unwrap();

        assert!(!repo.revoke(UserId::new(2), token.id).await.unwrap());
        assert!(
            !repo
                .revoke(UserId::new(1), TokenId(token.id.0 + 1))
                .await
                .unwrap()
        );
        assert!(repo.revoke(UserId::new(1), token.id).await.unwrap());
        assert!(!repo.revoke(UserId::new(1), token.id).await.unwrap());
        assert_eq!(repo.user_for_token("a").await.unwrap(), None);
    }

    #[sqlx::test]
    async fn ids_are_not_reused_after_revoke(pool: SqlitePool) {
        let repo = repo(pool, &[1]).await;
        let old = repo
            .get_or_create(UserId::new(1), "a".into())
            .await
            .unwrap();
        repo.revoke(UserId::new(1), old.id).await.unwrap();

        let new = repo
            .get_or_create(UserId::new(1), "b".into())
            .await
            .unwrap();

        assert!(new.created);
        assert_ne!(new.id, old.id);
        assert!(!repo.revoke(UserId::new(1), old.id).await.unwrap());
    }
}
