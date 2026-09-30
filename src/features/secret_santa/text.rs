pub struct Strings;

impl Strings {
    // Default values
    pub const DEFAULT_SS_NAME: &'static str = "Default Name";
    pub const DEFAULT_SS_DESCRIPTION: &'static str = "Default Description";

    // Command names and descriptions
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

    // Option names and descriptions
    pub const OPT_SS_EVT_ID_NAME: &'static str = "id";
    pub const OPT_SS_EVT_ID_DESC: &'static str = "Id number of Secret Santa event.";

    // Modal constants
    pub const MODAL_SS_CREATE_TITLE: &'static str = "Secret Santa Event";
    pub const MODAL_SS_CREATE_EDIT_TITLE: &'static str = "Modify Secret Santa Event";
    pub const MODAL_SS_INFO_NAME_ID: &'static str = "name";
    pub const MODAL_SS_INFO_NAME_LABEL: &'static str = "Event Name";
    pub const MODAL_SS_INFO_DESC_ID: &'static str = "description";
    pub const MODAL_SS_INFO_DESC_LABEL: &'static str = "Event Description";
}
