//! Mockable access to Discord. Features never see serenity's `Context` or `Http`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use serenity::all::{
    ChannelId, CreateInteractionResponse, CreateInteractionResponseFollowup, CreateMessage, Http,
    InteractionId, UserId,
};
use serenity::builder::Builder;
use serenity::http::HttpError;

pub type DiscordError = serenity::Error;

/// Discord's error code for "Cannot send messages to this user": the user has DMs from server
/// members turned off, or shares no server with the bot.
const CANNOT_MESSAGE_USER: isize = 50007;

#[derive(Debug, thiserror::Error)]
pub enum DmError {
    #[error("the user doesn't accept DMs from the bot")]
    Closed,
    #[error(transparent)]
    Discord(DiscordError),
}

impl From<DiscordError> for DmError {
    fn from(error: DiscordError) -> Self {
        match &error {
            serenity::Error::Http(HttpError::UnsuccessfulRequest(response))
                if response.error.code == CANNOT_MESSAGE_USER =>
            {
                DmError::Closed
            }
            _ => DmError::Discord(error),
        }
    }
}

/// Replies to the one interaction being handled.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait Responder: Send + Sync {
    /// The initial response. Discord requires it within 3 seconds.
    async fn respond(&self, response: CreateInteractionResponse) -> Result<(), DiscordError>;
    /// An additional message after the initial response.
    async fn followup(
        &self,
        message: CreateInteractionResponseFollowup,
    ) -> Result<(), DiscordError>;
    fn has_responded(&self) -> bool;
}

/// Bot-wide Discord calls that aren't tied to an interaction.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait DiscordApi: Send + Sync {
    async fn send_dm(&self, user: UserId, content: String) -> Result<(), DmError>;
    async fn user_name(&self, user: UserId) -> Result<String, DiscordError>;
    /// The DM channel with `user`, creating it if needed. Doesn't check that the user accepts
    /// DMs; sending to it does.
    async fn dm_channel(&self, user: UserId) -> Result<ChannelId, DiscordError>;
    /// Send `message` to `channel`. `DmError::Closed` if it's a DM channel whose user doesn't
    /// accept DMs from the bot.
    async fn send_message(&self, channel: ChannelId, message: CreateMessage)
    -> Result<(), DmError>;
}

pub struct SerenityResponder {
    http: Arc<Http>,
    id: InteractionId,
    token: String,
    responded: AtomicBool,
}

impl SerenityResponder {
    pub fn new(http: Arc<Http>, id: InteractionId, token: String) -> Self {
        Self {
            http,
            id,
            token,
            responded: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl Responder for SerenityResponder {
    async fn respond(&self, response: CreateInteractionResponse) -> Result<(), DiscordError> {
        response.execute(&self.http, (self.id, &self.token)).await?;
        self.responded.store(true, Ordering::Release);
        Ok(())
    }

    async fn followup(
        &self,
        message: CreateInteractionResponseFollowup,
    ) -> Result<(), DiscordError> {
        message.execute(&self.http, (None, &self.token)).await?;
        Ok(())
    }

    fn has_responded(&self) -> bool {
        self.responded.load(Ordering::Acquire)
    }
}

pub struct SerenityDiscordApi {
    http: Arc<Http>,
}

impl SerenityDiscordApi {
    pub fn new(http: Arc<Http>) -> Self {
        Self { http }
    }
}

#[async_trait]
impl DiscordApi for SerenityDiscordApi {
    async fn send_dm(&self, user: UserId, content: String) -> Result<(), DmError> {
        user.direct_message(&self.http, CreateMessage::new().content(content))
            .await?;
        Ok(())
    }

    async fn user_name(&self, user: UserId) -> Result<String, DiscordError> {
        Ok(user.to_user(&self.http).await?.name)
    }

    async fn dm_channel(&self, user: UserId) -> Result<ChannelId, DiscordError> {
        Ok(user.create_dm_channel(&self.http).await?.id)
    }

    async fn send_message(
        &self,
        channel: ChannelId,
        message: CreateMessage,
    ) -> Result<(), DmError> {
        channel.send_message(&self.http, message).await?;
        Ok(())
    }
}
