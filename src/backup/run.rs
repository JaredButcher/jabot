//! One backup run, and telling the owner when runs start or stop failing.

use std::path::PathBuf;
use std::sync::Arc;

use serenity::all::UserId;
use sqlx::SqlitePool;

use super::restic::{Restic, ResticError, SnapshotSummary};
use super::snapshot::{self, SnapshotError};
use crate::framework::DiscordApi;

/// Longest error text put in a DM, well inside Discord's 2000 characters.
const MAX_ERROR_CHARS: usize = 1500;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    #[error(transparent)]
    Restic(#[from] ResticError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub snapshot: SnapshotSummary,
    /// Size of the database copy, in bytes.
    pub size: u64,
    /// Whether `restic check --read-data` ran (and passed).
    pub checked: bool,
}

/// Copies the database to `staging` and uploads it with restic.
pub struct Backup {
    pool: SqlitePool,
    restic: Arc<dyn Restic>,
    /// Where the copy is made; always the same path, so restic sees one file changing.
    staging: PathBuf,
}

impl Backup {
    pub fn new(pool: SqlitePool, restic: Arc<dyn Restic>, staging: PathBuf) -> Self {
        Self {
            pool,
            restic,
            staging,
        }
    }

    pub fn restic(&self) -> &dyn Restic {
        self.restic.as_ref()
    }

    /// Snapshot, upload, prune, and with `check` verify the repository. The copy is deleted
    /// afterwards either way, leaving the database as the only plaintext copy.
    pub async fn run(&self, check: bool) -> Result<Report, BackupError> {
        let result = self.upload(check).await;
        if let Err(error) = snapshot::remove(&self.staging).await {
            tracing::warn!(%error, "could not delete the backup copy");
        }
        result
    }

    async fn upload(&self, check: bool) -> Result<Report, BackupError> {
        let size = snapshot::snapshot(&self.pool, &self.staging).await?;
        self.restic.unlock().await?;
        let snapshot = self.restic.backup(&self.staging).await?;
        self.restic.forget_and_prune().await?;
        if check {
            self.restic.check().await?;
        }
        Ok(Report {
            snapshot,
            size,
            checked: check,
        })
    }
}

/// Logs every run, and DMs the owner when backups start failing and when they work again:
/// one message per incident, not one per retry.
pub struct Alerts {
    discord: Arc<dyn DiscordApi>,
    owner: Option<UserId>,
    failing: bool,
}

impl Alerts {
    pub fn new(discord: Arc<dyn DiscordApi>, owner: Option<UserId>) -> Self {
        Self {
            discord,
            owner,
            failing: false,
        }
    }

    pub async fn failed(&mut self, error: &(dyn std::error::Error + Sync)) {
        tracing::error!(%error, "database backup failed");
        if !self.failing {
            let error: String = error.to_string().chars().take(MAX_ERROR_CHARS).collect();
            self.notify(format!(
                "⚠️ The database backup failed:\n```\n{error}\n```\nI'll keep retrying, and \
                 tell you when backups work again."
            ))
            .await;
        }
        self.failing = true;
    }

    pub async fn succeeded(&mut self, report: &Report) {
        tracing::info!(
            snapshot = %report.snapshot.id,
            size = report.size,
            data_added = report.snapshot.data_added,
            checked = report.checked,
            "database backed up"
        );
        if self.failing {
            self.notify(format!(
                "✅ Database backups work again (snapshot `{}`).",
                report.snapshot.id
            ))
            .await;
        }
        self.failing = false;
    }

