//! Everything kv shows in Discord. Pure functions of the model.

use serenity::all::{
    ButtonStyle, ChannelId, CreateActionRow, CreateAllowedMentions, CreateButton,
    CreateInteractionResponse, CreateInteractionResponseFollowup, CreateInteractionResponseMessage,
    CreateMessage, InteractionContext,
};

use super::custom_id::KvId;
use super::model::{Key, PendingId};
use super::pending::TIMEOUT;
use super::rules::Page;
use super::text;

/// Replies are ephemeral everywhere except the bot's DM, where they stay in the history.
pub fn is_ephemeral(context: Option<InteractionContext>) -> bool {
    context != Some(InteractionContext::BotDm)
}

/// No mentions: a value or key containing `@everyone` or `<@id>` pings nobody.
fn no_mentions() -> CreateAllowedMentions {
    CreateAllowedMentions::new()
}

fn message(content: impl Into<String>, ephemeral: bool) -> CreateInteractionResponseMessage {
    CreateInteractionResponseMessage::new()
        .content(content)
        .allowed_mentions(no_mentions())
        .ephemeral(ephemeral)
}

/// A stored value, verbatim, so it looks the way it was typed and can be copied as-is.
pub fn value(value: &str, ephemeral: bool) -> CreateInteractionResponse {
    CreateInteractionResponse::Message(message(value, ephemeral))
}

pub fn no_keys(ephemeral: bool) -> CreateInteractionResponse {
    CreateInteractionResponse::Message(message(text::NO_KEYS, ephemeral))
}

/// A new key list, as the reply to `/k get`.
pub fn key_list(page: &Page, query: Option<&str>, ephemeral: bool) -> CreateInteractionResponse {
    CreateInteractionResponse::Message(key_list_message(page, query).ephemeral(ephemeral))
}

/// The key list after Prev/Next. If every key has gone since, says so instead.
pub fn key_list_update(page: &Page, query: Option<&str>) -> CreateInteractionResponse {
    let message = if page.total == 0 {
        let content = match query {
            Some(query) => format!("No key contains `{query}` any more."),
            None => text::NO_KEYS.to_string(),
        };
        CreateInteractionResponseMessage::new()
            .content(content)
            .components(vec![])
    } else {
        key_list_message(page, query)
    };
    CreateInteractionResponse::UpdateMessage(message)
}

/// "Keys containing `pa` — page 2/3, 61 keys", one key per line, and Prev/Next when there's
/// more than one page.
fn key_list_message(page: &Page, query: Option<&str>) -> CreateInteractionResponseMessage {
    let title = match query {
        Some(query) => format!("Keys containing `{query}`"),
        None => "Your keys".to_string(),
    };
    let noun = if page.total == 1 { "key" } else { "keys" };
    let mut content = format!(
        "{title} — page {}/{}, {} {noun}",
        page.number + 1,
        page.count,
        page.total
    );
    for key in page.keys {
        content.push_str(&format!("\n`{key}`"));
    }

    let components = if page.count > 1 {
        let id = |page: usize| {
            KvId::Page {
                page,
                query: query.map(str::to_string),
            }
            .to_string()
        };
        let prev = CreateButton::new(id(page.number.saturating_sub(1)))
            .label(text::BTN_PREV)
            .style(ButtonStyle::Secondary)
            .disabled(!page.has_prev());
        let next = CreateButton::new(id(page.number + 1))
            .label(text::BTN_NEXT)
            .style(ButtonStyle::Secondary)
            .disabled(!page.has_next());
        vec![CreateActionRow::Buttons(vec![prev, next])]
    } else {
        vec![]
    };
    CreateInteractionResponseMessage::new()
        .content(content)
        .allowed_mentions(no_mentions())
        .components(components)
}

/// The `/k set` prompt: what to send, and Cancel (plus Delete if the key exists).
pub struct Prompt<'a> {
    pub key: &'a Key,
    pub id: PendingId,
    /// The key already has a value, which was sent just before the prompt.
    pub exists: bool,
}

