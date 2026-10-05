use std::fmt;

use crate::framework::{DmError, FeatureError};

use super::custom_id::InvalidId;
use super::rules::{MAX_KEYS, MAX_VALUE_CHARS};
use super::text;

/// A normalized key: trimmed, lowercased, 1–64 characters, no backticks or control
/// characters. Built only by `rules::normalize_key`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key(pub(super) String);

impl Key {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Identifies one `/k set` while it waits for its value, so the prompt's buttons can't act on
/// a later one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PendingId(pub u64);

impl fmt::Display for PendingId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KvError {
    #[error("{}", text::INVALID_KEY)]
    InvalidKey,
    #[error("No key contains `{0}`.")]
    NotFound(Key),
    #[error(
        "You already have {MAX_KEYS} keys. Replace one of them, or delete one first (run `/k set` on it and press Delete)."
    )]
    TooManyKeys,
    #[error("{}", text::NO_TEXT)]
    NoText,
    #[error(
        "That's {0} characters; I can store at most {MAX_VALUE_CHARS}. Send a shorter message."
    )]
    ValueTooLong(usize),

    #[error(transparent)]
    Repo(#[from] sqlx::Error),
    #[error(transparent)]
    Discord(#[from] serenity::Error),
    #[error(transparent)]
    Dm(#[from] DmError),
    #[error(transparent)]
    InvalidId(#[from] InvalidId),
    #[error("unknown /k subcommand {0:?}")]
    UnknownSubcommand(Option<String>),
    #[error("/k set arrived without its required key")]
    MissingKey,
}

impl KvError {
    /// Whether this is a refusal meant for the user rather than a failure to log.
    pub fn is_refusal(&self) -> bool {
        !matches!(
            self,
            KvError::Repo(_)
                | KvError::Discord(_)
                | KvError::Dm(_)
                | KvError::InvalidId(_)
                | KvError::UnknownSubcommand(_)
                | KvError::MissingKey
        )
    }
}

impl From<KvError> for FeatureError {
    fn from(error: KvError) -> Self {
        if error.is_refusal() {
            FeatureError::user(error.to_string())
        } else {
            FeatureError::internal(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_reach_the_user_and_failures_do_not() {
        let refusal = FeatureError::from(KvError::ValueTooLong(2001));
        assert!(matches!(
            refusal,
            FeatureError::User(message) if message.starts_with("That's 2001 characters")
        ));

        let failure = FeatureError::from(KvError::Repo(sqlx::Error::PoolTimedOut));
        assert!(matches!(failure, FeatureError::Internal(_)));
    }
}
