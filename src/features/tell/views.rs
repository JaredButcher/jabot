//! Everything tell shows in Discord. Pure functions of the model.

use serenity::all::{
    ButtonStyle, CreateActionRow, CreateButton, CreateInteractionResponse,
    CreateInteractionResponseFollowup, CreateInteractionResponseMessage,
};

use super::TellConfig;
use super::custom_id::TellId;
use super::model::StoredToken;
use super::text;

/// The `/tell` reply: the user's token, ready-to-run commands, and a Revoke button.
pub fn tell_reply(config: &TellConfig, token: &StoredToken) -> CreateInteractionResponse {
    let url = &config.url;
    let token_value = &token.token;
    let mut content = format!(
        "Your tell token: `{token_value}`\n\
         Anyone with it can DM you through me, so keep it private.\n\
         \n\
         Send yourself a message:\n\
         ```sh\n\
         curl -sS --fail-with-body {url} \\\n     \
         --json '{{\"token\": \"{token_value}\", \"message\": \"Task finished\"}}'\n\
         ```\n\
         Shell helper (needs `jq`), e.g. `long_task; tell \"long_task exited with $?\"`:\n\
         ```sh\n\
         tell() {{ jq -nc --arg token {token_value} --arg message \"${{*:-done}}\" '$ARGS.named' | \
         curl -sS --fail-with-body {url} --json @-; }}\n\
         ```"
    );
    if let Some(lan_url) = &config.lan_url {
        content.push_str(&format!("\nOn the LAN, use {lan_url} instead."));
    }

    let revoke = CreateButton::new(TellId::Revoke(token.id).to_string())
        .label(text::BTN_REVOKE)
        .style(ButtonStyle::Danger);
    CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(content)
            .components(vec![CreateActionRow::Buttons(vec![revoke])])
            .ephemeral(true),
    )
}

/// Replaces the `/tell` reply after Revoke, removing the token's commands and the button.
pub fn revoked(revoked: bool) -> CreateInteractionResponse {
    let content = if revoked {
        text::REVOKED
    } else {
        text::ALREADY_REVOKED
    };
    CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
            .content(content)
            .components(vec![]),
    )
}

pub fn dms_closed() -> CreateInteractionResponseFollowup {
    CreateInteractionResponseFollowup::new()
        .content(text::DMS_CLOSED)
        .ephemeral(true)
}
