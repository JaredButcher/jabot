//! Mockable access to Discord. Features never see serenity's `Context` or `Http`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use serenity::all::{
    CreateInteractionResponse, CreateInteractionResponseFollowup, CreateMessage, Http,
    InteractionId, UserId,
};
use serenity::builder::Builder;

pub type DiscordError = serenity::Error;

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
    async fn send_dm(&self, user: UserId, content: String) -> Result<(), DiscordError>;
    async fn user_name(&self, user: UserId) -> Result<String, DiscordError>;
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
    async fn send_dm(&self, user: UserId, content: String) -> Result<(), DiscordError> {
        user.direct_message(&self.http, CreateMessage::new().content(content))
            .await?;
        Ok(())
    }

    async fn user_name(&self, user: UserId) -> Result<String, DiscordError> {
        Ok(user.to_user(&self.http).await?.name)
    }
}
