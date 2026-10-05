//! DMs to the bot: the value for a waiting `/k set`.

use std::time::Instant;

use serenity::all::CreateMessage;

use super::model::KvError;
use super::pending::Current;
use super::{Kv, rules, views};
use crate::framework::{MessageCtx, MessageRequest};

/// Store `msg` under the author's waiting `/k set`, if there is one. DMs with nothing waiting
/// are ignored, so the bot doesn't answer every DM.
pub async fn receive(
    feature: &Kv,
    ctx: &MessageCtx,
    msg: &MessageRequest,
    now: Instant,
) -> Result<(), KvError> {
    if msg.guild.is_some() {
        return Ok(());
    }
    let pending = match feature.pending.current(msg.author, now) {
        Current::Nothing => return Ok(()),
        Current::Expired(key) => return reply(ctx, msg, views::timed_out(&key)).await,
        Current::Active(pending) => pending,
    };
    // A refusal leaves the set waiting, so the user can just send again.
    rules::check_value(&msg.content)?;
    // `None`: a DM sent at the same moment, Cancel or Delete got there first.
    let Some(key) = feature.pending.finish(msg.author, pending.id, now) else {
        return Ok(());
    };
    // The user has a `users` row: `/k set` was an interaction, and the registry ensured it.
    feature
        .repo
        .set(msg.author, key.as_str(), &msg.content)
        .await?;
    reply(ctx, msg, views::saved(&key, msg.extras > 0)).await
}

async fn reply(
    ctx: &MessageCtx,
    msg: &MessageRequest,
    reply: CreateMessage,
) -> Result<(), KvError> {
    ctx.discord.send_message(msg.channel, reply).await?;
    Ok(())
}
