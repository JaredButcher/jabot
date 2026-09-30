//! Custom ids for Secret Santa components and modals: `ss:<action>[:<event id>]`.
//! The `ss` prefix is the feature's namespace, which the registry routes on.

use std::fmt;
use std::str::FromStr;

use super::model::EventId;

pub const NAMESPACE: &str = "ss";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsId {
    /// Modal that creates a new event.
    CreateModal,
    /// Modal that edits an existing event's name and description.
    EditModal(EventId),
    /// Host's user-select menu for the participant list.
    UserSelect(EventId),
    Start(EventId),
    End(EventId),
    Cancel(EventId),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unrecognized Secret Santa custom id {0:?}")]
pub struct InvalidId(pub String);

impl From<InvalidId> for crate::framework::FeatureError {
    fn from(error: InvalidId) -> Self {
        Self::internal(error)
    }
}

impl fmt::Display for SsId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SsId::CreateModal => write!(f, "{NAMESPACE}:create"),
            SsId::EditModal(id) => write!(f, "{NAMESPACE}:edit:{id}"),
            SsId::UserSelect(id) => write!(f, "{NAMESPACE}:participants:{id}"),
            SsId::Start(id) => write!(f, "{NAMESPACE}:start:{id}"),
            SsId::End(id) => write!(f, "{NAMESPACE}:end:{id}"),
            SsId::Cancel(id) => write!(f, "{NAMESPACE}:cancel:{id}"),
        }
    }
}

impl FromStr for SsId {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidId(s.to_string());
        let mut parts = s.split(':');
        if parts.next() != Some(NAMESPACE) {
            return Err(invalid());
        }
        let action = parts.next().ok_or_else(invalid)?;
        let event = parts
            .next()
            .map(|id| id.parse().map(EventId).map_err(|_| invalid()));
        if parts.next().is_some() {
            return Err(invalid());
        }

        match (action, event) {
            ("create", None) => Ok(SsId::CreateModal),
            ("edit", Some(id)) => Ok(SsId::EditModal(id?)),
            ("participants", Some(id)) => Ok(SsId::UserSelect(id?)),
            ("start", Some(id)) => Ok(SsId::Start(id?)),
            ("end", Some(id)) => Ok(SsId::End(id?)),
            ("cancel", Some(id)) => Ok(SsId::Cancel(id?)),
            _ => Err(invalid()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let id = EventId(42);
        for original in [
            SsId::CreateModal,
            SsId::EditModal(id),
            SsId::UserSelect(id),
            SsId::Start(id),
            SsId::End(id),
            SsId::Cancel(id),
        ] {
            assert_eq!(original.to_string().parse::<SsId>(), Ok(original));
        }
    }

    #[test]
    fn starts_with_namespace() {
        assert_eq!(SsId::Start(EventId(5)).to_string(), "ss:start:5");
    }

    #[test]
    fn rejects_malformed_ids() {
        for bad in [
            "",
            "ss",
            "ss:start",
            "ss:start:abc",
            "ss:start:5:6",
            "ss:create:5",
            "ss:bogus:5",
            "xx:start:5",
        ] {
            assert_eq!(
                bad.parse::<SsId>(),
                Err(InvalidId(bad.to_string())),
                "{bad}"
            );
        }
    }
}
