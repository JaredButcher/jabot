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

impl From<i64> for EventStatus {
    fn from(value: i64) -> Self {
        match value {
            1 => EventStatus::Running,
            2 => EventStatus::Finished,
            _ => EventStatus::PreRun, // Default to PreRun for invalid values
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
