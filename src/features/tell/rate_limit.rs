//! In-memory token buckets, one per key. A bucket holds `capacity` requests and gets one back
//! every `period`. State is lost on restart, which is fine for abuse protection.
//!
//! Implemented as GCRA: per key, store only the time at which the bucket will be full again.
//! The current time is a parameter, so tests can drive the clock.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Above this many keys, full buckets are dropped, so a stream of new client IPs can't grow
/// the map without bound.
const PRUNE_ABOVE: usize = 1024;

pub struct RateLimiter<K> {
    period: Duration,
    /// How far a bucket's full-again time may lie in the future while it still has a request
    /// left: `period * (capacity - 1)`.
    tolerance: Duration,
    full_at: Mutex<HashMap<K, Instant>>,
}

impl<K: Eq + Hash> RateLimiter<K> {
    pub fn new(capacity: u32, period: Duration) -> Self {
        assert!(capacity > 0, "a rate limit needs a capacity of at least 1");
        Self {
            period,
            tolerance: period * (capacity - 1),
            full_at: Mutex::new(HashMap::new()),
        }
    }

    /// Take one request from `key`'s bucket. If it's empty, `Err` says how long until it
    /// has one again.
    pub fn check(&self, key: K, now: Instant) -> Result<(), Duration> {
        let mut full_at = self.full_at.lock().unwrap();
        let backlog = self.backlog(full_at.get(&key), now)?;
        if full_at.len() >= PRUNE_ABOVE {
            full_at.retain(|_, at| *at > now);
        }
        full_at.insert(key, now + backlog + self.period);
        Ok(())
    }

    /// Whether `check` would succeed, without taking a request.
    pub fn peek(&self, key: &K, now: Instant) -> Result<(), Duration> {
        let full_at = self.full_at.lock().unwrap();
        self.backlog(full_at.get(key), now).map(|_| ())
    }

    /// Time until the bucket is full again, or `Err(wait)` if it's empty.
    fn backlog(&self, full_at: Option<&Instant>, now: Instant) -> Result<Duration, Duration> {
        let backlog = full_at.map_or(Duration::ZERO, |at| at.saturating_duration_since(now));
        if backlog > self.tolerance {
            Err(backlog - self.tolerance)
        } else {
            Ok(backlog)
        }
    }

    #[cfg(test)]
    fn keys(&self) -> usize {
        self.full_at.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: Duration = Duration::from_secs(12);

    #[test]
    fn allows_capacity_then_reports_the_wait() {
        let limiter = RateLimiter::new(5, PERIOD);
        let now = Instant::now();

        for _ in 0..5 {
            assert_eq!(limiter.check("a", now), Ok(()));
        }
        assert_eq!(limiter.check("a", now), Err(PERIOD));
        assert_eq!(
            limiter.check("a", now + Duration::from_secs(5)),
            Err(Duration::from_secs(7))
        );
    }

    #[test]
    fn refills_one_request_per_period() {
        let limiter = RateLimiter::new(2, PERIOD);
        let now = Instant::now();
        limiter.check("a", now).unwrap();
        limiter.check("a", now).unwrap();

        let later = now + PERIOD;
        assert_eq!(limiter.check("a", later), Ok(()));
        assert_eq!(limiter.check("a", later), Err(PERIOD));

        // A long pause refills the bucket, but never past its capacity.
        let much_later = later + PERIOD * 10;
        assert_eq!(limiter.check("a", much_later), Ok(()));
        assert_eq!(limiter.check("a", much_later), Ok(()));
        assert!(limiter.check("a", much_later).is_err());
    }

    #[test]
    fn keys_are_independent() {
        let limiter = RateLimiter::new(1, PERIOD);
        let now = Instant::now();

        assert_eq!(limiter.check("a", now), Ok(()));
        assert!(limiter.check("a", now).is_err());
        assert_eq!(limiter.check("b", now), Ok(()));
    }

    #[test]
    fn peek_takes_nothing() {
        let limiter = RateLimiter::new(1, PERIOD);
        let now = Instant::now();

        assert_eq!(limiter.peek(&"a", now), Ok(()));
        assert_eq!(limiter.peek(&"a", now), Ok(()));
        limiter.check("a", now).unwrap();
        assert_eq!(limiter.peek(&"a", now), Err(PERIOD));
    }

    #[test]
    fn prunes_only_full_buckets() {
        let limiter = RateLimiter::new(1, PERIOD);
        let start = Instant::now();
        limiter.check(u32::MAX, start).unwrap();
        let later = start + PERIOD;
        for key in 0..PRUNE_ABOVE as u32 - 1 {
            limiter.check(key, later).unwrap();
        }
        assert_eq!(limiter.keys(), PRUNE_ABOVE);

        // The first key's bucket has refilled by now; every other one is still empty.
        limiter.check(PRUNE_ABOVE as u32, later).unwrap();

        assert_eq!(limiter.keys(), PRUNE_ABOVE);
        assert!(
            limiter.check(0, later).is_err(),
            "an empty bucket survives pruning"
        );
    }
}
