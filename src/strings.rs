pub struct Strings;

impl Strings {
    // Bot responses
    pub const HELLO_RESPONSE: &'static str = "Hello, world!";
    pub const UNKNOWN_COMMAND: &'static str = "Unknown command";
    pub const SUBT_RESPONSE: &'static str = "Hello!";

    // Default values
    pub const DEFAULT_SS_NAME: &'static str = "Default Name";
    pub const DEFAULT_SS_DESCRIPTION: &'static str = "Default Description";

    // Command names and descriptions
    pub const CMD_ECHO_NAME: &'static str = "echo";
    pub const CMD_ECHO_DESC: &'static str = "Echo the provided text";
    pub const CMD_SS_NAME: &'static str = "ss";
    pub const CMD_SS_DESC: &'static str = "Secret Santa Event Commands!";

    // ss command names and descriptions
    pub const CMD_SS_CREATE_NAME: &'static str = "create";
    pub const CMD_SS_CREATE_DESC: &'static str = "Create a new Secret Santa Event";
    pub const CMD_SS_INFO_NAME: &'static str = "info";
    pub const CMD_SS_INFO_DESC: &'static str = "Get info on a Secret Santa Event that you are in";
    pub const CMD_SS_LIST_NAME: &'static str = "list";
    pub const CMD_SS_LIST_DESC: &'static str =
        "List Secret Santa Events that you are in or invited to";
    pub const CMD_SS_JOIN_NAME: &'static str = "join";
    pub const CMD_SS_JOIN_DESC: &'static str = "Join a Secret Santa Event that you are invited to";
    pub const CMD_SS_WISH_NAME: &'static str = "wish";
    pub const CMD_SS_WISH_DESC: &'static str =
        "Set a message to give to your secret santa to help them.";

    // Option names and descriptions
    pub const OPT_TEXT: &'static str = "text";
    pub const OPT_TEXT_DESC: &'static str = "Text to echo";
    pub const OPT_NAME: &'static str = "name";
    pub const OPT_NAME_DESC: &'static str = "Secret Santa Event Name";
    pub const OPT_DESCRIPTION: &'static str = "description";
    pub const OPT_DESCRIPTION_DESC: &'static str = "Secret Santa Event Discription";
    pub const OPT_SS_EVT_ID_NAME: &'static str = "id";
    pub const OPT_SS_EVT_ID_DESC: &'static str = "Id number of Secret Santa event.";

    // Modal constants
    pub const MODAL_SS_CREATE_ID: &'static str = "ss:create:modal";
    pub const MODAL_SS_MODIFY_ID: &'static str = "ss:create:modal";
    pub const MODAL_SS_INFO_ID: &'static str = "ss:info:modal";
    pub const MODAL_SS_CREATE_TITLE: &'static str = "Secret Santa Event";
    pub const MODAL_SS_CREATE_EDIT_TITLE: &'static str = "Modify Secret Santa Event";
    pub const MODAL_SS_INFO_NAME_ID: &'static str = "ss:info:name";
    pub const MODAL_SS_INFO_NAME_LABEL: &'static str = "Event Name";
    pub const MODAL_SS_INFO_NAME_PLACEHOLDER: &'static str = "Enter event name...";
    pub const MODAL_SS_INFO_DESC_ID: &'static str = "ss:info:description";
    pub const MODAL_SS_INFO_DESC_LABEL: &'static str = "Event Description";
    pub const MODAL_SS_INFO_DESC_PLACEHOLDER: &'static str = "Enter event description...";

    // Modal response messages
    pub const MSG_SS_EVENT_CREATED: &'static str =
        "Secret Santa Event Created!\nName: {}\nDescription: {}";
    pub const MSG_NO_DESCRIPTION: &'static str = "No description provided";
    pub const MSG_UNKNOWN_NAME: &'static str = "Unknown";

    // Component ids
    pub const COMP_SS_INFO_USER_ID: &'static str = "ss:info:users";
}
