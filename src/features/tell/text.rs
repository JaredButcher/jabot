//! Command name and user-facing text for tell.

// Slash command
pub const CMD_TELL: &str = "tell";
pub const CMD_TELL_DESC: &str = "Get a curl command that DMs you, e.g. when a long task finishes";

// Replies
pub const BTN_REVOKE: &str = "Revoke token";
pub const REVOKED: &str = "Token revoked. Run /tell for a new one.";
pub const ALREADY_REVOKED: &str =
    "That token was already revoked. Run /tell to see your current one.";

// DMs
pub const TEST_DM: &str = "Messages sent with your /tell token will arrive here.";
pub const DMS_CLOSED: &str = "I couldn't DM you, so tell messages won't reach you. Allow direct \
    messages from members of a server we share (the server's Privacy Settings), then try the \
    curl command.";

// HTTP error messages
pub const ERR_CONTENT_TYPE: &str = "send the body as JSON, e.g. with curl --json";
pub const ERR_BODY: &str =
    "the body must be a JSON object with string fields \"token\" and \"message\"";
pub const ERR_BODY_TOO_LARGE: &str = "the request body is too large";
pub const ERR_TOKEN: &str = "invalid token";
pub const ERR_MESSAGE_MISSING: &str = "the message is missing or blank";
pub const ERR_DMS_CLOSED: &str = "Discord won't let the bot DM you. Allow direct messages from \
    members of a server you share with the bot (the server's Privacy Settings).";
pub const ERR_DISCORD: &str = "Discord didn't accept the message; try again later";
pub const ERR_INTERNAL: &str = "internal error";
