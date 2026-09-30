//! Host controls from the `/ss info` view: participant picker and start/end/cancel buttons.

use rand::rngs::StdRng;
use serenity::all::{CreateInteractionResponse, UserId};

use super::model::{Event, EventId, EventStatus, SsError};
use super::{SecretSanta, notify, reply, respond, rules, text, views};
use crate::framework::{ComponentKind, ComponentRequest, InteractionCtx};

async fn load(ss: &SecretSanta, id: EventId) -> Result<Event, SsError> {
    ss.repo.get_event(id).await?.ok_or(SsError::EventNotFound)
}

/// The host's participant picker was submitted: make the participant list match it.
pub async fn set_participants(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    id: EventId,
    req: &ComponentRequest,
) -> Result<(), SsError> {
    let ComponentKind::UserSelect(selected) = &req.kind else {
        return Err(SsError::UnexpectedComponent(req.kind.clone()));
    };
    let event = load(ss, id).await?;
    rules::ensure_host(&event, req.user)?;
    rules::ensure_participants_editable(&event)?;

    let existing: Vec<UserId> = ss
        .repo
        .participants(id)
        .await?
        .iter()
        .map(|p| p.user)
        .collect();
    let (add, remove) = rules::participant_diff(&existing, selected, event.host)?;
    tracing::info!(event = %id, ?add, ?remove, "updating participants");

    ctx.users.ensure(&add).await?;
    ss.repo.set_participants(id, &add, &remove).await?;
    respond(ctx, CreateInteractionResponse::Acknowledge).await?;

    // Notify after acknowledging, so slow DMs can't time out the interaction.
    let invite = views::invite_dm(&event);
    notify(ctx, add.into_iter().map(|user| (user, invite.clone()))).await
}

/// Draw assignments and tell every participant who they give a gift to.
pub async fn start(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    id: EventId,
    user: UserId,
    rng: &mut StdRng,
) -> Result<(), SsError> {
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
        return Err(SsError::AlreadyStarted);
    }
    reply(ctx, text::STARTED).await?;

    let dms = assignments
        .into_iter()
        .map(|(santa, recipient)| (santa, views::started_dm(&event, recipient)));
    notify(ctx, dms).await
}

pub async fn end(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    id: EventId,
    user: UserId,
) -> Result<(), SsError> {
    let event = load(ss, id).await?;
    rules::ensure_host(&event, user)?;
    rules::ensure_can_end(&event)?;
    if !ss
        .repo
        .transition(id, &[event.status], EventStatus::Finished)
        .await?
    {
        return Err(SsError::EventChanged);
    }
    let participants = ss.repo.participants(id).await?;
    reply(ctx, text::ENDED).await?;

    let ended = views::ended_dm(&event);
    notify(
        ctx,
        participants.into_iter().map(|p| (p.user, ended.clone())),
    )
    .await
}

pub async fn cancel(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    id: EventId,
    user: UserId,
) -> Result<(), SsError> {
    let event = load(ss, id).await?;
    rules::ensure_host(&event, user)?;
    rules::ensure_can_cancel(&event)?;
    // Only from the status we just read, so we know whether assignments went out.
    if !ss
        .repo
        .transition(id, &[event.status], EventStatus::Finished)
        .await?
    {
        return Err(SsError::EventChanged);
    }
    reply(ctx, text::CANCELED).await?;

    // Participants only know about their assignments once the event is running.
    if event.status != EventStatus::Running {
        return Ok(());
    }
    let canceled = views::canceled_dm(&event);
    let participants = ss.repo.participants(id).await?;
    notify(
        ctx,
        participants.into_iter().map(|p| (p.user, canceled.clone())),
    )
    .await
}
