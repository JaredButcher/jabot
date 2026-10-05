//! restic, run as a child process. It reads its own settings from the environment the bot
//! inherits: `RESTIC_REPOSITORY`, `RESTIC_PASSWORD_FILE`, `AWS_SHARED_CREDENTIALS_FILE`,
//! `RESTIC_COMPRESSION`, `RESTIC_CACHE_DIR`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;

/// Every snapshot is made under this host name. The container's own changes with every
/// recreate, which would split the snapshots into separate series.
pub const HOST: &str = "jabot";
/// Tag of the daily snapshots; `forget` applies only to these.
pub const TAG: &str = "scheduled";
/// How many snapshots `forget` keeps. This is also how long deleted data survives in backups.
const KEEP: [&str; 6] = [
    "--keep-daily",
    "7",
    "--keep-weekly",
    "4",
    "--keep-monthly",
    "6",
];
/// The longest any restic command may take before it's killed.
const TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Lines of stderr kept for an error message.
const STDERR_LINES: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum ResticError {
    #[error("could not run restic: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("restic {command} timed out after {} minutes", .after.as_secs() / 60)]
    Timeout {
        command: &'static str,
        after: Duration,
    },
    #[error("restic {command} failed ({status}): {stderr}")]
    Failed {
        command: &'static str,
        status: String,
        stderr: String,
    },
    #[error("unexpected output from restic {command}: {detail}")]
    Output {
        command: &'static str,
        detail: String,
    },
}

/// What `restic backup` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotSummary {
    pub id: String,
    /// New data stored in the repository, before compression.
    pub data_added: u64,
}

impl SnapshotSummary {
    /// The first 8 characters of the id, as restic itself shows them.
    pub fn short_id(&self) -> &str {
        self.id.get(..8).unwrap_or(&self.id)
    }
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait Restic: Send + Sync {
    /// Unix time of the newest scheduled snapshot, if any. Fails if the repository, the key or
    /// the password is wrong, so it doubles as the startup check.
    async fn latest_snapshot(&self) -> Result<Option<u64>, ResticError>;
    /// Remove stale locks left by a run that was killed.
    async fn unlock(&self) -> Result<(), ResticError>;
    async fn backup(&self, path: &Path) -> Result<SnapshotSummary, ResticError>;
    async fn forget_and_prune(&self) -> Result<(), ResticError>;
    /// Verify the repository, reading all of its data.
    async fn check(&self) -> Result<(), ResticError>;
}

/// The `restic` binary on `PATH`.
pub struct ResticCli {
    program: PathBuf,
    /// Arguments before restic's own; tests run a script through `sh`.
    prefix: Vec<OsString>,
    timeout: Duration,
}

impl Default for ResticCli {
    fn default() -> Self {
        Self::new()
    }
}

impl ResticCli {
    pub fn new() -> Self {
        Self {
            program: "restic".into(),
            prefix: vec![],
            timeout: TIMEOUT,
        }
    }

    /// Run `command` with `args`, returning stdout. A non-zero exit or the timeout is an error;
    /// the child is killed when the timeout drops it.
    async fn run(&self, command: &'static str, args: &[OsString]) -> Result<String, ResticError> {
        let child = Command::new(&self.program)
            .args(&self.prefix)
            .arg(command)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(ResticError::Spawn)?;
        let output = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| ResticError::Timeout {
                command,
                after: self.timeout,
            })?
            .map_err(ResticError::Spawn)?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(ResticError::Failed {
                command,
                status: output.status.to_string(),
                stderr: stderr_tail(&String::from_utf8_lossy(&output.stderr)),
            })
        }
    }
}

fn args<const N: usize>(args: [&str; N]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[async_trait]
impl Restic for ResticCli {
    async fn latest_snapshot(&self) -> Result<Option<u64>, ResticError> {
        let command = "snapshots";
        let stdout = self
            .run(
                command,
                &args(["--json", "--host", HOST, "--tag", TAG, "--latest", "1"]),
            )
            .await?;
        latest_time(&stdout).map_err(|detail| ResticError::Output { command, detail })
    }

    async fn unlock(&self) -> Result<(), ResticError> {
        self.run("unlock", &[]).await.map(drop)
    }

    async fn backup(&self, path: &Path) -> Result<SnapshotSummary, ResticError> {
        let command = "backup";
        let mut arguments = args(["--json", "--host", HOST, "--tag", TAG]);
        arguments.push(path.into());
        let stdout = self.run(command, &arguments).await?;
        backup_summary(&stdout).map_err(|detail| ResticError::Output { command, detail })
    }

    async fn forget_and_prune(&self) -> Result<(), ResticError> {
        let mut arguments = args(["--host", HOST, "--tag", TAG, "--prune"]);
        arguments.extend(args(KEEP));
        self.run("forget", &arguments).await.map(drop)
    }

    async fn check(&self) -> Result<(), ResticError> {
        self.run("check", &args(["--read-data"])).await.map(drop)
    }
}

/// The last few non-empty lines of stderr, on one line.
fn stderr_tail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let tail = &lines[lines.len().saturating_sub(STDERR_LINES)..];
    if tail.is_empty() {
        "no error output".to_string()
    } else {
        tail.join(" / ")
    }
}

