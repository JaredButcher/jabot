//! Secret Santa storage (the `ss_*` tables). Handlers depend on the `SecretSantaRepo` trait so
//! tests can mock it; `SqliteSecretSantaRepo` is the real implementation.
//!
//! Every user id written here must already have a row in the shared `users` table
//! (`framework::UserRepo::ensure`).

use async_trait::async_trait;
use serenity::all::UserId;
use sqlx::{QueryBuilder, SqlitePool};

use super::model::{Event, EventId, EventStatus, Participant};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait SecretSantaRepo: Send + Sync {
    async fn get_event(&self, id: EventId) -> Result<Option<Event>, sqlx::Error>;
    /// Events the user participates in (hosts are always participants).
    async fn events_for_user(&self, user: UserId) -> Result<Vec<Event>, sqlx::Error>;
    /// Events hosted by `host` that haven't finished.
    async fn count_active_hosted(&self, host: UserId) -> Result<i64, sqlx::Error>;
    /// Creates the event and adds the host as its first participant, in one transaction.
    async fn create_event(
        &self,
        host: UserId,
        name: &str,
        description: &str,
    ) -> Result<EventId, sqlx::Error>;
    async fn update_event(
        &self,
        id: EventId,
        name: &str,
        description: Option<String>,
    ) -> Result<(), sqlx::Error>;
    async fn participants(&self, id: EventId) -> Result<Vec<Participant>, sqlx::Error>;
    /// Adds and removes participants in one transaction.
    async fn set_participants(
        &self,
        id: EventId,
        add: &[UserId],
        remove: &[UserId],
    ) -> Result<(), sqlx::Error>;
    /// Moves PreRun → Running and stores every `(santa, recipient)` assignment, in one
    /// transaction. `Ok(false)` if the event was no longer PreRun, and nothing changes.
    async fn start_event(
        &self,
        id: EventId,
        assignments: &[(UserId, UserId)],
    ) -> Result<bool, sqlx::Error>;
    /// Sets the status to `to` only if it is currently one of `from`. `Ok(false)` if it wasn't.
    async fn transition(
        &self,
        id: EventId,
        from: &[EventStatus],
        to: EventStatus,
    ) -> Result<bool, sqlx::Error>;
}

pub struct SqliteSecretSantaRepo {
    pool: SqlitePool,
}

impl SqliteSecretSantaRepo {
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

fn event(
    id: i64,
    name: String,
    description: Option<String>,
    host: i64,
    status: i64,
) -> Result<Event, sqlx::Error> {
    Ok(Event {
        id: EventId(id),
        name,
        description,
        host: user_id(host),
        status: EventStatus::try_from(status).map_err(|e| sqlx::Error::Decode(Box::new(e)))?,
    })
}

#[async_trait]
impl SecretSantaRepo for SqliteSecretSantaRepo {
    async fn get_event(&self, id: EventId) -> Result<Option<Event>, sqlx::Error> {
        let row = sqlx::query!(
            "SELECT id, name, description, host_id, status FROM ss_events WHERE id = ?",
            id.0
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| event(r.id, r.name, r.description, r.host_id, r.status))
            .transpose()
    }

    async fn events_for_user(&self, user: UserId) -> Result<Vec<Event>, sqlx::Error> {
        let user = db_id(user);
        let rows = sqlx::query!(
            "SELECT e.id, e.name, e.description, e.host_id, e.status
             FROM ss_events e JOIN ss_participants ep ON e.id = ep.event_id
             WHERE ep.user_id = ?
             ORDER BY e.id",
            user
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| event(r.id, r.name, r.description, r.host_id, r.status))
            .collect()
    }

    async fn count_active_hosted(&self, host: UserId) -> Result<i64, sqlx::Error> {
        let host = db_id(host);
        let finished = i64::from(EventStatus::Finished);
        let row = sqlx::query!(
            "SELECT COUNT(*) AS count FROM ss_events WHERE host_id = ? AND status != ?",
            host,
            finished
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.count)
    }

