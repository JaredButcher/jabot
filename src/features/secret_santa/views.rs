//! Everything Secret Santa shows: responses, forms and DM text. Pure functions of the model.

use serenity::all::{
    ButtonStyle, CreateActionRow, CreateButton, CreateInputText, CreateInteractionResponse,
    CreateInteractionResponseFollowup, CreateInteractionResponseMessage, CreateModal,
    CreateSelectMenu, CreateSelectMenuKind, InputTextStyle, UserId,
};

use super::custom_id::SsId;
use super::model::{Event, EventId, EventStatus, Participant};
use super::rules::MAX_PARTICIPANTS;
use super::text;

pub fn ephemeral(content: impl Into<String>) -> CreateInteractionResponse {
    CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(content)
            .ephemeral(true),
    )
}

pub fn create_form() -> CreateInteractionResponse {
    event_form(SsId::CreateModal, text::FORM_CREATE_TITLE, None)
}

pub fn edit_form(event: &Event) -> CreateInteractionResponse {
    event_form(
        SsId::EditModal(event.id),
        text::FORM_EDIT_TITLE,
        Some(event),
    )
}

fn event_form(id: SsId, title: &str, current: Option<&Event>) -> CreateInteractionResponse {
    let mut name = CreateInputText::new(
        InputTextStyle::Short,
        text::FIELD_NAME_LABEL,
        text::FIELD_NAME,
    )
    .required(true);
    let mut description = CreateInputText::new(
        InputTextStyle::Short,
        text::FIELD_DESCRIPTION_LABEL,
        text::FIELD_DESCRIPTION,
    )
    .required(false);
    if let Some(event) = current {
        name = name.value(&event.name);
        description = description.value(event.description.clone().unwrap_or_default());
    }
    CreateInteractionResponse::Modal(CreateModal::new(id.to_string(), title).components(vec![
        CreateActionRow::InputText(name),
        CreateActionRow::InputText(description),
    ]))
}

pub fn created(id: EventId, name: &str) -> CreateInteractionResponse {
    ephemeral(format!("Event created\n**Id:** {id}\n**Name:** {name}"))
}

pub fn updated(name: &str) -> CreateInteractionResponse {
    ephemeral(format!("Event '{name}' updated successfully"))
}

pub fn event_limit(limit: i64) -> CreateInteractionResponse {
    ephemeral(format!(
        "Cannot create event: You have reached the limit of {limit} active events"
    ))
}

/// The `/ss info` view. The host also gets controls: the participant picker while preparing,
/// and start/end/cancel buttons.
pub fn event_info(
    event: &Event,
    participants: &[Participant],
    viewer: UserId,
) -> CreateInteractionResponse {
    let is_host = event.host == viewer;

    let mut content = format!(
        "**Event Information**\n**Name:** {}\n**Description:** {}\n**Status:** {}\n**Host:** <@{}>",
        event.name,
        event.description.as_deref().unwrap_or("No description"),
        event.status.label(),
        event.host
    );
    // While preparing, the host sees participants in the picker instead.
    if !is_host || event.status != EventStatus::PreRun {
        let mentions: Vec<String> = participants
            .iter()
            .map(|p| format!("<@{}>", p.user))
            .collect();
        content += &format!("\n**Participants:** {}", mentions.join(", "));
    }
    let assignee = participants
        .iter()
        .find(|p| p.user == viewer)
        .and_then(|p| p.assignee);
    if let Some(assignee) = assignee {
        content += &format!("\n**Get a gift for:** <@{assignee}>");
    }

    let mut message = CreateInteractionResponseMessage::new()
        .content(content)
        .ephemeral(true);
    if is_host {
        let components = host_controls(event, participants);
        if !components.is_empty() {
            message = message.components(components);
        }
    }
    CreateInteractionResponse::Message(message)
}

fn host_controls(event: &Event, participants: &[Participant]) -> Vec<CreateActionRow> {
    let mut rows = vec![];
    if event.status == EventStatus::PreRun {
        let users: Vec<UserId> = participants.iter().map(|p| p.user).collect();
        rows.push(CreateActionRow::SelectMenu(
            CreateSelectMenu::new(
                SsId::UserSelect(event.id).to_string(),
                CreateSelectMenuKind::User {
                    default_users: Some(users),
                },
            )
            .placeholder("Add users to event...")
            .max_values(MAX_PARTICIPANTS as u8),
        ));
    }

    let cancel = CreateButton::new(SsId::Cancel(event.id).to_string())
        .label("Cancel Event")
        .style(ButtonStyle::Danger);
    let buttons = match event.status {
        EventStatus::PreRun => vec![
            CreateButton::new(SsId::Start(event.id).to_string())
                .label("Start Event")
                .style(ButtonStyle::Success),
            cancel,
        ],
        EventStatus::Running => vec![
            CreateButton::new(SsId::End(event.id).to_string())
                .label("End Event")
                .style(ButtonStyle::Success),
            cancel,
        ],
        EventStatus::Finished => vec![],
    };
    if !buttons.is_empty() {
        rows.push(CreateActionRow::Buttons(buttons));
    }
    rows
}

