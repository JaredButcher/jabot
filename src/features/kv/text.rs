//! Command names and user-facing text for kv.

// Slash command
pub const CMD_K: &str = "k";
pub const CMD_K_DESC: &str = "Store text under a key and get it back";
pub const SUB_SET: &str = "set";
pub const SUB_SET_DESC: &str = "Store your next DM to me under a key";
pub const SUB_GET: &str = "get";
pub const SUB_GET_DESC: &str = "Show a value, search your keys, or list them all";
pub const OPT_KEY: &str = "key";
pub const OPT_KEY_SET_DESC: &str = "Key to store the value under";
pub const OPT_KEY_GET_DESC: &str = "A key or part of one; leave empty to list all your keys";

// Buttons
pub const BTN_OPEN_DM: &str = "Open DM";
pub const BTN_CANCEL: &str = "Cancel";
pub const BTN_DELETE: &str = "Delete";
pub const BTN_PREV: &str = "◀ Prev";
pub const BTN_NEXT: &str = "Next ▶";

// Replies
pub const NO_KEYS: &str = "You have no keys yet. Use `/k set` to store one.";
pub const PROMPT_EXPIRED: &str = "This prompt has expired. Run `/k set` again.";
pub const DMS_CLOSED: &str = "I couldn't DM you, so I can't take the value. Allow direct \
    messages from members of a server we share (the server's Privacy Settings), then run \
    `/k set` again.";

// Refusals
pub const INVALID_KEY: &str = "A key must be 1–64 characters, with no backticks or line breaks.";
pub const NO_TEXT: &str = "Only text can be stored. Send the value as a text message.";
