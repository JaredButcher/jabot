use std::fmt;

/// Primary key of a `tell_tokens` row. AUTOINCREMENT, so an id is never reused: a Revoke
/// button holding an old id can't match a newer token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenId(pub i64);

impl fmt::Display for TokenId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredToken {
    pub id: TokenId,
    pub token: String,
    /// False if the user already had a token and the candidate was discarded.
    pub created: bool,
}