/// The `/ss list` view. Hosts are mentions, which Discord renders as names without an API call.
pub fn event_list(events: &[Event]) -> CreateInteractionResponse {
    if events.is_empty() {
        return ephemeral(text::NO_EVENTS);
    }
    let mut content = String::from("---Events---\n");
    for event in events {
        content += &format!(
            "**ID:** {}\n**Name:** {}\n**Description:** {}\n**Status:** {}\n**Host:** <@{}>\n\n",
            event.id,
            event.name,
            event.description.as_deref().unwrap_or_default(),
            event.status.label(),
            event.host
        );
    }
    ephemeral(content)
}

/// Tells the host which participants couldn't be DMed.
pub fn dm_failures(users: &[UserId]) -> CreateInteractionResponseFollowup {
    let mentions: Vec<String> = users.iter().map(|u| format!("<@{u}>")).collect();
    CreateInteractionResponseFollowup::new()
        .content(format!(
            "Couldn't DM: {} (they may have DMs disabled)",
            mentions.join(", ")
        ))
        .ephemeral(true)
}

pub fn invite_dm(event: &Event) -> String {
    format!(
        "You have been invited to a Secret Santa Event: {}\n{}",
        event.name,
        event.description.as_deref().unwrap_or_default()
    )
}

pub fn started_dm(event: &Event, recipient: UserId) -> String {
    format!(
        "The Secret Santa Event **{}** has started!\n{}\nYou are expected to give a gift to **<@{}>**\nUse the command `/ss info id:{}` to check the event's status.",
        event.name,
        event.description.as_deref().unwrap_or_default(),
        recipient,
        event.id
    )
}

pub fn ended_dm(event: &Event) -> String {
    format!(
        "The Secret Santa Event '{}' has concluded successfully!\n{}",
        event.name,
        event.description.as_deref().unwrap_or_default()
    )
}

pub fn canceled_dm(event: &Event) -> String {
    format!(
        "The Secret Santa Event '{}' has been canceled by the host.\n{}",
        event.name,
        event.description.as_deref().unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::testing::{content, is_ephemeral, json};

    fn event(status: EventStatus) -> Event {
        Event {
            id: EventId(5),
            name: "Party".into(),
            description: Some("Bring snacks".into()),
            host: UserId::new(1),
            status,
        }
    }

    fn participants() -> Vec<Participant> {
        vec![
            Participant {
                user: UserId::new(1),
                assignee: Some(UserId::new(2)),
            },
            Participant {
                user: UserId::new(2),
                assignee: Some(UserId::new(1)),
            },
        ]
    }

    /// custom_ids of every component in a response.
    fn component_ids(response: &CreateInteractionResponse) -> Vec<String> {
        let json = json(response);
        let rows = json["data"]["components"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        rows.iter()
            .flat_map(|row| row["components"].as_array().cloned().unwrap_or_default())
            .map(|c| c["custom_id"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    #[test]
    fn host_sees_picker_start_and_cancel_while_preparing() {
        let view = event_info(&event(EventStatus::PreRun), &participants(), UserId::new(1));

        assert!(is_ephemeral(&view));
        assert_eq!(
            component_ids(&view),
            ["ss:participants:5", "ss:start:5", "ss:cancel:5"]
        );
    }

    #[test]
    fn host_sees_end_and_cancel_while_running() {
        let view = event_info(
            &event(EventStatus::Running),
            &participants(),
            UserId::new(1),
        );

        assert_eq!(component_ids(&view), ["ss:end:5", "ss:cancel:5"]);
    }

    #[test]
    fn host_sees_no_controls_when_finished() {
        let view = event_info(
            &event(EventStatus::Finished),
            &participants(),
            UserId::new(1),
        );

        assert!(component_ids(&view).is_empty());
    }

    #[test]
    fn participant_sees_no_controls_but_sees_assignment() {
        let view = event_info(
            &event(EventStatus::Running),
            &participants(),
            UserId::new(2),
        );

        assert!(component_ids(&view).is_empty());
        assert!(content(&view).contains("**Participants:** <@1>, <@2>"));
        assert!(content(&view).contains("**Get a gift for:** <@1>"));
    }

    #[test]
    fn edit_form_is_prefilled() {
        let form = json(&edit_form(&event(EventStatus::PreRun)));

        assert_eq!(form["data"]["custom_id"], "ss:edit:5");
        let inputs = &form["data"]["components"];
        assert_eq!(inputs[0]["components"][0]["value"], "Party");
        assert_eq!(inputs[1]["components"][0]["value"], "Bring snacks");
    }

    #[test]
    fn empty_list_says_so() {
        assert_eq!(content(&event_list(&[])), text::NO_EVENTS);
    }
}
