//! kv: a per-user key-value store. `/k set <key>` stores the user's next DM to the bot under
//! the key; `/k get [key]` shows it again, or lists keys.

mod commands;
mod components;
mod custom_id;
mod messages;
mod model;
mod pending;
mod repo;
mod rules;
#[cfg(test)]
mod tests;
mod text;
mod views;

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serenity::all::{CreateCommand, GatewayIntents};

use crate::framework::{
    AutocompleteCtx, AutocompleteRequest, CommandRequest, ComponentRequest, Feature, FeatureError,
    InteractionCtx, MessageCtx, MessageRequest,
};
use custom_id::KvId;
use model::KvError;
use pending::PendingSets;
pub use repo::{KvRepo, SqliteKvRepo};

pub struct Kv {
    repo: Arc<dyn KvRepo>,
    /// `/k set`s waiting for the user's DM.
    pending: PendingSets,
}

impl Kv {
    pub fn new(repo: Arc<dyn KvRepo>) -> Self {
        Self {
            repo,
            pending: PendingSets::new(),
        }
    }
}

#[async_trait]
impl Feature for Kv {
    fn name(&self) -> &'static str {
        "kv"
    }

    fn namespace(&self) -> &'static str {
        custom_id::NAMESPACE
    }

    fn commands(&self) -> Vec<CreateCommand> {
        vec![commands::k_command()]
    }

    /// DMs to the bot, for `/k set`'s value. Not privileged: DMs include their text without
    /// `MESSAGE_CONTENT`.
    fn intents(&self) -> GatewayIntents {
        GatewayIntents::DIRECT_MESSAGES
    }

    async fn on_command(
        &self,
        ctx: &InteractionCtx,
        req: CommandRequest,
    ) -> Result<(), FeatureError> {
        let result = match req.subcommand.as_deref() {
            Some(text::SUB_SET) => commands::set(self, ctx, &req, Instant::now()).await,
            Some(text::SUB_GET) => commands::get(self, ctx, &req).await,
            other => Err(KvError::UnknownSubcommand(other.map(str::to_string))),
        };
        Ok(result?)
    }

    async fn on_component(
        &self,
        ctx: &InteractionCtx,
        req: ComponentRequest,
    ) -> Result<(), FeatureError> {
        let result = match req.custom_id.parse::<KvId>().map_err(KvError::from)? {
            KvId::Page { page, query } => components::page(self, ctx, req.user, page, query).await,
            KvId::Cancel(id) => components::cancel(self, ctx, req.user, id, Instant::now()).await,
            KvId::Delete(id) => components::delete(self, ctx, req.user, id, Instant::now()).await,
        };
        Ok(result?)
    }

    async fn on_message(&self, ctx: &MessageCtx, msg: MessageRequest) -> Result<(), FeatureError> {
        Ok(messages::receive(self, ctx, &msg, Instant::now()).await?)
    }

    async fn on_autocomplete(
        &self,
        _ctx: &AutocompleteCtx,
        req: AutocompleteRequest,
    ) -> Result<Vec<(String, String)>, FeatureError> {
        Ok(commands::suggest(self, &req).await?)
    }
}
