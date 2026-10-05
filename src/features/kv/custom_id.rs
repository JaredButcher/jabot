//! Custom ids for kv components:
//! - `kv:page:<page>` and `kv:page:<page>:<query>`: Prev/Next on a key list;
//! - `kv:cancel:<pending id>` and `kv:delete:<pending id>`: buttons on a `/k set` prompt.
//!
//! The `kv` prefix is the feature's namespace, which the registry routes on. A query is a
//! normalized key, which may itself contain `:`, so it comes last.

use std::fmt;
use std::str::FromStr;

use super::model::PendingId;

pub const NAMESPACE: &str = "kv";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvId {
    /// Page `page` (0-based) of the keys containing `query`, or of all keys.
    Page {
        page: usize,
        query: Option<String>,
    },
    Cancel(PendingId),
    Delete(PendingId),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unrecognized kv custom id {0:?}")]
pub struct InvalidId(pub String);

impl fmt::Display for KvId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KvId::Page { page, query: None } => write!(f, "{NAMESPACE}:page:{page}"),
            KvId::Page {
                page,
                query: Some(query),
            } => write!(f, "{NAMESPACE}:page:{page}:{query}"),
            KvId::Cancel(id) => write!(f, "{NAMESPACE}:cancel:{id}"),
            KvId::Delete(id) => write!(f, "{NAMESPACE}:delete:{id}"),
        }
    }
}

impl FromStr for KvId {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidId(s.to_string());
        let mut parts = s.splitn(4, ':');
        if parts.next() != Some(NAMESPACE) {
            return Err(invalid());
        }
        let pending = |id: &str| id.parse().map(PendingId).map_err(|_| invalid());
        match (parts.next(), parts.next(), parts.next()) {
            (Some("page"), Some(page), query) => {
                let page = page.parse().map_err(|_| invalid())?;
                match query {
                    Some("") => Err(invalid()),
                    query => Ok(KvId::Page {
                        page,
                        query: query.map(str::to_string),
                    }),
                }
            }
            (Some("cancel"), Some(id), None) => Ok(KvId::Cancel(pending(id)?)),
            (Some("delete"), Some(id), None) => Ok(KvId::Delete(pending(id)?)),
            _ => Err(invalid()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for (id, text) in [
            (
                KvId::Page {
                    page: 2,
                    query: None,
                },
                "kv:page:2",
            ),
            (
                KvId::Page {
                    page: 0,
                    query: Some("pa".into()),
                },
                "kv:page:0:pa",
            ),
            (
                KvId::Page {
                    page: 1,
                    query: Some("a:b:c".into()),
                },
                "kv:page:1:a:b:c",
            ),
            (KvId::Cancel(PendingId(7)), "kv:cancel:7"),
            (KvId::Delete(PendingId(7)), "kv:delete:7"),
        ] {
            assert_eq!(id.to_string(), text);
            assert_eq!(text.parse::<KvId>(), Ok(id));
        }
    }

    #[test]
    fn longest_page_id_fits_discord_limit() {
        let id = KvId::Page {
            page: 999,
            query: Some("é".repeat(super::super::rules::MAX_KEY_CHARS)),
        };
        assert!(id.to_string().chars().count() <= 100);
    }

    #[test]
    fn rejects_malformed_ids() {
        for bad in [
            "",
            "kv",
            "kv:page",
            "kv:page:x",
            "kv:page:-1",
            "kv:page:1:",
            "kv:cancel",
            "kv:cancel:x",
            "kv:cancel:1:2",
            "kv:delete:",
            "kv:other:1",
            "ss:page:1",
        ] {
            assert_eq!(
                bad.parse::<KvId>(),
                Err(InvalidId(bad.to_string())),
                "{bad}"
            );
        }
    }
}
