//! The Revoke token button on a `/tell` reply.

use serenity::all::UserId;

use super::model::TokenId;
use super::{Tell, views};
use crate::framework::{FeatureError, InteractionCtx};

/// Revoke token `id` if it is still the user's, and replace the reply either way.
pub async fn revoke(
    feature: &Tell,
    ctx: &InteractionCtx,
    id: TokenId,
    user: UserId,
) -> Result<(), FeatureError> {
    let revoked = feature.repo.revoke(user, id).await?;
    ctx.responder.respond(views::revoked(revoked)).await?;
    Ok(())
}
