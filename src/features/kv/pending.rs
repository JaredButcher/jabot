//! `/k set`s waiting for their value: at most one per user, each expiring after `TIMEOUT`.
//! In memory, so a restart drops them, which is fine for a 10-minute window.
//!
//! The current time is a parameter, so tests can drive the clock.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serenity::all::UserId;

use super::model::{Key, PendingId};

/// How long `/k set` waits for the value.
pub const TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Above this many entries, `start` drops expired ones, so abandoned sets can't pile up.
const PRUNE_ABOVE: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub id: PendingId,
    pub key: Key,
    pub expires_at: Instant,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Current {
    Nothing,
    Active(Pending),
    /// It expired; it's removed now, so this is reported once.
    Expired(Key),
}

pub struct PendingSets {
    next_id: AtomicU64,
    by_user: Mutex<HashMap<UserId, Pending>>,
}

impl Default for PendingSets {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingSets {
    /// Ids start at the current time in milliseconds rather than 0, so a button from before a
    /// restart can't match a set started after it.
    pub fn new() -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        Self {
            next_id: AtomicU64::new(millis),
            by_user: Mutex::new(HashMap::new()),
        }
    }

    /// Start waiting for `user`'s value for `key`. Returns the new id, and the key of the set
    /// it replaced, if that one was still active.
    pub fn start(&self, user: UserId, key: Key, now: Instant) -> (PendingId, Option<Key>) {
        let id = PendingId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let mut by_user = self.by_user.lock().unwrap();
        if by_user.len() >= PRUNE_ABOVE {
            by_user.retain(|_, pending| pending.expires_at > now);
        }
        let pending = Pending {
            id,
            key,
            expires_at: now + TIMEOUT,
        };
        let replaced = by_user
            .insert(user, pending)
            .filter(|old| old.expires_at > now)
            .map(|old| old.key);
        (id, replaced)
    }

    /// The user's pending set. An expired one is removed and returned as `Expired`.
    pub fn current(&self, user: UserId, now: Instant) -> Current {
        let mut by_user = self.by_user.lock().unwrap();
        match by_user.get(&user) {
            None => Current::Nothing,
            Some(pending) if pending.expires_at > now => Current::Active(pending.clone()),
            Some(_) => Current::Expired(by_user.remove(&user).unwrap().key),
        }
    }

    /// Remove the pending set if it's still `id`, returning its key. `None` if it was
    /// replaced, used, cancelled or deleted, or has expired.
    pub fn finish(&self, user: UserId, id: PendingId, now: Instant) -> Option<Key> {
        let mut by_user = self.by_user.lock().unwrap();
        let pending = by_user.get(&user).filter(|pending| pending.id == id)?;
        let active = pending.expires_at > now;
        let pending = by_user.remove(&user)?;
        active.then_some(pending.key)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.by_user.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::kv::rules::normalize_key;

    fn key(raw: &str) -> Key {
        normalize_key(raw).unwrap()
    }

    fn user(id: u64) -> UserId {
        UserId::new(id)
    }

    #[test]
    fn start_replaces_and_reports_the_previous_key() {
        let sets = PendingSets::new();
        let now = Instant::now();

        let (first, replaced) = sets.start(user(1), key("pasta"), now);
        assert_eq!(replaced, None);
        let (second, replaced) = sets.start(user(1), key("pizza"), now);
        assert_eq!(replaced, Some(key("pasta")));
        assert_ne!(first, second);

        assert_eq!(sets.finish(user(1), first, now), None);
        assert_eq!(sets.finish(user(1), second, now), Some(key("pizza")));
    }

    #[test]
    fn replacing_an_expired_set_reports_nothing() {
        let sets = PendingSets::new();
        let now = Instant::now();
        sets.start(user(1), key("pasta"), now);

        let (_, replaced) = sets.start(user(1), key("pizza"), now + TIMEOUT);
        assert_eq!(replaced, None);
    }

    #[test]
    fn active_until_the_timeout_then_expired_once() {
        let sets = PendingSets::new();
        let now = Instant::now();
        let (id, _) = sets.start(user(1), key("pasta"), now);

        let just_before = now + TIMEOUT - Duration::from_millis(1);
        assert!(matches!(
            sets.current(user(1), just_before),
            Current::Active(Pending { id: active, .. }) if active == id
        ));
        assert_eq!(
            sets.current(user(1), now + TIMEOUT),
            Current::Expired(key("pasta"))
        );
        assert_eq!(sets.current(user(1), now + TIMEOUT), Current::Nothing);
    }

    #[test]
    fn users_are_independent() {
        let sets = PendingSets::new();
        let now = Instant::now();
        sets.start(user(1), key("pasta"), now);

        assert_eq!(sets.current(user(2), now), Current::Nothing);
    }

    #[test]
    fn finish_happens_once() {
        let sets = PendingSets::new();
        let now = Instant::now();
        let (id, _) = sets.start(user(1), key("pasta"), now);

        assert_eq!(sets.finish(user(1), id, now), Some(key("pasta")));
        assert_eq!(sets.finish(user(1), id, now), None);
        assert_eq!(sets.current(user(1), now), Current::Nothing);
    }

    #[test]
    fn finish_after_the_timeout_removes_but_returns_nothing() {
        let sets = PendingSets::new();
        let now = Instant::now();
        let (id, _) = sets.start(user(1), key("pasta"), now);

        assert_eq!(sets.finish(user(1), id, now + TIMEOUT), None);
        assert_eq!(sets.current(user(1), now + TIMEOUT), Current::Nothing);
    }

    #[test]
    fn finish_with_another_users_id_does_nothing() {
        let sets = PendingSets::new();
        let now = Instant::now();
        let (id, _) = sets.start(user(1), key("pasta"), now);

        assert_eq!(sets.finish(user(2), id, now), None);
        assert!(matches!(sets.current(user(1), now), Current::Active(_)));
    }

    #[test]
    fn pruning_drops_only_expired_sets() {
        let sets = PendingSets::new();
        let start = Instant::now();
        for i in 0..PRUNE_ABOVE as u64 {
            sets.start(user(i + 1), key("k"), start);
        }
        let later = start + TIMEOUT / 2;
        sets.start(user(5000), key("k"), later);
        assert_eq!(sets.len(), PRUNE_ABOVE + 1);

        // The first batch has expired by now; the one started `later` hasn't.
        sets.start(user(5001), key("k"), start + TIMEOUT);
        assert_eq!(sets.len(), 2);
    }
}
