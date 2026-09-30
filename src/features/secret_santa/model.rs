use std::fmt;

use serenity::all::UserId;

use super::custom_id::{InvalidId, SsId};
use super::text;
use crate::framework::{ComponentKind, FeatureError};

/// Primary key of an `ss_events` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventId(pub i64);

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventStatus {
    /// Created; the host is still choosing participants.
    PreRun,
    /// Assignments have been drawn and sent.
    Running,
    /// Ended or canceled.
    Finished,
}

impl EventStatus {
    pub fn label(self) -> &'static str {
        match self {
            EventStatus::PreRun => "Preparing",
            EventStatus::Running => "Running",
            EventStatus::Finished => "Finished",
        }
    }
}

impl From<EventStatus> for i64 {
    fn from(status: EventStatus) -> Self {
        match status {
            EventStatus::PreRun => 0,
            EventStatus::Running => 1,
            EventStatus::Finished => 2,
        }
    }
}

/// A stored status value that doesn't name any `EventStatus`.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid Secret Santa event status {0}")]
pub struct InvalidStatus(pub i64);

impl TryFrom<i64> for EventStatus {
    type Error = InvalidStatus;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(EventStatus::PreRun),
            1 => Ok(EventStatus::Running),
            2 => Ok(EventStatus::Finished),
            other => Err(InvalidStatus(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub id: EventId,
    pub name: String,
    pub description: Option<String>,
    pub host: UserId,
    pub status: EventStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub user: UserId,
    /// Who this participant gives a gift to, once the event has started.
    pub assignee: Option<UserId>,
}

/// Why a Secret Santa action failed. Refusals (the variants with a `text::` message) are shown
/// to the user as-is; everything else is logged and the user sees a generic error.
#[derive(Debug, thiserror::Error)]
pub enum SsError {
    #[error("{}", text::EVENT_NOT_FOUND)]
    EventNotFound,
    #[error("{}", text::NOT_HOST)]
    NotHost,
    #[error("{}", text::NOT_PARTICIPANT)]
    NotParticipant,
    #[error("{}", text::MISSING_EVENT_ID)]
    MissingEventId,
    #[error("{}", text::NOT_PREPARING)]
    NotPreparing,
    #[error("{}", text::NOT_RUNNING)]
    NotRunning,
    #[error("{}", text::ALREADY_FINISHED)]
    AlreadyFinished,
    #[error("{}", text::PARTICIPANTS_LOCKED)]
    ParticipantsLocked,
    #[error("{}", text::NOT_ENOUGH_PARTICIPANTS)]
    NotEnoughParticipants,
    /// Lost a race with another start (e.g. a double click).
    #[error("{}", text::ALREADY_STARTED)]
    AlreadyStarted,
    /// The status changed between reading the event and updating it.
    #[error("{}", text::EVENT_CHANGED)]
    EventChanged,

    #[error(transparent)]
    Repo(#[from] sqlx::Error),
    #[error(transparent)]
    Discord(#[from] serenity::Error),
    #[error(transparent)]
    InvalidId(#[from] InvalidId),
    #[error("{0:?} was routed to the wrong handler")]
    UnexpectedId(SsId),
    #[error("unknown /ss subcommand {0:?}")]
    UnknownSubcommand(Option<String>),
    #[error("participant picker sent {0:?}")]
    UnexpectedComponent(ComponentKind),
}

impl SsError {
    /// Whether this is a refusal meant for the user rather than a failure to log.
    pub fn is_refusal(&self) -> bool {
        !matches!(
            self,
            SsError::Repo(_)
                | SsError::Discord(_)
                | SsError::InvalidId(_)
                | SsError::UnexpectedId(_)
                | SsError::UnknownSubcommand(_)
                | SsError::UnexpectedComponent(_)
        )
    }
}

impl From<SsError> for FeatureError {
    fn from(error: SsError) -> Self {
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

    /// B8: unknown values are an error, not a silently startable event.
    #[test]
    fn status_round_trips_and_rejects_unknown_values() {
        for status in [
            EventStatus::PreRun,
            EventStatus::Running,
            EventStatus::Finished,
        ] {
            assert_eq!(EventStatus::try_from(i64::from(status)), Ok(status));
        }
        assert_eq!(EventStatus::try_from(3), Err(InvalidStatus(3)));
        assert_eq!(EventStatus::try_from(-1), Err(InvalidStatus(-1)));
    }

    #[test]
    fn refusals_reach_the_user_and_failures_do_not() {
        let refusal = FeatureError::from(SsError::NotHost);
        assert!(matches!(refusal, FeatureError::User(message) if message == text::NOT_HOST));

        let failure = FeatureError::from(SsError::Repo(sqlx::Error::PoolTimedOut));
        assert!(matches!(failure, FeatureError::Internal(_)));
    }
}