/// The newest `time` in `restic snapshots --json` output.
fn latest_time(stdout: &str) -> Result<Option<u64>, String> {
    #[derive(serde::Deserialize)]
    struct Snapshot {
        time: String,
    }
    let snapshots: Vec<Snapshot> = serde_json::from_str(stdout).map_err(|e| e.to_string())?;
    let mut latest = None;
    for snapshot in snapshots {
        let time = parse_rfc3339(&snapshot.time)
            .ok_or_else(|| format!("unreadable snapshot time {:?}", snapshot.time))?;
        latest = latest.max(Some(time));
    }
    Ok(latest)
}

/// The `summary` line of `restic backup --json` output, which is one JSON object per line.
fn backup_summary(stdout: &str) -> Result<SnapshotSummary, String> {
    #[derive(serde::Deserialize)]
    struct Message {
        message_type: String,
        snapshot_id: Option<String>,
        #[serde(default)]
        data_added: u64,
    }
    let summary = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Message>(line).ok())
        .find(|message| message.message_type == "summary")
        .ok_or("no summary")?;
    Ok(SnapshotSummary {
        id: summary
            .snapshot_id
            .ok_or("the summary has no snapshot id")?,
        data_added: summary.data_added,
    })
}

/// Unix seconds of an RFC 3339 time as restic writes it, e.g.
/// `2026-10-05T08:00:01.123456789-04:00` or `...Z`. Fractions of a second are dropped.
fn parse_rfc3339(s: &str) -> Option<u64> {
    let number = |start: usize, end: usize| -> Option<i64> {
        let digits = s.get(start..end)?;
        digits
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| digits.parse().ok())?
    };
    let separator =
        |at: usize, allowed: &[u8]| allowed.contains(s.as_bytes().get(at)?).then_some(());
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    separator(4, b"-")?;
    separator(7, b"-")?;
    separator(10, b"Tt ")?;
    separator(13, b":")?;
    separator(16, b":")?;
    let valid = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && hour < 24
        && minute < 60
        && second < 61;
    if !valid {
        return None;
    }

    let mut rest = s.get(19..)?;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ if rest.len() == 6 && rest.as_bytes()[3] == b':' => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hours: i64 = rest.get(1..3)?.parse().ok()?;
            let minutes: i64 = rest.get(4..6)?.parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
        _ => return None,
    };

    let seconds =
        days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    u64::try_from(seconds).ok()
}

