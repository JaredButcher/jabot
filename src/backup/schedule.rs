//! When to back up, as pure functions of unix time (UTC seconds). No dates or time zones:
//! a day is 86,400 seconds, and the daily time is seconds since midnight UTC.

pub const DAY: u64 = 86_400;
/// Wait after startup before a catch-up run, so the bot settles first.
const STARTUP_DELAY: u64 = 2 * 60;
/// Wait before retrying a failed run.
const RETRY_DELAY: u64 = 60 * 60;
/// Retries after a failed run, before waiting for the next day.
const MAX_RETRIES: u32 = 3;
/// Slack when deciding whether the latest snapshot belongs to the latest daily run.
const GRACE: u64 = 5 * 60;

/// `08:00` or `8:00` as seconds since midnight.
pub fn parse_time_of_day(s: &str) -> Option<u64> {
    let (hours, minutes) = s.trim().split_once(':')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !digits(hours) || hours.len() > 2 || minutes.len() != 2 || !digits(minutes) {
        return None;
    }
    let (hours, minutes): (u64, u64) = (hours.parse().ok()?, minutes.parse().ok()?);
    (hours < 24 && minutes < 60).then_some(hours * 3600 + minutes * 60)
}

/// Sunday, for the weekly `restic check`. 1970-01-01 was a Thursday.
pub fn is_sunday(unix: u64) -> bool {
    (unix / DAY + 4).is_multiple_of(7)
}

/// The daily schedule, and the retry count of the current run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// Seconds since midnight UTC.
    at: u64,
    retries: u32,
}

impl Schedule {
    pub fn new(at: u64) -> Self {
        Self { at, retries: 0 }
    }

    /// The first run after startup. If no snapshot was taken since the latest daily time (the
    /// bot was down, or that run failed), catch up shortly; otherwise wait for the next one.
    /// An unknown latest snapshot (`None`) counts as missing.
    pub fn first_run(&self, now: u64, latest: Option<u64>) -> u64 {
        let last_daily = self.next_daily(now) - DAY;
        match latest {
            Some(latest) if latest + GRACE >= last_daily => self.next_daily(now),
            _ => now + STARTUP_DELAY,
        }
    }

    /// The run after one that just finished: the next daily time, or after a failure a retry
    /// in an hour, up to `MAX_RETRIES` times.
    pub fn after_run(&mut self, now: u64, succeeded: bool) -> u64 {
        if !succeeded && self.retries < MAX_RETRIES {
            self.retries += 1;
            now + RETRY_DELAY
        } else {
            self.retries = 0;
            self.next_daily(now)
        }
    }

    /// The first daily time strictly after `now`.
    fn next_daily(&self, now: u64) -> u64 {
        let today = now - now % DAY + self.at;
        if today > now { today } else { today + DAY }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-05 (a Monday) at midnight UTC.
    const MONDAY: u64 = 1_791_158_400;
    const HOUR: u64 = 3600;
    const EIGHT: u64 = 8 * HOUR;

    fn schedule() -> Schedule {
        Schedule::new(EIGHT)
    }

    #[test]
    fn parses_times_of_day() {
        assert_eq!(parse_time_of_day("08:00"), Some(EIGHT));
        assert_eq!(parse_time_of_day("8:00"), Some(EIGHT));
        assert_eq!(parse_time_of_day(" 23:59 "), Some(23 * HOUR + 59 * 60));
        assert_eq!(parse_time_of_day("00:00"), Some(0));
        for bad in [
            "", "8", "24:00", "08:60", "08:0", "008:00", "-1:00", "8:00pm", "08:00:00",
        ] {
            assert_eq!(parse_time_of_day(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn knows_sundays() {
        assert!(!is_sunday(MONDAY));
        assert!(is_sunday(MONDAY - 1));
        assert!(is_sunday(MONDAY - DAY));
        assert!(!is_sunday(MONDAY - DAY - 1));
    }

    #[test]
    fn next_daily_is_today_or_tomorrow() {
        let schedule = schedule();
        assert_eq!(schedule.next_daily(MONDAY + 7 * HOUR), MONDAY + EIGHT);
        assert_eq!(schedule.next_daily(MONDAY + EIGHT), MONDAY + DAY + EIGHT);
        assert_eq!(schedule.next_daily(MONDAY + 9 * HOUR), MONDAY + DAY + EIGHT);
    }

    #[test]
    fn startup_after_todays_run_waits_for_tomorrow() {
        let now = MONDAY + 12 * HOUR;
        let latest = Some(MONDAY + EIGHT + 3);
        assert_eq!(schedule().first_run(now, latest), MONDAY + DAY + EIGHT);
    }

    #[test]
    fn startup_before_todays_run_waits_for_it() {
        let now = MONDAY + 7 * HOUR;
        let latest = Some(MONDAY - DAY + EIGHT + 3);
        assert_eq!(schedule().first_run(now, latest), MONDAY + EIGHT);
    }

    #[test]
    fn startup_after_a_missed_run_catches_up() {
        // Down at 08:00 today; yesterday's snapshot is only 25 hours old, but today's is missing.
        let now = MONDAY + 9 * HOUR;
        let latest = Some(MONDAY - DAY + EIGHT + 3);
        assert_eq!(schedule().first_run(now, latest), now + STARTUP_DELAY);
    }

    #[test]
    fn startup_without_snapshots_catches_up() {
        let now = MONDAY + 12 * HOUR;
        assert_eq!(schedule().first_run(now, None), now + STARTUP_DELAY);
    }

    #[test]
    fn a_retry_that_succeeded_counts_as_todays_run() {
        let now = MONDAY + 12 * HOUR;
        let latest = Some(MONDAY + 10 * HOUR);
        assert_eq!(schedule().first_run(now, latest), MONDAY + DAY + EIGHT);
    }

    #[test]
    fn success_waits_for_the_next_day() {
        let mut schedule = schedule();
        let now = MONDAY + EIGHT + 5;
        assert_eq!(schedule.after_run(now, true), MONDAY + DAY + EIGHT);
    }

    #[test]
    fn failures_retry_hourly_three_times_then_wait_for_the_next_day() {
        let mut schedule = schedule();
        let mut now = MONDAY + EIGHT;
        for _ in 0..MAX_RETRIES {
            let next = schedule.after_run(now, false);
            assert_eq!(next, now + RETRY_DELAY);
            now = next;
        }
        assert_eq!(schedule.after_run(now, false), MONDAY + DAY + EIGHT);
        // The next day starts with a full set of retries again.
        let now = MONDAY + DAY + EIGHT;
        assert_eq!(schedule.after_run(now, false), now + RETRY_DELAY);
    }

    #[test]
    fn success_resets_the_retries() {
        let mut schedule = schedule();
        schedule.after_run(MONDAY + EIGHT, false);
        schedule.after_run(MONDAY + 9 * HOUR, true);
        let now = MONDAY + DAY + EIGHT;
        for _ in 0..MAX_RETRIES {
            schedule.after_run(now, false);
        }
        assert_eq!(schedule.retries, MAX_RETRIES);
    }
}
