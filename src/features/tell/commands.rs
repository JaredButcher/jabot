//! `/tell`: definition and handler.

use serenity::all::{CreateCommand, InteractionContext};

use super::{Tell, text, token, views};
use crate::framework::{CommandRequest, DmError, FeatureError, InteractionCtx};

pub fn tell_command() -> CreateCommand {
    CreateCommand::new(text::CMD_TELL)
        .description(text::CMD_TELL_DESC)
        .contexts(vec![
            InteractionContext::Guild,
            InteractionContext::BotDm,
            InteractionContext::PrivateChannel,
        ])
}

/// Show the user's token, creating one if they have none. A new token also gets a test DM,
/// so a user with closed DMs finds out now rather than when their task finishes.
pub async fn tell(
    feature: &Tell,
    ctx: &InteractionCtx,
    req: &CommandRequest,
) -> Result<(), FeatureError> {
    let stored = feature
        .repo
        .get_or_create(req.user, token::generate())
        .await?;
    ctx.responder
        .respond(views::tell_reply(&feature.config, &stored))
        .await?;

    if stored.created {
        match ctx.discord.send_dm(req.user, text::TEST_DM.into()).await {
            Ok(()) => {}
            Err(DmError::Closed) => ctx.responder.followup(views::dms_closed()).await?,
            Err(error) => tracing::warn!(user = %req.user, %error, "could not send tell test DM"),
        }
    }
    Ok(())
}
