//! Prev/Next on a key list, and Cancel/Delete on a `/k set` prompt.

use std::time::Instant;

use serenity::all::UserId;

use super::model::{KvError, PendingId};
use super::{Kv, rules, text, views};
use crate::framework::InteractionCtx;

/// Show page `page` of the presser's keys containing `query`, reloaded, so the list is never
/// stale.
pub async fn page(
    feature: &Kv,
    ctx: &InteractionCtx,
    user: UserId,
    page: usize,
    query: Option<String>,
) -> Result<(), KvError> {
    let keys = feature.repo.keys(user, query.as_deref()).await?;
    let page = rules::paginate(&keys, page);
    ctx.responder
        .respond(views::key_list_update(&page, query.as_deref()))
        .await?;
    Ok(())
}

/// End `/k set` `id` without storing anything.
pub async fn cancel(
    feature: &Kv,
    ctx: &InteractionCtx,
    user: UserId,
    id: PendingId,
    now: Instant,
) -> Result<(), KvError> {
    let content = match feature.pending.finish(user, id, now) {
        Some(key) => format!("Cancelled; `{key}` is unchanged."),
        None => text::PROMPT_EXPIRED.to_string(),
    };
    ctx.responder.respond(views::prompt_closed(content)).await?;
    Ok(())
}

/// End `/k set` `id` and delete its key. Works only while the set is waiting, so an old
/// prompt can't delete a value stored since.
pub async fn delete(
    feature: &Kv,
    ctx: &InteractionCtx,
    user: UserId,
    id: PendingId,
    now: Instant,
) -> Result<(), KvError> {
    let content = match feature.pending.finish(user, id, now) {
        Some(key) => match feature.repo.delete(user, key.as_str()).await? {
            true => format!("Deleted `{key}`."),
            false => format!("`{key}` was already deleted."),
        },
        None => text::PROMPT_EXPIRED.to_string(),
    };
    ctx.responder.respond(views::prompt_closed(content)).await?;
    Ok(())
}
