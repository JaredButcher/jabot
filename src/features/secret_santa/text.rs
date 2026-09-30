//! Command names and user-facing text for Secret Santa.

// Slash command
pub const CMD_SS: &str = "ss";
pub const CMD_SS_DESC: &str = "Secret Santa Event Commands!";
pub const CMD_CREATE: &str = "create";
pub const CMD_CREATE_DESC: &str = "Create a new Secret Santa Event, or edit one you host";
pub const CMD_INFO: &str = "info";
pub const CMD_INFO_DESC: &str = "Get info on a Secret Santa Event that you are in";
pub const CMD_LIST: &str = "list";
pub const CMD_LIST_DESC: &str = "List Secret Santa Events that you are in";
pub const OPT_EVENT_ID: &str = "id";
pub const OPT_EVENT_ID_DESC: &str = "Id number of Secret Santa event.";

// Create/edit form
pub const FORM_CREATE_TITLE: &str = "Secret Santa Event";
pub const FORM_EDIT_TITLE: &str = "Modify Secret Santa Event";
pub const FIELD_NAME: &str = "name";
pub const FIELD_NAME_LABEL: &str = "Event Name";
pub const FIELD_DESCRIPTION: &str = "description";
pub const FIELD_DESCRIPTION_LABEL: &str = "Event Description";
pub const DEFAULT_NAME: &str = "Default Name";
pub const DEFAULT_DESCRIPTION: &str = "Default Description";

// Replies
pub const EVENT_NOT_FOUND: &str = "Event not found";
pub const NOT_HOST: &str = "You are not the host of this event";
pub const NOT_PARTICIPANT: &str = "Event not found, or you are not a participant in it";
pub const MISSING_EVENT_ID: &str = "Please provide an event ID.";
pub const NOT_PREPARING: &str = "Event is not in preparing state";
pub const NOT_RUNNING: &str = "Event is not currently running";
pub const ALREADY_FINISHED: &str = "Event cannot be canceled (already finished)";
pub const ALREADY_STARTED: &str = "Event was already started";
pub const EVENT_CHANGED: &str = "The event changed in the meantime. Please try again.";
pub const PARTICIPANTS_LOCKED: &str = "Cannot modify users of started event";
pub const NOT_ENOUGH_PARTICIPANTS: &str = "Secret Santa requires more than one participant";
pub const TOO_MANY_PARTICIPANTS: &str =
    "An event can have at most 25 participants, including the host";
pub const STARTED: &str = "Event started successfully!";
pub const ENDED: &str = "Event ended successfully!";
pub const CANCELED: &str = "Event canceled successfully!";
pub const NO_EVENTS: &str = "You are not in any Secret Santa events.";
