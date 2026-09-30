//! Host controls from the `/ss info` view: participant picker and start/end/cancel buttons.

use rand::rngs::StdRng;
use serenity::all::{CreateInteractionResponse, UserId};

use super::model::{Event, EventId, EventStatus, SsError};
use super::{SecretSanta, reply, respond, rules, text, views};
use crate::framework::{ComponentKind, ComponentRequest, FeatureError, InteractionCtx};

async fn load(ss: &SecretSanta, id: EventId) -> Result<Event, FeatureError> {
    Ok(ss.repo.get_event(id).await?.ok_or(SsError::EventNotFound)?)
}

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
    let event = load(ss, id).await?;
    rules::ensure_participants_editable(&event)?;

    let existing: Vec<UserId> = ss
        .repo
        .participants(id)
        .await?
        .iter()
        .map(|p| p.user)
        .collect();
    let (add, remove) = rules::participant_diff(&existing, selected);

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
    let event = load(ss, id).await?;
    rules::ensure_host(&event, user)?;
    rules::ensure_can_start(&event)?;
    let participants: Vec<UserId> = ss
        .repo
        .participants(id)
        .await?
        .iter()
        .map(|p| p.user)
        .collect();
    let assignments = rules::assign_santas(&participants, rng)?;

    // Status and assignments are saved together before anyone is told anything.
    if !ss.repo.start_event(id, &assignments).await? {
        return Err(SsError::AlreadyStarted.into());
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
    let event = load(ss, id).await?;
    rules::ensure_host(&event, user)?;
    rules::ensure_can_end(&event)?;
    if !ss
        .repo
        .transition(id, &[event.status], EventStatus::Finished)
        .await?
    {
        return Err(SsError::EventChanged.into());
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
    let event = load(ss, id).await?;
    rules::ensure_host(&event, user)?;
    rules::ensure_can_cancel(&event)?;
    // Only from the status we just read, so we know whether assignments went out.
    if !ss
        .repo
        .transition(id, &[event.status], EventStatus::Finished)
        .await?
    {
        return Err(SsError::EventChanged.into());
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
