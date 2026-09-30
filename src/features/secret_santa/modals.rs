//! Create and edit form submissions.

use super::model::{EventId, SsError};
use super::{SecretSanta, respond, rules, text, views};
use crate::framework::{FeatureError, InteractionCtx, ModalRequest};

/// Most unfinished events one host may have at a time.
pub const HOST_EVENT_LIMIT: i64 = 32;

pub async fn create(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    req: &ModalRequest,
) -> Result<(), FeatureError> {
    let name = req.field(text::FIELD_NAME).unwrap_or(text::DEFAULT_NAME);
    let description = req
        .field(text::FIELD_DESCRIPTION)
        .unwrap_or(text::DEFAULT_DESCRIPTION);

    if ss.repo.count_active_hosted(req.user).await? >= HOST_EVENT_LIMIT {
        return respond(ctx, views::event_limit(HOST_EVENT_LIMIT)).await;
    }
    let id = ss.repo.create_event(req.user, name, description).await?;
    respond(ctx, views::created(id, name)).await
}

pub async fn edit(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    id: EventId,
    req: &ModalRequest,
) -> Result<(), FeatureError> {
    let event = ss.repo.get_event(id).await?.ok_or(SsError::EventNotFound)?;
    rules::ensure_host(&event, req.user)?;

    let name = req.field(text::FIELD_NAME).unwrap_or(&event.name);
    let description = req
        .field(text::FIELD_DESCRIPTION)
        .map(str::to_string)
        .or(event.description.clone());
    ss.repo.update_event(id, name, description).await?;
    respond(ctx, views::updated(name)).await
}
