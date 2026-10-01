//! Custom ids for tell components: `tell:<action>:<token id>`.
//! The `tell` prefix is the feature's namespace, which the registry routes on.

use std::fmt;
use std::str::FromStr;

use super::model::TokenId;

pub const NAMESPACE: &str = "tell";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TellId {
    /// The Revoke token button on a `/tell` reply.
    Revoke(TokenId),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unrecognized tell custom id {0:?}")]
pub struct InvalidId(pub String);

impl fmt::Display for TellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TellId::Revoke(id) => write!(f, "{NAMESPACE}:revoke:{id}"),
        }
    }
}

impl FromStr for TellId {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidId(s.to_string());
        let mut parts = s.split(':');
        if parts.next() != Some(NAMESPACE) {
            return Err(invalid());
        }
        let id = match (parts.next(), parts.next(), parts.next()) {
            (Some("revoke"), Some(id), None) => id.parse().map_err(|_| invalid())?,
            _ => return Err(invalid()),
        };
        Ok(TellId::Revoke(TokenId(id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let id = TellId::Revoke(TokenId(7));
        assert_eq!(id.to_string(), "tell:revoke:7");
        assert_eq!("tell:revoke:7".parse::<TellId>(), Ok(id));
    }

    #[test]
    fn rejects_malformed_ids() {
        for bad in [
            "",
            "tell",
            "tell:revoke",
            "tell:revoke:x",
            "tell:revoke:7:8",
            "tell:other:7",
            "ss:revoke:7",
        ] {
            assert_eq!(
                bad.parse::<TellId>(),
                Err(InvalidId(bad.to_string())),
                "{bad}"
            );
        }
    }
}