    async fn notify(&self, message: String) {
        let Some(owner) = self.owner else {
            return;
        };
        if let Err(error) = self.discord.send_dm(owner, message).await {
            tracing::warn!(%error, "could not DM the bot owner about backups");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::restic::MockRestic;
    use crate::framework::{DmError, MockDiscordApi};
    use mockall::Sequence;

    fn summary() -> SnapshotSummary {
        SnapshotSummary {
            id: "4f3c2a1b".into(),
            data_added: 10,
        }
    }

    fn failure() -> ResticError {
        ResticError::Failed {
            command: "backup",
            status: "exit status: 1".into(),
            stderr: "Fatal: unable to open repository".into(),
        }
    }

    /// A restic that expects the full run in order, checking the copy exists at upload time.
    fn restic_expecting_run(check: bool, staging: PathBuf) -> MockRestic {
        let mut seq = Sequence::new();
        let mut restic = MockRestic::new();
        restic
            .expect_unlock()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        restic
            .expect_backup()
            .withf(move |path| path == staging && path.exists())
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(summary()));
        restic
            .expect_forget_and_prune()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        restic
            .expect_check()
            .times(usize::from(check))
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        restic
    }

    fn staging(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("backup/database.sqlite")
    }

    #[sqlx::test]
    async fn a_run_uploads_prunes_and_cleans_up(pool: SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let restic = restic_expecting_run(false, staging(&dir));
        let backup = Backup::new(pool, Arc::new(restic), staging(&dir));

        let report = backup.run(false).await.unwrap();

        assert_eq!(report.snapshot, summary());
        assert!(report.size > 0);
        assert!(!report.checked);
        assert!(!staging(&dir).exists());
    }

    #[sqlx::test]
    async fn a_run_with_check_verifies_the_repository(pool: SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let restic = restic_expecting_run(true, staging(&dir));
        let backup = Backup::new(pool, Arc::new(restic), staging(&dir));

        assert!(backup.run(true).await.unwrap().checked);
    }

    #[sqlx::test]
    async fn a_failed_upload_skips_pruning_and_still_cleans_up(pool: SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let mut restic = MockRestic::new();
        restic.expect_unlock().returning(|| Ok(()));
        restic.expect_backup().returning(|_| Err(failure()));
        restic.expect_forget_and_prune().times(0);
        let backup = Backup::new(pool, Arc::new(restic), staging(&dir));

        let error = backup.run(false).await.unwrap_err();

        assert!(matches!(
            error,
            BackupError::Restic(ResticError::Failed { .. })
        ));
        assert!(!staging(&dir).exists());
    }

    #[sqlx::test]
    async fn a_failed_snapshot_never_reaches_restic(pool: SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        // A file where the staging directory should be.
        std::fs::write(dir.path().join("backup"), "").unwrap();
        let backup = Backup::new(pool, Arc::new(MockRestic::new()), staging(&dir));

        let error = backup.run(false).await.unwrap_err();

        assert!(matches!(error, BackupError::Snapshot(_)));
    }

    fn report() -> Report {
        Report {
            snapshot: summary(),
            size: 100,
            checked: false,
        }
    }

    fn owner() -> UserId {
        UserId::new(5)
    }

    /// A Discord expecting DMs to the owner that satisfy `checks`, in order.
    fn discord_expecting(checks: Vec<fn(&str) -> bool>) -> MockDiscordApi {
        let mut seq = Sequence::new();
        let mut discord = MockDiscordApi::new();
        for check in checks {
            discord
                .expect_send_dm()
                .withf(move |user, text| *user == owner() && check(text))
                .times(1)
                .in_sequence(&mut seq)
                .returning(|_, _| Ok(()));
        }
        discord
    }

    #[tokio::test]
    async fn owner_hears_once_per_incident() {
        let discord = discord_expecting(vec![
            |text| text.contains("backup failed") && text.contains("unable to open repository"),
            |text| text.contains("work again") && text.contains("`4f3c2a1b`"),
            |text| text.contains("backup failed"),
        ]);
        let mut alerts = Alerts::new(Arc::new(discord), Some(owner()));

        alerts.succeeded(&report()).await;
        alerts.failed(&failure()).await;
        alerts.failed(&failure()).await;
        alerts.failed(&failure()).await;
        alerts.succeeded(&report()).await;
        alerts.succeeded(&report()).await;
        alerts.failed(&failure()).await;
    }

    #[tokio::test]
    async fn no_owner_means_no_dms() {
        let mut discord = MockDiscordApi::new();
        discord.expect_send_dm().times(0);
        let mut alerts = Alerts::new(Arc::new(discord), None);

        alerts.failed(&failure()).await;
        alerts.succeeded(&report()).await;
    }

    #[tokio::test]
    async fn long_errors_are_cut_to_fit_a_dm() {
        let mut discord = MockDiscordApi::new();
        discord
            .expect_send_dm()
            .withf(|_, text| text.chars().count() < 2000)
            .times(1)
            .returning(|_, _| Ok(()));
        let mut alerts = Alerts::new(Arc::new(discord), Some(owner()));

        let error = ResticError::Failed {
            command: "backup",
            status: "exit status: 1".into(),
            stderr: "x".repeat(5000),
        };
        alerts.failed(&error).await;
    }

    #[tokio::test]
    async fn an_undeliverable_dm_is_only_logged() {
        let mut discord = MockDiscordApi::new();
        discord
            .expect_send_dm()
            .returning(|_, _| Err(DmError::Closed));
        let mut alerts = Alerts::new(Arc::new(discord), Some(owner()));

        alerts.failed(&failure()).await;
        assert!(alerts.failing);
    }
}