impl Prompt<'_> {
    fn content(&self, replaced: Option<&Key>) -> String {
        let key = self.key;
        let minutes = TIMEOUT.as_secs() / 60;
        let mut content = if self.exists {
            format!(
                "↑ That's the current value of `{key}`. Your next message here replaces it; \
                 send it within {minutes} minutes."
            )
        } else {
            format!(
                "Send the value for `{key}` as your next message here, within {minutes} minutes."
            )
        };
        if let Some(replaced) = replaced {
            content.push_str(&format!(
                "\nThis replaces your unfinished `/k set` for `{replaced}`."
            ));
        }
        content
    }

    fn buttons(&self) -> Vec<CreateActionRow> {
        let mut buttons = vec![
            CreateButton::new(KvId::Cancel(self.id).to_string())
                .label(text::BTN_CANCEL)
                .style(ButtonStyle::Secondary),
        ];
        if self.exists {
            buttons.push(
                CreateButton::new(KvId::Delete(self.id).to_string())
                    .label(text::BTN_DELETE)
                    .style(ButtonStyle::Danger),
            );
        }
        vec![CreateActionRow::Buttons(buttons)]
    }

    /// As a DM, after a `/k set` elsewhere.
    pub fn dm(&self) -> CreateMessage {
        CreateMessage::new()
            .content(self.content(None))
            .allowed_mentions(no_mentions())
            .components(self.buttons())
    }

    /// As the reply to `/k set` in the bot's DM, for a new key.
    pub fn response(&self, replaced: Option<&Key>) -> CreateInteractionResponse {
        CreateInteractionResponse::Message(
            message(self.content(replaced), false).components(self.buttons()),
        )
    }

    /// After the existing value, when `/k set` is used in the bot's DM.
    pub fn followup(&self, replaced: Option<&Key>) -> CreateInteractionResponseFollowup {
        CreateInteractionResponseFollowup::new()
            .content(self.content(replaced))
            .allowed_mentions(no_mentions())
            .components(self.buttons())
    }
}

/// A stored value as a DM, verbatim.
pub fn value_dm(value: &str) -> CreateMessage {
    CreateMessage::new()
        .content(value)
        .allowed_mentions(no_mentions())
}

/// The reply to `/k set` outside the bot's DM: where to go, with a link there.
pub fn sent_to_dm(
    key: &Key,
    channel: ChannelId,
    replaced: Option<&Key>,
) -> CreateInteractionResponse {
    let mut content = format!(
        "I've DMed you. Send the value for `{key}` there within {} minutes.",
        TIMEOUT.as_secs() / 60
    );
    if let Some(replaced) = replaced {
        content.push_str(&format!(
            "\nThis replaces your unfinished `/k set` for `{replaced}`."
        ));
    }
    let link = CreateButton::new_link(format!("https://discord.com/channels/@me/{channel}"))
        .label(text::BTN_OPEN_DM);
    CreateInteractionResponse::Message(
        message(content, true).components(vec![CreateActionRow::Buttons(vec![link])]),
    )
}

pub fn dms_closed() -> CreateInteractionResponseFollowup {
    CreateInteractionResponseFollowup::new()
        .content(text::DMS_CLOSED)
        .ephemeral(true)
}

/// Replaces a prompt after Cancel or Delete, removing its buttons.
pub fn prompt_closed(content: impl Into<String>) -> CreateInteractionResponse {
    CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
            .content(content)
            .allowed_mentions(no_mentions())
            .components(vec![]),
    )
}

pub fn saved(key: &Key, dropped_extras: bool) -> CreateMessage {
    let mut content = format!("Saved `{key}`.");
    if dropped_extras {
        content.push_str(" Attachments and stickers aren't stored, only the text.");
    }
    CreateMessage::new()
        .content(content)
        .allowed_mentions(no_mentions())
}

pub fn timed_out(key: &Key) -> CreateMessage {
    CreateMessage::new()
        .content(format!(
            "Your `/k set` for `{key}` timed out, so I didn't store that. Run it again."
        ))
        .allowed_mentions(no_mentions())
}