    async fn create_event(
        &self,
        host: UserId,
        name: &str,
        description: &str,
    ) -> Result<EventId, sqlx::Error> {
        let host = db_id(host);
        let status = i64::from(EventStatus::PreRun);
        let mut tx = self.pool.begin().await?;
        let event = sqlx::query!(
            "INSERT INTO ss_events (name, description, host_id, status) VALUES (?, ?, ?, ?)",
            name,
            description,
            host,
            status
        )
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        sqlx::query!(
            "INSERT INTO ss_participants (user_id, event_id) VALUES (?, ?)",
            host,
            event
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(EventId(event))
    }

    async fn update_event(
        &self,
        id: EventId,
        name: &str,
        description: Option<String>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE ss_events SET name = ?, description = ? WHERE id = ?",
            name,
            description,
            id.0
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn participants(&self, id: EventId) -> Result<Vec<Participant>, sqlx::Error> {
        let rows = sqlx::query!(
            "SELECT user_id, assignee_id FROM ss_participants WHERE event_id = ? ORDER BY rowid",
            id.0
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| Participant {
                user: user_id(r.user_id),
                assignee: r.assignee_id.map(user_id),
            })
            .collect())
    }

    async fn set_participants(
        &self,
        id: EventId,
        add: &[UserId],
        remove: &[UserId],
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        for &user in add {
            let user = db_id(user);
            sqlx::query!(
                "INSERT OR IGNORE INTO ss_participants (user_id, event_id) VALUES (?, ?)",
                user,
                id.0
            )
            .execute(&mut *tx)
            .await?;
        }
        for &user in remove {
            let user = db_id(user);
            sqlx::query!(
                "DELETE FROM ss_participants WHERE event_id = ? AND user_id = ?",
                id.0,
                user
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }

    async fn start_event(
        &self,
        id: EventId,
        assignments: &[(UserId, UserId)],
    ) -> Result<bool, sqlx::Error> {
        let pre_run = i64::from(EventStatus::PreRun);
        let running = i64::from(EventStatus::Running);
        let mut tx = self.pool.begin().await?;
        let started = sqlx::query!(
            "UPDATE ss_events SET status = ? WHERE id = ? AND status = ?",
            running,
            id.0,
            pre_run
        )
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if !started {
            return Ok(false);
        }

        for &(santa, recipient) in assignments {
            let santa = db_id(santa);
            let recipient = db_id(recipient);
            let updated = sqlx::query!(
                "UPDATE ss_participants SET assignee_id = ? WHERE event_id = ? AND user_id = ?",
                recipient,
                id.0,
                santa
            )
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if updated != 1 {
                // Not a participant (anymore). Dropping `tx` rolls back the status change too.
                return Err(sqlx::Error::RowNotFound);
            }
        }
        tx.commit().await?;
        Ok(true)
    }

    async fn transition(
        &self,
        id: EventId,
        from: &[EventStatus],
        to: EventStatus,
    ) -> Result<bool, sqlx::Error> {
        if from.is_empty() {
            return Ok(false);
        }
        let mut query = QueryBuilder::new("UPDATE ss_events SET status = ");
        query
            .push_bind(i64::from(to))
            .push(" WHERE id = ")
            .push_bind(id.0)
            .push(" AND status IN (");
        let mut statuses = query.separated(", ");
        for &status in from {
            statuses.push_bind(i64::from(status));
        }
        statuses.push_unseparated(")");
        let result = query.build().execute(&self.pool).await?;
        Ok(result.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{SqliteUserRepo, UserRepo};

    fn user(id: u64) -> UserId {
        UserId::new(id)
    }

    /// A repo whose database already knows users 1 through 99.
    async fn setup(pool: SqlitePool) -> SqliteSecretSantaRepo {
        let known: Vec<UserId> = (1..=99).map(user).collect();
        SqliteUserRepo::new(pool.clone())
            .ensure(&known)
            .await
            .unwrap();
        SqliteSecretSantaRepo::new(pool)
    }

    async fn event_with(repo: &SqliteSecretSantaRepo, host: UserId, others: &[UserId]) -> EventId {
        let id = repo
            .create_event(host, "Party", "Bring snacks")
            .await
            .unwrap();
        repo.set_participants(id, others, &[]).await.unwrap();
        id
    }

    fn assignee_of(participants: &[Participant], santa: UserId) -> Option<UserId> {
        participants.iter().find(|p| p.user == santa)?.assignee
    }

    #[sqlx::test]
    async fn create_event_adds_host_as_participant(pool: SqlitePool) {
        let repo = setup(pool).await;

        let id = repo
            .create_event(user(1), "Party", "Bring snacks")
            .await
            .unwrap();

        let event = repo.get_event(id).await.unwrap().unwrap();
        assert_eq!(event.name, "Party");
        assert_eq!(event.description.as_deref(), Some("Bring snacks"));
        assert_eq!(event.host, user(1));
        assert_eq!(event.status, EventStatus::PreRun);
        let participants = repo.participants(id).await.unwrap();
        assert_eq!(
            participants,
            vec![Participant {
                user: user(1),
                assignee: None
            }]
        );
    }

    #[sqlx::test]
    async fn set_participants_adds_and_removes(pool: SqlitePool) {
        let repo = setup(pool).await;
        let id = event_with(&repo, user(1), &[user(2), user(3)]).await;

        repo.set_participants(id, &[user(4)], &[user(2)])
            .await
            .unwrap();

        let users: Vec<_> = repo
            .participants(id)
            .await
            .unwrap()
            .iter()
            .map(|p| p.user)
            .collect();
        assert_eq!(users, [user(1), user(3), user(4)]);
    }

    #[sqlx::test]
    async fn events_for_user_lists_only_their_events(pool: SqlitePool) {
        let repo = setup(pool).await;
        let a = event_with(&repo, user(1), &[user(2)]).await;
        let _b = event_with(&repo, user(3), &[]).await;

        let events = repo.events_for_user(user(2)).await.unwrap();

        assert_eq!(events.iter().map(|e| e.id).collect::<Vec<_>>(), [a]);
    }

    #[sqlx::test]
    async fn count_active_hosted_ignores_finished(pool: SqlitePool) {
        let repo = setup(pool).await;
        let a = event_with(&repo, user(1), &[]).await;
        event_with(&repo, user(1), &[]).await;
        repo.transition(a, &[EventStatus::PreRun], EventStatus::Finished)
            .await
            .unwrap();

        assert_eq!(repo.count_active_hosted(user(1)).await.unwrap(), 1);
    }

    /// B1: starting one event must not touch the user's assignments in other events.
    #[sqlx::test]
    async fn start_event_only_assigns_within_event(pool: SqlitePool) {
        let repo = setup(pool).await;
        let a = event_with(&repo, user(1), &[user(2)]).await;
        let b = event_with(&repo, user(3), &[user(1)]).await;
        assert!(
            repo.start_event(a, &[(user(1), user(2)), (user(2), user(1))])
                .await
                .unwrap()
        );

        assert!(
            repo.start_event(b, &[(user(3), user(1)), (user(1), user(3))])
                .await
                .unwrap()
        );

        let in_a = repo.participants(a).await.unwrap();
        assert_eq!(assignee_of(&in_a, user(1)), Some(user(2)));
        let in_b = repo.participants(b).await.unwrap();
        assert_eq!(assignee_of(&in_b, user(1)), Some(user(3)));
    }

    /// B2: a failed assignment write rolls back the status change.
    #[sqlx::test]
    async fn start_event_is_atomic(pool: SqlitePool) {
        let repo = setup(pool).await;
        let id = event_with(&repo, user(1), &[user(2)]).await;

        let not_a_participant = user(99);
        let result = repo
            .start_event(id, &[(user(1), user(2)), (not_a_participant, user(1))])
            .await;

        assert!(result.is_err());
        let event = repo.get_event(id).await.unwrap().unwrap();
        assert_eq!(event.status, EventStatus::PreRun);
        let participants = repo.participants(id).await.unwrap();
        assert!(participants.iter().all(|p| p.assignee.is_none()));
    }

    /// B5: a second start (e.g. a double click) changes nothing.
    #[sqlx::test]
    async fn start_event_twice_is_rejected(pool: SqlitePool) {
        let repo = setup(pool).await;
        let id = event_with(&repo, user(1), &[user(2)]).await;
        assert!(
            repo.start_event(id, &[(user(1), user(2)), (user(2), user(1))])
                .await
                .unwrap()
        );

        let second = repo
            .start_event(id, &[(user(1), user(1)), (user(2), user(2))])
            .await;

        assert!(!second.unwrap());
        let participants = repo.participants(id).await.unwrap();
        assert_eq!(assignee_of(&participants, user(1)), Some(user(2)));
    }

    /// B5: transitions only apply from the expected status.
    #[sqlx::test]
    async fn transition_requires_expected_status(pool: SqlitePool) {
        let repo = setup(pool).await;
        let id = event_with(&repo, user(1), &[]).await;

        let from_running = repo
            .transition(id, &[EventStatus::Running], EventStatus::Finished)
            .await;
        assert!(!from_running.unwrap());
        let from_either = repo
            .transition(
                id,
                &[EventStatus::PreRun, EventStatus::Running],
                EventStatus::Finished,
            )
            .await;
        assert!(from_either.unwrap());
        let again = repo
            .transition(id, &[EventStatus::PreRun], EventStatus::Finished)
            .await;
        assert!(!again.unwrap());
    }

    #[sqlx::test]
    async fn update_event_changes_name_and_description(pool: SqlitePool) {
        let repo = setup(pool).await;
        let id = event_with(&repo, user(1), &[]).await;

        repo.update_event(id, "New", None).await.unwrap();

        let event = repo.get_event(id).await.unwrap().unwrap();
        assert_eq!(event.name, "New");
        assert_eq!(event.description, None);
    }

    #[sqlx::test]
    async fn participants_must_be_known_users(pool: SqlitePool) {
        let repo = setup(pool).await;
        let id = event_with(&repo, user(1), &[]).await;

        let unknown = user(1000);
        let result = repo.set_participants(id, &[unknown], &[]).await;

        assert!(
            result.is_err(),
            "foreign key to users(id) should be enforced"
        );
    }

    /// B8: a corrupted status is reported, not read as PreRun.
    #[sqlx::test]
    async fn invalid_status_is_an_error(pool: SqlitePool) {
        let repo = setup(pool.clone()).await;
        let id = event_with(&repo, user(1), &[]).await;
        sqlx::query!("UPDATE ss_events SET status = 7 WHERE id = ?", id.0)
            .execute(&pool)
            .await
            .unwrap();

        assert!(matches!(
            repo.get_event(id).await,
            Err(sqlx::Error::Decode(_))
        ));
    }
}
