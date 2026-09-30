use std::fmt;

use serenity::all::UserId;

use super::text;
use crate::framework::FeatureError;

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

/// Why a Secret Santa action was refused. Each message is shown to the user as-is.
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
}

impl From<SsError> for FeatureError {
    fn from(error: SsError) -> Self {
        FeatureError::user(error.to_string())
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
}
