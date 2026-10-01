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
        "Send yourself a message:\n\
         ```sh\n\
         curl -fsS {url} \\\n -H 'Content-Type: application/json' -d '{{\"token\": \"{token_value}\", \"message\": \"Task finished\"}}'\n\
         ```\n\
	"
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
