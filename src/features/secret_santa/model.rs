use std::fmt;

use serenity::all::UserId;

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
