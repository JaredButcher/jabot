//! Daily off-site backups: the bot copies its database with `VACUUM INTO`, then uploads the
//! copy with restic (encrypted and compressed) to the repository in `RESTIC_REPOSITORY`.
//!
//! `run_scheduler` runs beside the Discord client for as long as the bot does; `Backup::run`
//! is one backup, also used by `jabot backup`.

mod restic;
mod run;
mod schedule;
mod snapshot;

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use restic::{Restic, ResticCli, ResticError, SnapshotSummary};
pub use run::{Alerts, Backup, BackupError, Report};
use schedule::Schedule;
pub use snapshot::{SnapshotError, check_integrity, snapshot};

/// Default daily time, in UTC: 4 a.m. Eastern in summer, 3 a.m. in winter.
const DEFAULT_TIME: &str = "08:00";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupConfig {
    /// Seconds since midnight UTC.
    pub time_of_day: u64,
    /// Where the database copy is made before upload.
    pub staging: PathBuf,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("BACKUP_TIME_UTC must be a time like 08:00, not {0:?}")]
    Time(String),
}

impl BackupConfig {
    /// From `RESTIC_REPOSITORY` and `BACKUP_TIME_UTC`. `None` if `RESTIC_REPOSITORY` is unset or
    /// empty: backups are off. The copy is made in `backup/` under the working directory, which
    /// is the data directory in the container.
    pub fn from_env() -> Result<Option<Self>, ConfigError> {
        let var = |name| std::env::var(name).ok();
        let staging = std::env::current_dir()
            .unwrap_or_default()
            .join("backup/database.sqlite");
        Self::from_vars(var("RESTIC_REPOSITORY"), var("BACKUP_TIME_UTC"), staging)
    }

    fn from_vars(
        repository: Option<String>,
        time: Option<String>,
        staging: PathBuf,
    ) -> Result<Option<Self>, ConfigError> {
        if repository.is_none_or(|repository| repository.trim().is_empty()) {
            return Ok(None);
        }
        let time = time
            .filter(|time| !time.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_TIME.to_string());
        let time_of_day = schedule::parse_time_of_day(&time).ok_or(ConfigError::Time(time))?;
        Ok(Some(Self {
            time_of_day,
            staging,
        }))
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Back up daily at the configured time, forever. Checks the repository at startup, and
/// catches up if the latest daily run was missed. Never returns; errors go to `alerts`.
pub async fn run_scheduler(config: BackupConfig, backup: Backup, mut alerts: Alerts) {
    let mut schedule = Schedule::new(config.time_of_day);
    // The startup check: listing snapshots needs the repository, the key and the password.
    let latest = match backup.restic().latest_snapshot().await {
        Ok(latest) => latest,
        Err(error) => {
            alerts.failed(&error).await;
            None
        }
    };
    let mut next = schedule.first_run(unix_now(), latest);
    loop {
        let wait = next.saturating_sub(unix_now());
        tracing::info!(in_minutes = wait / 60, "next database backup");
        tokio::time::sleep(Duration::from_secs(wait)).await;

        let result = backup.run(schedule::is_sunday(unix_now())).await;
        match &result {
            Ok(report) => alerts.succeeded(report).await,
            Err(error) => alerts.failed(error).await,
        }
        next = schedule.after_run(unix_now(), result.is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(
        repository: Option<&str>,
        time: Option<&str>,
    ) -> Result<Option<BackupConfig>, ConfigError> {
        BackupConfig::from_vars(
            repository.map(str::to_string),
            time.map(str::to_string),
            "/data/backup/database.sqlite".into(),
        )
    }

    #[test]
    fn backups_are_off_without_a_repository() {
        assert_eq!(config(None, Some("09:00")), Ok(None));
        assert_eq!(config(Some(" "), None), Ok(None));
    }

    #[test]
    fn time_defaults_to_eight_utc() {
        for time in [None, Some("")] {
            let enabled = config(Some("s3:https://example.com/bucket"), time)
                .unwrap()
                .unwrap();
            assert_eq!(enabled.time_of_day, 8 * 3600);
            assert_eq!(
                enabled.staging,
                PathBuf::from("/data/backup/database.sqlite")
            );
        }
    }

    #[test]
    fn time_is_configurable_and_checked() {
        let config_at = |time| config(Some("s3:x"), Some(time));
        assert_eq!(
            config_at("21:30").unwrap().unwrap().time_of_day,
            21 * 3600 + 30 * 60
        );
        assert_eq!(config_at("9pm"), Err(ConfigError::Time("9pm".into())));
    }
}
