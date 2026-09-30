use std::error::Error as StdError;

/// Error returned by feature handlers. The registry turns it into a reply to the user.
#[derive(Debug, thiserror::Error)]
pub enum FeatureError {
    /// Safe to show the user as-is; sent as an ephemeral message.
    #[error("{0}")]
    User(String),
    /// Logged; the user sees a generic message.
    #[error(transparent)]
    Internal(Box<dyn StdError + Send + Sync>),
}

impl FeatureError {
    pub fn user(message: impl Into<String>) -> Self {
        Self::User(message.into())
    }

    pub fn internal(error: impl Into<Box<dyn StdError + Send + Sync>>) -> Self {
        Self::Internal(error.into())
    }
}

impl From<sqlx::Error> for FeatureError {
    fn from(error: sqlx::Error) -> Self {
        Self::internal(error)
    }
}

impl From<serenity::Error> for FeatureError {
    fn from(error: serenity::Error) -> Self {
        Self::internal(error)
    }
}
