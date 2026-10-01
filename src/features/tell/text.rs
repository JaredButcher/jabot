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
