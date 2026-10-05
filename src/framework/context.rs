use std::sync::Arc;

use super::discord::{DiscordApi, Responder};
use super::users::UserRepo;

/// Dependencies handed to a feature for one interaction. Every field is a trait object, so
/// tests can pass mocks.
pub struct InteractionCtx {
    pub responder: Arc<dyn Responder>,
    pub discord: Arc<dyn DiscordApi>,
    pub users: Arc<dyn UserRepo>,
}

/// Dependencies handed to a feature for one message. There's no responder: a feature replies
/// with `discord.send_message` to the message's channel.
pub struct MessageCtx {
    pub discord: Arc<dyn DiscordApi>,
    pub users: Arc<dyn UserRepo>,
}
