//! Host controls from the `/ss info` view: participant picker and start/end/cancel buttons.

use std::collections::HashSet;

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use serenity::all::{CreateInteractionResponse, UserId};

use super::model::{EventId, EventStatus};
use super::{SecretSanta, reply, respond, text, views};
use crate::framework::{ComponentKind, ComponentRequest, FeatureError, InteractionCtx};

/// The host's participant picker was submitted: make the participant list match it.
pub async fn set_participants(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    id: EventId,
    req: &ComponentRequest,
) -> Result<(), FeatureError> {
    let ComponentKind::UserSelect(selected) = &req.kind else {
        return Err(FeatureError::internal(format!(
            "participant picker sent {:?}",
            req.kind
        )));
    };
    let event = ss
        .repo
        .get_event(id)
        .await?
        .ok_or_else(|| FeatureError::internal(format!("event {id} not found")))?;
    if event.status != EventStatus::PreRun {
        return reply(ctx, text::PARTICIPANTS_LOCKED).await;
    }

    let existing: HashSet<UserId> = ss
        .repo
        .participants(id)
        .await?
        .iter()
        .map(|p| p.user)
        .collect();
    let selected_set: HashSet<UserId> = selected.iter().copied().collect();
    let add: Vec<UserId> = selected
        .iter()
        .copied()
        .filter(|user| !existing.contains(user))
        .collect();
    let remove: Vec<UserId> = existing
        .iter()
        .copied()
        .filter(|user| !selected_set.contains(user))
        .collect();

    ctx.users.ensure(&add).await?;
    ss.repo.set_participants(id, &add, &remove).await?;
    respond(ctx, CreateInteractionResponse::Acknowledge).await?;

    // Notify after acknowledging, so slow DMs can't time out the interaction.
    for user in add {
        ctx.discord.send_dm(user, views::invite_dm(&event)).await?;
    }
    Ok(())
}

/// Draw assignments and tell every participant who they give a gift to.
pub async fn start(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    id: EventId,
    user: UserId,
    rng: &mut StdRng,
) -> Result<(), FeatureError> {
    let Some(event) = ss.repo.get_event(id).await? else {
        return reply(ctx, text::EVENT_NOT_FOUND).await;
    };
    if event.host != user {
        return reply(ctx, text::NOT_HOST).await;
    }
    if event.status != EventStatus::PreRun {
        return reply(ctx, text::NOT_PREPARING).await;
    }
    let participants = ss.repo.participants(id).await?;
    if participants.len() < 2 {
        return reply(ctx, text::NOT_ENOUGH_PARTICIPANTS).await;
    }

    // Shuffle, then each participant gives to the next one in the list.
    let mut shuffled: Vec<UserId> = participants.iter().map(|p| p.user).collect();
    shuffled.shuffle(rng);
    let assignments: Vec<(UserId, UserId)> = (0..shuffled.len())
        .map(|i| (shuffled[i], shuffled[(i + 1) % shuffled.len()]))
        .collect();

    // Status and assignments are saved together before anyone is told anything.
    if !ss.repo.start_event(id, &assignments).await? {
        return reply(ctx, text::ALREADY_STARTED).await;
    }
    reply(ctx, text::STARTED).await?;

    for (santa, recipient) in assignments {
        ctx.discord
            .send_dm(santa, views::started_dm(&event, recipient))
            .await?;
    }
    Ok(())
}

pub async fn end(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    id: EventId,
    user: UserId,
) -> Result<(), FeatureError> {
    let Some(event) = ss.repo.get_event(id).await? else {
        return reply(ctx, text::EVENT_NOT_FOUND).await;
    };
    if event.host != user {
        return reply(ctx, text::NOT_HOST).await;
    }
    if event.status != EventStatus::Running
        || !ss
            .repo
            .transition(id, &[EventStatus::Running], EventStatus::Finished)
            .await?
    {
        return reply(ctx, text::NOT_RUNNING).await;
    }
    let participants = ss.repo.participants(id).await?;
    reply(ctx, text::ENDED).await?;

    for participant in participants {
        if let Err(why) = ctx
            .discord
            .send_dm(participant.user, views::ended_dm(&event))
            .await
        {
            println!("End notification error {}", why);
        }
    }
    Ok(())
}

pub async fn cancel(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    id: EventId,
    user: UserId,
) -> Result<(), FeatureError> {
    let Some(event) = ss.repo.get_event(id).await? else {
        return reply(ctx, text::EVENT_NOT_FOUND).await;
    };
    if event.host != user {
        return reply(ctx, text::NOT_HOST).await;
    }
    if event.status == EventStatus::Finished {
        return reply(ctx, text::ALREADY_FINISHED).await;
    }
    // Only from the status we just read, so we know whether assignments went out.
    if !ss
        .repo
        .transition(id, &[event.status], EventStatus::Finished)
        .await?
    {
        return reply(ctx, text::CHANGED_WHILE_CANCELING).await;
    }
    reply(ctx, text::CANCELED).await?;

    // Participants only know about their assignments once the event is running.
    if event.status == EventStatus::Running {
        for participant in ss.repo.participants(id).await? {
            ctx.discord
                .send_dm(participant.user, views::canceled_dm(&event))
                .await?;
        }
    }
    Ok(())
}
