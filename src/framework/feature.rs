use async_trait::async_trait;
use serenity::all::{CreateCommand, GatewayIntents};

use super::context::{AutocompleteCtx, InteractionCtx, MessageCtx};
use super::error::FeatureError;
use super::http::HttpCtx;
use super::request::{
    AutocompleteRequest, CommandRequest, ComponentRequest, MessageRequest, ModalRequest,
};

/// A self-contained bot capability (e.g. Secret Santa). Register it with
/// [`FeatureRegistry`](super::FeatureRegistry); the registry routes interactions to it.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait Feature: Send + Sync {
    /// Human-readable name, used in logs.
    fn name(&self) -> &'static str;

    /// Prefix of every custom_id this feature emits, e.g. "ss" → "ss:btn:start:5".
    /// The registry routes components and modals by this prefix.
    fn namespace(&self) -> &'static str;

    /// Top-level slash commands this feature owns. The registry routes by command name.
    fn commands(&self) -> Vec<CreateCommand> {
        vec![]
    }

    /// Gateway intents this feature needs. The bot requests the union over all features.
    /// `DIRECT_MESSAGES` or `GUILD_MESSAGES` also subscribes the feature to `on_message` for
    /// messages from DMs or servers.
    fn intents(&self) -> GatewayIntents {
        GatewayIntents::empty()
    }

    /// HTTP routes this feature serves. The registry nests them under
    /// `<base path>/<namespace>`, so `.route("/", ..)` in `tell` is served at `/jabot/tell`.
    fn http_routes(&self, _ctx: HttpCtx) -> Option<axum::Router> {
        None
    }

    async fn on_command(
        &self,
        _ctx: &InteractionCtx,
        _req: CommandRequest,
    ) -> Result<(), FeatureError> {
        Ok(())
    }

    async fn on_component(
        &self,
        _ctx: &InteractionCtx,
        _req: ComponentRequest,
    ) -> Result<(), FeatureError> {
        Ok(())
    }

    async fn on_modal(
        &self,
        _ctx: &InteractionCtx,
        _req: ModalRequest,
    ) -> Result<(), FeatureError> {
        Ok(())
    }

    /// A message from a user (never a bot) where this feature's `intents` apply.
    async fn on_message(
        &self,
        _ctx: &MessageCtx,
        _msg: MessageRequest,
    ) -> Result<(), FeatureError> {
        Ok(())
    }

    /// Suggestions for the option the user is typing into, as `(label, value)` pairs. The
    /// registry sends at most 25. Errors can't be shown to the user; they're logged.
    async fn on_autocomplete(
        &self,
        _ctx: &AutocompleteCtx,
        _req: AutocompleteRequest,
    ) -> Result<Vec<(String, String)>, FeatureError> {
        Ok(vec![])
    }
}
