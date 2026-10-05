//! `/k`: definition, subcommand handlers, and key suggestions for autocomplete.

use std::time::Instant;

use serenity::all::{CommandOptionType, CreateCommand, CreateCommandOption, InteractionContext};

use super::model::KvError;
use super::rules::{self, MAX_KEY_CHARS, MAX_KEYS, Resolution, SUGGESTIONS};
use super::views::Prompt;
use super::{Kv, text, views};
use crate::framework::{AutocompleteRequest, CommandRequest, DmError, InteractionCtx};

fn key_option(description: &str, required: bool) -> CreateCommandOption {
    CreateCommandOption::new(CommandOptionType::String, text::OPT_KEY, description)
        .required(required)
        .min_length(1)
        .max_length(MAX_KEY_CHARS as u16)
        .set_autocomplete(true)
}

pub fn k_command() -> CreateCommand {
    CreateCommand::new(text::CMD_K)
        .description(text::CMD_K_DESC)
        .contexts(vec![
            InteractionContext::Guild,
            InteractionContext::BotDm,
            InteractionContext::PrivateChannel,
        ])
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                text::SUB_SET,
                text::SUB_SET_DESC,
            )
            .add_sub_option(key_option(text::OPT_KEY_SET_DESC, true)),
        )
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::SubCommand,
                text::SUB_GET,
                text::SUB_GET_DESC,
            )
            .add_sub_option(key_option(text::OPT_KEY_GET_DESC, false)),
        )
}

/// `/k set <key>`: show the existing value (if any) and a prompt in the bot's DM, then wait
/// for the user's next DM there (see `messages::receive`).
pub async fn set(
    feature: &Kv,
    ctx: &InteractionCtx,
    req: &CommandRequest,
    now: Instant,
) -> Result<(), KvError> {
    let raw = req.options.str(text::OPT_KEY).ok_or(KvError::MissingKey)?;
    let key = rules::normalize_key(raw)?;
    let existing = feature.repo.get(req.user, key.as_str()).await?;
    if existing.is_none() && feature.repo.count(req.user).await? >= MAX_KEYS {
        return Err(KvError::TooManyKeys);
    }
    // Before `start`, so a failure here leaves nothing waiting.
    let channel = if req.context == Some(InteractionContext::BotDm) {
        None
    } else {
        Some(ctx.discord.dm_channel(req.user).await?)
    };

    let (id, replaced) = feature.pending.start(req.user, key.clone(), now);
    let replaced = replaced.filter(|old| *old != key);
    let prompt = Prompt {
        key: &key,
        id,
        exists: existing.is_some(),
    };

    let Some(channel) = channel else {
        match &existing {
            Some(value) => {
                ctx.responder.respond(views::value(value, false)).await?;
                ctx.responder
                    .followup(prompt.followup(replaced.as_ref()))
                    .await?;
            }
            None => {
                ctx.responder
                    .respond(prompt.response(replaced.as_ref()))
                    .await?
            }
        }
        return Ok(());
    };

    ctx.responder
        .respond(views::sent_to_dm(&key, channel, replaced.as_ref()))
        .await?;
    let sent = async {
        if let Some(value) = &existing {
            ctx.discord
                .send_message(channel, views::value_dm(value))
                .await?;
        }
        ctx.discord.send_message(channel, prompt.dm()).await
    }
    .await;
    if let Err(error) = sent {
        feature.pending.finish(req.user, id, now);
        match error {
            DmError::Closed => ctx.responder.followup(views::dms_closed()).await?,
            error => return Err(error.into()),
        }
    }
    Ok(())
}

/// `/k get [key]`: no key lists every key; otherwise see `rules::resolve`.
pub async fn get(feature: &Kv, ctx: &InteractionCtx, req: &CommandRequest) -> Result<(), KvError> {
    let ephemeral = views::is_ephemeral(req.context);
    let response = match req.options.str(text::OPT_KEY) {
        None => {
            let keys = feature.repo.keys(req.user, None).await?;
            if keys.is_empty() {
                views::no_keys(ephemeral)
            } else {
                views::key_list(&rules::paginate(&keys, 0), None, ephemeral)
            }
        }
        Some(raw) => {
            let query = rules::normalize_key(raw)?;
            let matches = feature.repo.keys(req.user, Some(query.as_str())).await?;
            match rules::resolve(&query, matches) {
                Resolution::Show(key) => match feature.repo.get(req.user, key.as_str()).await? {
                    Some(value) => views::value(&value, ephemeral),
                    // Deleted between the two queries.
                    None => return Err(KvError::NotFound(query)),
                },
                Resolution::List(keys) => {
                    views::key_list(&rules::paginate(&keys, 0), Some(query.as_str()), ephemeral)
                }
                Resolution::NotFound => return Err(KvError::NotFound(query)),
            }
        }
    };
    ctx.responder.respond(response).await?;
    Ok(())
}

/// The user's keys containing what they've typed into a `key` option, as `(label, value)`.
pub async fn suggest(
    feature: &Kv,
    req: &AutocompleteRequest,
) -> Result<Vec<(String, String)>, KvError> {
    if req.focused != text::OPT_KEY {
        return Ok(vec![]);
    }
    let partial = rules::normalize_partial(&req.partial);
    let keys = feature
        .repo
        .suggest(req.user, &partial, SUGGESTIONS)
        .await?;
    Ok(keys.into_iter().map(|key| (key.clone(), key)).collect())
}