/// Days from 1970-01-01 to the given date (proleptic Gregorian), after Howard Hinnant's
/// `days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// A fake restic: a shell script run through `sh` (so it's never exec'd directly), which
    /// records its arguments and replays `stdout`, `stderr` and `status` files from its
    /// directory.
    struct Fake {
        dir: tempfile::TempDir,
    }

    impl Fake {
        fn new() -> Self {
            Self::with_script(
                r#"d=$(dirname "$0")
printf '%s\n' "$*" >> "$d/args"
[ -f "$d/stdout" ] && cat "$d/stdout"
[ -f "$d/stderr" ] && cat "$d/stderr" >&2
exit "$(cat "$d/status" 2>/dev/null || echo 0)"
"#,
            )
        }

        fn with_script(script: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("restic.sh"), script).unwrap();
            Self { dir }
        }

        fn cli(&self, timeout: Duration) -> ResticCli {
            ResticCli {
                program: "sh".into(),
                prefix: vec![self.dir.path().join("restic.sh").into()],
                timeout,
            }
        }

        fn reply(self, file: &str, content: &str) -> Self {
            std::fs::write(self.dir.path().join(file), content).unwrap();
            self
        }

        fn args(&self) -> String {
            std::fs::read_to_string(self.dir.path().join("args")).unwrap()
        }
    }

    const MINUTE: Duration = Duration::from_secs(60);

    #[tokio::test]
    async fn latest_snapshot_reads_the_newest_time() {
        let fake = Fake::new().reply(
            "stdout",
            r#"[{"time":"2026-10-04T08:00:03.5Z","id":"a"},{"time":"2026-10-05T04:00:01.123456789-04:00","id":"b"}]"#,
        );

        let latest = fake.cli(MINUTE).latest_snapshot().await.unwrap();

        assert_eq!(latest, parse_rfc3339("2026-10-05T08:00:01Z"));
        assert_eq!(
            fake.args(),
            "snapshots --json --host jabot --tag scheduled --latest 1\n"
        );
    }

    #[tokio::test]
    async fn no_snapshots_yet() {
        let fake = Fake::new().reply("stdout", "[]");
        assert_eq!(fake.cli(MINUTE).latest_snapshot().await.unwrap(), None);
    }

    #[tokio::test]
    async fn backup_returns_the_summary() {
        let fake = Fake::new().reply(
            "stdout",
            r#"{"message_type":"status","percent_done":0.5}
{"message_type":"summary","snapshot_id":"4f3c2a1b","data_added":1234,"total_files_processed":1}
"#,
        );

        let summary = fake
            .cli(MINUTE)
            .backup(Path::new("/data/backup/database.sqlite"))
            .await
            .unwrap();

        assert_eq!(
            summary,
            SnapshotSummary {
                id: "4f3c2a1b".into(),
                data_added: 1234
            }
        );
        assert_eq!(
            fake.args(),
            "backup --json --host jabot --tag scheduled /data/backup/database.sqlite\n"
        );
    }

    #[test]
    fn short_ids_are_eight_characters() {
        let summary = |id: &str| SnapshotSummary {
            id: id.into(),
            data_added: 0,
        };
        assert_eq!(summary("436f51c11a42e191420f").short_id(), "436f51c1");
        assert_eq!(summary("abc").short_id(), "abc");
    }

    #[tokio::test]
    async fn backup_without_a_summary_is_an_error() {
        let fake = Fake::new().reply("stdout", r#"{"message_type":"status"}"#);
        let result = fake.cli(MINUTE).backup(Path::new("/x")).await;
        assert!(matches!(
            result,
            Err(ResticError::Output {
                command: "backup",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn forget_check_and_unlock_command_lines() {
        let fake = Fake::new();
        let cli = fake.cli(MINUTE);

        cli.forget_and_prune().await.unwrap();
        cli.check().await.unwrap();
        cli.unlock().await.unwrap();

        assert_eq!(
            fake.args(),
            "forget --host jabot --tag scheduled --prune --keep-daily 7 --keep-weekly 4 --keep-monthly 6\n\
             check --read-data\n\
             unlock\n"
        );
    }

    #[tokio::test]
    async fn failure_reports_the_end_of_stderr() {
        let fake = Fake::new().reply("status", "1").reply(
            "stderr",
            "one\ntwo\n\nthree\nfour\nfive\nFatal: wrong password or no key found\n",
        );

        let error = fake.cli(MINUTE).unlock().await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "restic unlock failed (exit status: 1): two / three / four / five / \
             Fatal: wrong password or no key found"
        );
    }

    #[tokio::test]
    async fn a_hung_restic_is_killed_at_the_timeout() {
        let fake = Fake::with_script("sleep 30\n");
        let started = Instant::now();

        let error = fake
            .cli(Duration::from_millis(200))
            .unlock()
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            ResticError::Timeout {
                command: "unlock",
                ..
            }
        ));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn missing_binary_is_a_spawn_error() {
        let cli = ResticCli {
            program: "/nonexistent/restic".into(),
            prefix: vec![],
            timeout: MINUTE,
        };
        assert!(matches!(cli.unlock().await, Err(ResticError::Spawn(_))));
    }

    #[test]
    fn parses_restic_times() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-10-05T08:00:00Z"), Some(1_791_187_200));
        assert_eq!(
            parse_rfc3339("2026-10-05T04:00:00.987654321-04:00"),
            Some(1_791_187_200)
        );
        assert_eq!(
            parse_rfc3339("2026-10-05T13:30:00+05:30"),
            Some(1_791_187_200)
        );
        // Leap day, and a date before March (the algorithm's year boundary).
        assert_eq!(parse_rfc3339("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(parse_rfc3339("2025-01-01T00:00:00Z"), Some(1_735_689_600));
    }

    #[test]
    fn rejects_malformed_times() {
        for bad in [
            "",
            "2026-10-05",
            "2026-10-05T08:00:00",
            "2026-10-05T08:00:00.Z",
            "2026-13-05T08:00:00Z",
            "2026-10-05T24:00:00Z",
            "2026/10/05T08:00:00Z",
            "2026-10-05T08:00:00+0400",
            "2026-1a-05T08:00:00Z",
            "1969-12-31T23:59:59Z",
            "２026-10-05T08:00:00Z",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad:?}");
        }
    }
}
