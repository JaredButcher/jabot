//! `/ss` slash command: definition and subcommand handlers.

use serenity::all::{CommandOptionType, CreateCommand, CreateCommandOption, InteractionContext};

use super::model::EventId;
use super::{SecretSanta, reply, respond, text, views};
use crate::framework::{CommandRequest, FeatureError, InteractionCtx};

pub fn ss_command() -> CreateCommand {
    let event_id = || {
        CreateCommandOption::new(
            CommandOptionType::Integer,
            text::OPT_EVENT_ID,
            text::OPT_EVENT_ID_DESC,
        )
    };
    CreateCommand::new(text::CMD_SS)
        .description(text::CMD_SS_DESC)
        .contexts(vec![
            InteractionContext::Guild,
            InteractionContext::BotDm,
            InteractionContext::PrivateChannel,
        ])
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                text::CMD_CREATE,
                text::CMD_CREATE_DESC,
            )
            .add_sub_option(event_id().required(false)),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                text::CMD_INFO,
                text::CMD_INFO_DESC,
            )
            .add_sub_option(event_id().required(true)),
        )
        .add_option(CreateCommandOption::new(
            CommandOptionType::SubCommand,
            text::CMD_LIST,
            text::CMD_LIST_DESC,
        ))
}

/// `/ss create` opens the create form; `/ss create id:<n>` opens the edit form for the host.
pub async fn create(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    req: &CommandRequest,
) -> Result<(), FeatureError> {
    let Some(id) = req.options.i64(text::OPT_EVENT_ID) else {
        return respond(ctx, views::create_form()).await;
    };
    let Some(event) = ss.repo.get_event(EventId(id)).await? else {
        return reply(ctx, text::EVENT_NOT_FOUND).await;
    };
    if event.host != req.user {
        return reply(ctx, text::NOT_HOST).await;
    }
    respond(ctx, views::edit_form(&event)).await
}

pub async fn info(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    req: &CommandRequest,
) -> Result<(), FeatureError> {
    let Some(id) = req.options.i64(text::OPT_EVENT_ID) else {
        return reply(ctx, text::MISSING_EVENT_ID).await;
    };
    let id = EventId(id);
    let event = ss.repo.get_event(id).await?;
    let participants = ss.repo.participants(id).await?;
    let is_participant = participants.iter().any(|p| p.user == req.user);
    match event {
        Some(event) if is_participant => {
            respond(ctx, views::event_info(&event, &participants, req.user)).await
        }
        _ => reply(ctx, text::NOT_PARTICIPANT).await,
    }
}

pub async fn list(
    ss: &SecretSanta,
    ctx: &InteractionCtx,
    req: &CommandRequest,
) -> Result<(), FeatureError> {
    let mut rows = vec![];
    for event in ss.repo.events_for_user(req.user).await? {
        let host_name = match ctx.discord.user_name(event.host).await {
            Ok(name) => name,
            Err(_) => format!("Unknown User ({})", event.host),
        };
        rows.push((event, host_name));
    }
    respond(ctx, views::event_list(&rows)).await
}
