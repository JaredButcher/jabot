use async_trait::async_trait;
use serenity::all::CreateCommand;

use super::context::InteractionCtx;
use super::error::FeatureError;
use super::request::{CommandRequest, ComponentRequest, ModalRequest};

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
}
