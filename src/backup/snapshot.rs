//! A consistent copy of the live database, taken while the bot runs.

use std::io;
use std::path::Path;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection, SqlitePool};

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("could not prepare {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("VACUUM INTO failed: {0}")]
    Vacuum(#[source] sqlx::Error),
    #[error("could not check the copy: {0}")]
    Open(#[source] sqlx::Error),
    #[error("the copy failed its integrity check: {0}")]
    Corrupt(String),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> SnapshotError + '_ {
    move |source| SnapshotError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Copy the database behind `pool` to `target` with `VACUUM INTO`, replacing any earlier copy,
/// then check the copy. Returns its size in bytes.
///
/// `VACUUM INTO` reads in one transaction, so the copy is consistent even while the bot writes.
/// Writers wait for it, which takes milliseconds at this database's size.
pub async fn snapshot(pool: &SqlitePool, target: &Path) -> Result<u64, SnapshotError> {
    if let Some(dir) = target.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(io_error(dir))?;
    }
    // VACUUM INTO refuses to overwrite an existing file.
    remove(target).await?;
    sqlx::query("VACUUM INTO ?")
        .bind(target.to_string_lossy())
        .execute(pool)
        .await
        .map_err(SnapshotError::Vacuum)?;
    check_integrity(target).await?;
    let size = tokio::fs::metadata(target)
        .await
        .map_err(io_error(target))?
        .len();
    Ok(size)
}

/// `PRAGMA integrity_check` on the file at `path`, opened read-only.
pub async fn check_integrity(path: &Path) -> Result<(), SnapshotError> {
    let options = SqliteConnectOptions::new().filename(path).read_only(true);
    let mut connection = SqliteConnection::connect_with(&options)
        .await
        .map_err(SnapshotError::Open)?;
    let result: Result<Vec<String>, _> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(&mut connection)
        .await;
    let _ = connection.close().await;
    // A badly damaged file fails the pragma itself ("database disk image is malformed").
    let problems = result.map_err(|error| SnapshotError::Corrupt(error.to_string()))?;
    if problems == ["ok"] {
        Ok(())
    } else {
        Err(SnapshotError::Corrupt(problems.join("; ")))
    }
}

/// Delete the copy, if there is one.
pub async fn remove(target: &Path) -> Result<(), SnapshotError> {
    match tokio::fs::remove_file(target).await {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(io_error(target)(error)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom, Write};

    /// Users 1 to `users`, in one statement.
    async fn fill(pool: &SqlitePool, users: i64) {
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?)
             INSERT INTO users (id) SELECT i FROM n",
        )
        .bind(users)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn count_users(path: &Path) -> i64 {
        let options = SqliteConnectOptions::new().filename(path).read_only(true);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&mut connection)
            .await
            .unwrap()
    }

    #[sqlx::test]
    async fn copies_the_data_into_a_new_directory(pool: SqlitePool) {
        fill(&pool, 3).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("backup/database.sqlite");

        let size = snapshot(&pool, &target).await.unwrap();

        assert!(size > 0);
        assert_eq!(count_users(&target).await, 3);
    }

    #[sqlx::test]
    async fn replaces_an_earlier_copy(pool: SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("database.sqlite");
        fill(&pool, 1).await;
        snapshot(&pool, &target).await.unwrap();

        sqlx::query!("INSERT INTO users (id) VALUES (99)")
            .execute(&pool)
            .await
            .unwrap();
        snapshot(&pool, &target).await.unwrap();

        assert_eq!(count_users(&target).await, 2);
    }

    #[sqlx::test]
    async fn damaged_copy_fails_the_check(pool: SqlitePool) {
        // Enough rows for several pages, so damage lands inside the data, not just the header.
        fill(&pool, 2000).await;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("database.sqlite");
        snapshot(&pool, &target).await.unwrap();

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&target)
            .unwrap();
        let length = file.metadata().unwrap().len();
        file.seek(SeekFrom::Start(length / 2)).unwrap();
        file.write_all(&[0xFF; 4096]).unwrap();
        drop(file);

        assert!(matches!(
            check_integrity(&target).await,
            Err(SnapshotError::Corrupt(_))
        ));
    }

    #[tokio::test]
    async fn removing_a_missing_copy_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        remove(&dir.path().join("nothing.sqlite")).await.unwrap();
    }
}
