use std::fmt;
use std::time::Duration;

use axum::Json;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::text;
use crate::framework::DiscordError;

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

/// Body of a request to the tell endpoint. Fields are optional so that a missing one is
/// reported as such rather than as malformed JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct TellRequest {
    pub token: Option<String>,
    pub message: Option<String>,
}

/// Why a request to the tell endpoint failed. Each variant is one HTTP status.
#[derive(Debug, thiserror::Error)]
pub enum TellError {
    #[error("{}", text::ERR_CONTENT_TYPE)]
    UnsupportedMediaType,
    #[error("{}", text::ERR_BODY)]
    InvalidBody,
    #[error("{}", text::ERR_BODY_TOO_LARGE)]
    BodyTooLarge,
    #[error("{}", text::ERR_TOKEN)]
    InvalidToken,
    #[error("{}", text::ERR_MESSAGE_MISSING)]
    MissingMessage,
    #[error("the message is longer than {MAX_MESSAGE_CHARS} characters")]
    MessageTooLong,
    #[error("too many requests; retry in {} seconds", retry_after_secs(*.0))]
    RateLimited(Duration),
    #[error("{}", text::ERR_DMS_CLOSED)]
    DmsClosed,
    #[error("{}", text::ERR_DISCORD)]
    Discord(#[source] DiscordError),
    #[error("{}", text::ERR_INTERNAL)]
    Repo(#[source] sqlx::Error),
}

/// Discord's limit on message length.
pub const MAX_MESSAGE_CHARS: usize = 2000;

/// A wait as whole seconds for `Retry-After`, rounded up and at least 1.
pub fn retry_after_secs(wait: Duration) -> u64 {
    (wait.as_secs() + u64::from(wait.subsec_nanos() > 0)).max(1)
}

impl TellError {
    pub fn status(&self) -> StatusCode {
        match self {
            TellError::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            TellError::InvalidBody | TellError::MissingMessage => StatusCode::BAD_REQUEST,
            TellError::BodyTooLarge | TellError::MessageTooLong => StatusCode::PAYLOAD_TOO_LARGE,
            TellError::InvalidToken => StatusCode::UNAUTHORIZED,
            TellError::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
            TellError::DmsClosed => StatusCode::UNPROCESSABLE_ENTITY,
            TellError::Discord(_) => StatusCode::BAD_GATEWAY,
            TellError::Repo(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// `{"error": "<reason>"}`, plus `retry_after` and a `Retry-After` header for `429`.
impl IntoResponse for TellError {
    fn into_response(self) -> Response {
        match &self {
            TellError::Discord(error) => tracing::error!(%error, "tell DM failed"),
            TellError::Repo(error) => tracing::error!(%error, "tell storage failed"),
            _ => {}
        }
        let mut body = serde_json::json!({ "error": self.to_string() });
        if let TellError::RateLimited(wait) = self {
            let secs = retry_after_secs(wait);
            body["retry_after"] = secs.into();
            (
                self.status(),
                [(header::RETRY_AFTER, secs.to_string())],
                Json(body),
            )
                .into_response()
        } else {
            (self.status(), Json(body)).into_response()
        }
    }
}
