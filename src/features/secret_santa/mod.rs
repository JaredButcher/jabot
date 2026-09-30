//! Secret Santa: hosts create an event, add participants, and start it to draw assignments.

mod custom_id;
mod model;
mod repo;
mod text;

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use rand::seq::SliceRandom;
use serenity::all::{
    ButtonStyle, CommandOptionType, CreateActionRow, CreateButton, CreateCommand,
    CreateCommandOption, CreateInputText, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateModal, CreateSelectMenu, CreateSelectMenuKind,
    InputTextStyle, InteractionContext, UserId,
};

use crate::framework::{
    CommandRequest, ComponentKind, ComponentRequest, Feature, FeatureError, InteractionCtx,
    ModalRequest,
};
use custom_id::SsId;
use model::{EventId, EventStatus};
pub use repo::{SecretSantaRepo, SqliteSecretSantaRepo};
use text::Strings;

const SS_HOST_EVENT_LIMIT: i64 = 32;

fn message(content: impl Into<String>) -> CreateInteractionResponse {
    CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(content))
}

fn ephemeral(content: impl Into<String>) -> CreateInteractionResponse {
    CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(content)
            .ephemeral(true),
    )
}

pub struct SecretSanta {
    repo: Arc<dyn SecretSantaRepo>,
}

impl SecretSanta {
    pub fn new(repo: Arc<dyn SecretSantaRepo>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl Feature for SecretSanta {
    fn name(&self) -> &'static str {
        "Secret Santa"
    }

    fn namespace(&self) -> &'static str {
        custom_id::NAMESPACE
    }

    fn commands(&self) -> Vec<CreateCommand> {
        vec![
            CreateCommand::new(Strings::CMD_SS_NAME)
                .description(Strings::CMD_SS_DESC)
                .contexts(vec![
                    InteractionContext::Guild,
                    InteractionContext::BotDm,
                    InteractionContext::PrivateChannel,
                ])
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        Strings::CMD_SS_CREATE_NAME,
                        Strings::CMD_SS_CREATE_DESC,
                    )
                    .add_sub_option(
                        CreateCommandOption::new(
                            CommandOptionType::Integer,
                            Strings::OPT_SS_EVT_ID_NAME,
                            Strings::OPT_SS_EVT_ID_DESC,
                        )
                        .required(false),
                    ),
                )
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        Strings::CMD_SS_INFO_NAME,
                        Strings::CMD_SS_INFO_DESC,
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::Integer,
                        Strings::OPT_SS_EVT_ID_NAME,
                        Strings::OPT_SS_EVT_ID_DESC,
                    )),
                )
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    Strings::CMD_SS_LIST_NAME,
                    Strings::CMD_SS_LIST_DESC,
                )),
        ]
    }

    async fn on_command(
        &self,
        ctx: &InteractionCtx,
        req: CommandRequest,
    ) -> Result<(), FeatureError> {
        let response = match req.subcommand.as_deref() {
            Some(Strings::CMD_SS_CREATE_NAME) => self.create_command(&req).await?,
            Some(Strings::CMD_SS_INFO_NAME) => self.info_command(&req).await?,
            Some(Strings::CMD_SS_LIST_NAME) => self.list_command(ctx, &req).await?,
            Some(other) => {
                println!("Unreconnized command {}", other);
                ephemeral(format!("Unreconnized Command {}", other))
            }
            None => {
                println!("No SS command name given");
                ephemeral("No sub command given")
            }
        };
        ctx.responder.respond(response).await?;
        Ok(())
    }

    async fn on_modal(&self, ctx: &InteractionCtx, req: ModalRequest) -> Result<(), FeatureError> {
        match req.custom_id.parse::<SsId>()? {
            SsId::CreateModal => self.create_modal(ctx, &req).await,
            SsId::EditModal(id) => self.edit_modal(ctx, &req, id).await,
            other => Err(FeatureError::internal(format!("{other:?} is not a modal"))),
        }
    }

    async fn on_component(
        &self,
        ctx: &InteractionCtx,
        req: ComponentRequest,
    ) -> Result<(), FeatureError> {
        match req.custom_id.parse::<SsId>()? {
            SsId::UserSelect(id) => self.participants_select(ctx, &req, id).await,
            SsId::Start(id) => self.start_button(ctx, &req, id).await,
            SsId::End(id) => self.end_button(ctx, &req, id).await,
            SsId::Cancel(id) => self.cancel_button(ctx, &req, id).await,
            other => Err(FeatureError::internal(format!(
                "{other:?} is not a component"
            ))),
        }
    }
}

