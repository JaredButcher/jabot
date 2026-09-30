//! Secret Santa: hosts create an event, add participants, and start it to draw assignments.

mod commands;
mod components;
mod custom_id;
mod modals;
mod model;
mod repo;
#[cfg(test)]
mod tests;
mod text;
mod views;

use std::sync::Arc;

use async_trait::async_trait;
use rand::SeedableRng;
use rand::rngs::StdRng;
use serenity::all::{CreateCommand, CreateInteractionResponse};

use crate::framework::{
    CommandRequest, ComponentRequest, Feature, FeatureError, InteractionCtx, ModalRequest,
};
use custom_id::SsId;
pub use repo::{SecretSantaRepo, SqliteSecretSantaRepo};

pub struct SecretSanta {
    repo: Arc<dyn SecretSantaRepo>,
}

impl SecretSanta {
    pub fn new(repo: Arc<dyn SecretSantaRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl Feature for SecretSanta {
    fn name(&self) -> &'static str {
        "Secret Santa"
    }

    fn namespace(&self) -> &'static str {
        custom_id::NAMESPACE
    }

    fn commands(&self) -> Vec<CreateCommand> {
        vec![commands::ss_command()]
    }

    async fn on_command(
        &self,
        ctx: &InteractionCtx,
        req: CommandRequest,
    ) -> Result<(), FeatureError> {
        match req.subcommand.as_deref() {
            Some(text::CMD_CREATE) => commands::create(self, ctx, &req).await,
            Some(text::CMD_INFO) => commands::info(self, ctx, &req).await,
            Some(text::CMD_LIST) => commands::list(self, ctx, &req).await,
            other => Err(FeatureError::internal(format!(
                "unknown /ss subcommand {other:?}"
            ))),
        }
    }

    async fn on_component(
        &self,
        ctx: &InteractionCtx,
        req: ComponentRequest,
    ) -> Result<(), FeatureError> {
        match req.custom_id.parse::<SsId>()? {
            SsId::UserSelect(id) => components::set_participants(self, ctx, id, &req).await,
            SsId::Start(id) => {
                let mut rng = StdRng::from_os_rng();
                components::start(self, ctx, id, req.user, &mut rng).await
            }
            SsId::End(id) => components::end(self, ctx, id, req.user).await,
            SsId::Cancel(id) => components::cancel(self, ctx, id, req.user).await,
            other => Err(FeatureError::internal(format!(
                "{other:?} is not a component"
            ))),
        }
    }

    async fn on_modal(&self, ctx: &InteractionCtx, req: ModalRequest) -> Result<(), FeatureError> {
        match req.custom_id.parse::<SsId>()? {
            SsId::CreateModal => modals::create(self, ctx, &req).await,
            SsId::EditModal(id) => modals::edit(self, ctx, id, &req).await,
            other => Err(FeatureError::internal(format!("{other:?} is not a modal"))),
        }
    }
}

async fn respond(
    ctx: &InteractionCtx,
    response: CreateInteractionResponse,
) -> Result<(), FeatureError> {
    ctx.responder.respond(response).await?;
    Ok(())
}

/// Reply with a short ephemeral message.
async fn reply(ctx: &InteractionCtx, content: &str) -> Result<(), FeatureError> {
    respond(ctx, views::ephemeral(content)).await
}