impl SecretSanta {
    async fn create_command(
        &self,
        req: &CommandRequest,
    ) -> Result<CreateInteractionResponse, FeatureError> {
        let Some(evt_id) = req.options.i64(Strings::OPT_SS_EVT_ID_NAME) else {
            // Send modal to create event
            return Ok(CreateInteractionResponse::Modal(
                CreateModal::new(
                    SsId::CreateModal.to_string(),
                    Strings::MODAL_SS_CREATE_TITLE,
                )
                .components(vec![
                    CreateActionRow::InputText(
                        CreateInputText::new(
                            InputTextStyle::Short,
                            Strings::MODAL_SS_INFO_NAME_LABEL,
                            Strings::MODAL_SS_INFO_NAME_ID,
                        )
                        .required(true),
                    ),
                    CreateActionRow::InputText(
                        CreateInputText::new(
                            InputTextStyle::Short,
                            Strings::MODAL_SS_INFO_DESC_LABEL,
                            Strings::MODAL_SS_INFO_DESC_ID,
                        )
                        .required(false),
                    ),
                ]),
            ));
        };
        let evt_id = EventId(evt_id);

        let Some(event) = self.repo.get_event(evt_id).await? else {
            return Ok(message("Event doesn't exist"));
        };
        if event.host != req.user {
            return Ok(message("Cannot modify event, not event's host"));
        }

        // Send modal to modify event
        Ok(CreateInteractionResponse::Modal(
            CreateModal::new(
                SsId::EditModal(evt_id).to_string(),
                Strings::MODAL_SS_CREATE_EDIT_TITLE,
            )
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(
                        InputTextStyle::Short,
                        Strings::MODAL_SS_INFO_NAME_LABEL,
                        Strings::MODAL_SS_INFO_NAME_ID,
                    )
                    .value(event.name)
                    .required(true),
                ),
                CreateActionRow::InputText(
                    CreateInputText::new(
                        InputTextStyle::Short,
                        Strings::MODAL_SS_INFO_DESC_LABEL,
                        Strings::MODAL_SS_INFO_DESC_ID,
                    )
                    .value(event.description.unwrap_or_default())
                    .required(false),
                ),
            ]),
        ))
    }

    async fn info_command(
        &self,
        req: &CommandRequest,
    ) -> Result<CreateInteractionResponse, FeatureError> {
        let Some(evt_id) = req.options.i64(Strings::OPT_SS_EVT_ID_NAME) else {
            return Ok(ephemeral("Please provide an event ID."));
        };
        let evt_id = EventId(evt_id);

        let event = self.repo.get_event(evt_id).await?;
        let participants = self.repo.participants(evt_id).await?;
        let (Some(event), Some(me)) = (event, participants.iter().find(|p| p.user == req.user))
        else {
            return Ok(ephemeral(
                "Event not found or you are not a participant in this event.",
            ));
        };
        let is_host = event.host == req.user;

        let mut components: Vec<CreateActionRow> = vec![];
        if is_host {
            // Host view components
            // Add user selection dropdown
            if event.status == EventStatus::PreRun {
                let user_ids: Vec<UserId> = participants.iter().map(|p| p.user).collect();
                components.push(CreateActionRow::SelectMenu(
                    CreateSelectMenu::new(
                        SsId::UserSelect(evt_id).to_string(),
                        CreateSelectMenuKind::User {
                            default_users: Some(user_ids),
                        },
                    )
                    .placeholder("Add users to event...")
                    .max_values(25),
                ));
            }

            let mut buttons = vec![];
            let cancel_btn = CreateButton::new(SsId::Cancel(evt_id).to_string())
                .label("Cancel Event")
                .style(ButtonStyle::Danger);

            match event.status {
                EventStatus::PreRun => {
                    buttons.push(
                        CreateButton::new(SsId::Start(evt_id).to_string())
                            .label("Start Event")
                            .style(ButtonStyle::Success),
                    );
                    buttons.push(cancel_btn);
                }
                EventStatus::Running => {
                    buttons.push(
                        CreateButton::new(SsId::End(evt_id).to_string())
                            .label("End Event")
                            .style(ButtonStyle::Success),
                    );
                    buttons.push(cancel_btn);
                }
                EventStatus::Finished => {}
            }
            if !buttons.is_empty() {
                components.push(CreateActionRow::Buttons(buttons));
            }
        }

        let mut content = format!(
            "**Event Information**\n**Name:** {}\n**Description:** {}\n**Status:** {}\n**Host:** <@{}>",
            event.name,
            event.description.unwrap_or("No description".to_string()),
            event.status.label(),
            event.host
        );
        if !is_host || event.status != EventStatus::PreRun {
            let user_text_list = participants
                .iter()
                .map(|p| format!("<@{}>", p.user))
                .collect::<Vec<String>>()
                .join(", ");
            content += format!("\n**Participants:** {}", user_text_list).as_str();
        }
        if let Some(assignee) = me.assignee {
            content += format!("\n**Get a gift for:** <@{}>", assignee).as_str();
        }

        let mut response = CreateInteractionResponseMessage::new()
            .content(content)
            .ephemeral(true);
        if !components.is_empty() {
            response = response.components(components);
        }
        Ok(CreateInteractionResponse::Message(response))
    }

    async fn list_command(
        &self,
        ctx: &InteractionCtx,
        req: &CommandRequest,
    ) -> Result<CreateInteractionResponse, FeatureError> {
        let events = self.repo.events_for_user(req.user).await?;

        let mut result_str: String = "---Events---\r\n".to_string();
        for event in events {
            // Get host user from Discord API/cache
            let host_name = match ctx.discord.user_name(event.host).await {
                Ok(name) => name,
                Err(_) => format!("Unknown User ({})", event.host),
            };

            result_str += format!(
                "**ID:** {}\n**Name:** {}\n**Description:** {}\n **Status:** {}\n**Host:** {}\n\n",
                event.id,
                event.name,
                event.description.unwrap_or_default(),
                event.status.label(),
                host_name
            )
            .as_str();
        }

        Ok(ephemeral(result_str))
    }

    async fn create_modal(
        &self,
        ctx: &InteractionCtx,
        req: &ModalRequest,
    ) -> Result<(), FeatureError> {
        // Create Event
        let name = req
            .field(Strings::MODAL_SS_INFO_NAME_ID)
            .unwrap_or(Strings::DEFAULT_SS_NAME);
        let description = req
            .field(Strings::MODAL_SS_INFO_DESC_ID)
            .unwrap_or(Strings::DEFAULT_SS_DESCRIPTION);

        // Check if host has reached event limit
        if self.repo.count_active_hosted(req.user).await? >= SS_HOST_EVENT_LIMIT {
            ctx.responder
                .respond(message(format!(
                    "Cannot create event: You have reached the limit of {} active events",
                    SS_HOST_EVENT_LIMIT
                )))
                .await?;
            return Ok(());
        }

        let event_id = self.repo.create_event(req.user, name, description).await?;

        ctx.responder
            .respond(ephemeral(format!(
                "Event created\n**Id:** {}\n**Name:** {}",
                event_id, name
            )))
            .await?;
        Ok(())
    }

    async fn edit_modal(
        &self,
        ctx: &InteractionCtx,
        req: &ModalRequest,
        evt_id: EventId,
    ) -> Result<(), FeatureError> {
        // Fetch existing event if it exists and the command's user is the host
        let Some(existing_event) = self.repo.get_event(evt_id).await? else {
            ctx.responder.respond(message("Event not found")).await?;
            return Ok(());
        };
        if existing_event.host != req.user {
            ctx.responder
                .respond(message(
                    "Cannot modify event: You are not the host of this event",
                ))
                .await?;
            return Ok(());
        }

        // Modify existing event
        let update_name = req
            .field(Strings::MODAL_SS_INFO_NAME_ID)
            .unwrap_or(&existing_event.name);
        let update_description = req
            .field(Strings::MODAL_SS_INFO_DESC_ID)
            .map(str::to_string)
            .or(existing_event.description.clone());
        self.repo
            .update_event(evt_id, update_name, update_description)
            .await?;

        println!("Event '{}' updated", update_name);
        ctx.responder
            .respond(message(format!(
                "Event '{}' updated successfully",
                update_name
            )))
            .await?;
        Ok(())
    }

    async fn participants_select(
        &self,
        ctx: &InteractionCtx,
        req: &ComponentRequest,
        evt_id: EventId,
    ) -> Result<(), FeatureError> {
        let ComponentKind::UserSelect(values) = &req.kind else {
            ctx.responder
                .respond(ephemeral("User does not exist"))
                .await?;
            return Ok(());
        };

        // Check if event exists and user is the event host and get event info
        let event = self
            .repo
            .get_event(evt_id)
            .await?
            .ok_or_else(|| FeatureError::internal(format!("event {evt_id} not found")))?;

        if event.status != EventStatus::PreRun {
            ctx.responder
                .respond(ephemeral("Cannot modify users of started event"))
                .await?;
            return Ok(());
        }

        // Get existing participant IDs for comparison
        let existing_participants: HashSet<UserId> = self
            .repo
            .participants(evt_id)
            .await?
            .iter()
            .map(|p| p.user)
            .collect();
        let selected: HashSet<UserId> = values.iter().copied().collect();

        // Find users to add (selected but not already participating) and remove (no longer selected)
        let users_to_add: Vec<UserId> = values
            .iter()
            .copied()
            .filter(|user| !existing_participants.contains(user))
            .collect();
        let users_to_remove: Vec<UserId> = existing_participants
            .iter()
            .copied()
            .filter(|user| !selected.contains(user))
            .collect();

        println!(
            "Modify users add: {:?} remove: {:?}",
            users_to_add, users_to_remove
        );
        self.repo
            .set_participants(evt_id, &users_to_add, &users_to_remove)
            .await?;

        ctx.responder
            .respond(CreateInteractionResponse::Acknowledge)
            .await?;

        // Notify users afterwards to avoid ack timeout
        for &user in &users_to_add {
            ctx.discord
                .send_dm(
                    user,
                    format!(
                        "You have been invited to a Secret Santa Event: {}\r\n{}",
                        event.name,
                        event.description.as_deref().unwrap_or("")
                    ),
                )
                .await?;
        }
        Ok(())
    }

    async fn start_button(
        &self,
        ctx: &InteractionCtx,
        req: &ComponentRequest,
        evt_id: EventId,
    ) -> Result<(), FeatureError> {
        let Some(event) = self.repo.get_event(evt_id).await? else {
            ctx.responder.respond(ephemeral("Event not found")).await?;
            return Ok(());
        };
        // Confirm requesting user is host
        if event.host != req.user {
            ctx.responder
                .respond(ephemeral("You are not the host of this event"))
                .await?;
            return Ok(());
        }

        // Confrim that event's status is preparing
        if event.status != EventStatus::PreRun {
            ctx.responder
                .respond(ephemeral("Event is not in preparing state"))
                .await?;
            return Ok(());
        }

        let participants = self.repo.participants(evt_id).await?;
        if participants.len() <= 1 {
            ctx.responder
                .respond(ephemeral("Secret Santa requires more than one participant"))
                .await?;
            return Ok(());
        }

        // Shuffle participants and assign each to give a gift to the next person in the list
        let mut shuffled: Vec<UserId> = participants.iter().map(|p| p.user).collect();
        shuffled.shuffle(&mut rand::rng());
        let assignments: Vec<(UserId, UserId)> = (0..shuffled.len())
            .map(|i| (shuffled[i], shuffled[(i + 1) % shuffled.len()]))
            .collect();

        // Save status and assignments together, before anyone is notified
        if !self.repo.start_event(evt_id, &assignments).await? {
            ctx.responder
                .respond(ephemeral("Event was already started"))
                .await?;
            return Ok(());
        }

        ctx.responder
            .respond(ephemeral("Event started successfully!"))
            .await?;

        // Send notifications to participants
        for (santa, recipient) in assignments {
            ctx.discord
                .send_dm(
                    santa,
                    format!(
                        "The Secret Santa Event **{}** has started!\n{}\nYou are expected to give a gift to **<@{}>**\nUse the command `\\ss info {}` to check the event's status.",
                        event.name,
                        event.description.as_deref().unwrap_or(""),
                        recipient,
                        event.id
                    ),
                )
                .await?;
        }
        Ok(())
    }

    async fn end_button(
        &self,
        ctx: &InteractionCtx,
        req: &ComponentRequest,
        evt_id: EventId,
    ) -> Result<(), FeatureError> {
        let Some(event) = self.repo.get_event(evt_id).await? else {
            ctx.responder.respond(ephemeral("Event not found")).await?;
            return Ok(());
        };
        // Confirm requesting user is host
        if event.host != req.user {
            ctx.responder
                .respond(ephemeral("You are not the host of this event"))
                .await?;
            return Ok(());
        }

        // Change event status to finished, only if it is running
        if event.status != EventStatus::Running
            || !self
                .repo
                .transition(evt_id, &[EventStatus::Running], EventStatus::Finished)
                .await?
        {
            ctx.responder
                .respond(ephemeral("Event is not currently running"))
                .await?;
            return Ok(());
        }

        let participants = self.repo.participants(evt_id).await?;

        ctx.responder
            .respond(ephemeral("Event ended successfully!"))
            .await?;

        // Notify each participant that event has finished
        for participant in participants {
            if let Err(why) = ctx
                .discord
                .send_dm(
                    participant.user,
                    format!(
                        "The Secret Santa Event '{}' has concluded successfully!\n{}",
                        event.name,
                        event.description.as_deref().unwrap_or("")
                    ),
                )
                .await
            {
                println!("End notification error {}", why);
            }
        }
        Ok(())
    }

    async fn cancel_button(
        &self,
        ctx: &InteractionCtx,
        req: &ComponentRequest,
        evt_id: EventId,
    ) -> Result<(), FeatureError> {
        let Some(event) = self.repo.get_event(evt_id).await? else {
            ctx.responder.respond(ephemeral("Event not found")).await?;
            return Ok(());
        };
        // Confirm requesting user is host
        if event.host != req.user {
            ctx.responder
                .respond(ephemeral("You are not the host of this event"))
                .await?;
            return Ok(());
        }

        // Confrim that event's status is preparing or running
        if event.status == EventStatus::Finished {
            ctx.responder
                .respond(ephemeral("Event cannot be canceled (already finished)"))
                .await?;
            return Ok(());
        }

        // Change event status to finished, only from the status we just saw, so we know
        // whether assignments went out and participants need telling
        if !self
            .repo
            .transition(evt_id, &[event.status], EventStatus::Finished)
            .await?
        {
            ctx.responder
                .respond(ephemeral(
                    "The event changed while you were canceling it. Please try again.",
                ))
                .await?;
            return Ok(());
        }

        ctx.responder
            .respond(ephemeral("Event canceled successfully!"))
            .await?;

        // If it was running, notify each participant that event is now canceled
        if event.status == EventStatus::Running {
            for participant in self.repo.participants(evt_id).await? {
                ctx.discord
                    .send_dm(
                        participant.user,
                        format!(
                            "The Secret Santa Event '{}' has been canceled by the host.\n{}",
                            event.name,
                            event.description.as_deref().unwrap_or("")
                        ),
                    )
                    .await?;
            }
        }
        Ok(())
    }
}
