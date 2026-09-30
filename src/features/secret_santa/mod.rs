//! Secret Santa: hosts create an event, add participants, and start it to draw assignments.

mod custom_id;
mod model;
mod text;

use std::collections::HashSet;

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
use model::EventId;
use text::Strings;

const SS_HOST_EVENT_LIMIT: i32 = 32;

#[derive(PartialEq, Eq)]
enum SSState {
    PreRun,
    Running,
    Finished,
}

impl From<SSState> for i32 {
    fn from(state: SSState) -> Self {
        match state {
            SSState::PreRun => 0,
            SSState::Running => 1,
            SSState::Finished => 2,
        }
    }
}
impl From<i32> for SSState {
    fn from(value: i32) -> Self {
        match value {
            0 => SSState::PreRun,
            1 => SSState::Running,
            2 => SSState::Finished,
            _ => SSState::PreRun, // Default to PreRun for invalid values
        }
    }
}

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
    database: sqlx::SqlitePool,
}

impl SecretSanta {
    pub fn new(database: sqlx::SqlitePool) -> Self {
        Self { database }
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
            SsId::EditModal(id) => self.edit_modal(ctx, &req, id.0).await,
            other => Err(FeatureError::internal(format!("{other:?} is not a modal"))),
        }
    }

    async fn on_component(
        &self,
        ctx: &InteractionCtx,
        req: ComponentRequest,
    ) -> Result<(), FeatureError> {
        match req.custom_id.parse::<SsId>()? {
            SsId::UserSelect(id) => self.participants_select(ctx, &req, id.0).await,
            SsId::Start(id) => self.start_button(ctx, &req, id.0).await,
            SsId::End(id) => self.end_button(ctx, &req, id.0).await,
            SsId::Cancel(id) => self.cancel_button(ctx, &req, id.0).await,
            other => Err(FeatureError::internal(format!(
                "{other:?} is not a component"
            ))),
        }
    }
}

impl SecretSanta {
    async fn add_user_to_event(&self, user: i64, event: i64) -> Result<(), sqlx::Error> {
        // If event_participants entry does not already exist for this user and event pair, insert one.
        sqlx::query!(
            "INSERT OR IGNORE INTO event_participants (user_id, event_id, joined) VALUES (?, ?, TRUE)",
            user,
            event
        )
        .execute(&self.database)
        .await?;

        Ok(())
    }

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

        let Ok(rec) = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id)
            .fetch_one(&self.database)
            .await
        else {
            return Ok(message("Event doesn't exist"));
        };
        if rec.host_id != i64::from(req.user) {
            return Ok(message("Cannot modify event, not event's host"));
        }

        // Send modal to modify event
        Ok(CreateInteractionResponse::Modal(
            CreateModal::new(
                SsId::EditModal(EventId(evt_id)).to_string(),
                Strings::MODAL_SS_CREATE_EDIT_TITLE,
            )
            .components(vec![
                CreateActionRow::InputText(
                    CreateInputText::new(
                        InputTextStyle::Short,
                        Strings::MODAL_SS_INFO_NAME_LABEL,
                        Strings::MODAL_SS_INFO_NAME_ID,
                    )
                    .value(rec.name)
                    .required(true),
                ),
                CreateActionRow::InputText(
                    CreateInputText::new(
                        InputTextStyle::Short,
                        Strings::MODAL_SS_INFO_DESC_LABEL,
                        Strings::MODAL_SS_INFO_DESC_ID,
                    )
                    .value(rec.description.unwrap_or_default())
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
        let user_id = i64::from(req.user);

        // Check if user is a participant and get event info
        let event_query = sqlx::query!(
            "SELECT e.* FROM events e
             JOIN event_participants ep ON e.id = ep.event_id
             WHERE e.id = ? AND ep.user_id = ?",
            evt_id,
            user_id
        )
        .fetch_optional(&self.database)
        .await?;

        // Fetch all event users
        let event_users_query = sqlx::query!(
            "SELECT ep.user_id, ep.assignee_id FROM events e
             JOIN event_participants ep ON e.id = ep.event_id
             WHERE e.id = ?",
            evt_id
        )
        .fetch_all(&self.database)
        .await?;

        let Some(event) = event_query else {
            return Ok(ephemeral(
                "Event not found or you are not a participant in this event.",
            ));
        };
        let Some(user) = event_users_query.iter().find(|u| u.user_id == user_id) else {
            return Ok(ephemeral("You are not a participant in this event"));
        };
        let is_host = event.host_id == user_id;
        let event_status = SSState::from(event.status as i32);

        let mut components: Vec<CreateActionRow> = vec![];
        if is_host {
            // Host view components
            // Add user selection dropdown
            let user_ids: Vec<UserId> = event_users_query
                .iter()
                .map(|u| UserId::new(u.user_id as u64))
                .collect();

            if event_status == SSState::PreRun {
                components.push(CreateActionRow::SelectMenu(
                    CreateSelectMenu::new(
                        SsId::UserSelect(EventId(evt_id)).to_string(),
                        CreateSelectMenuKind::User {
                            default_users: Some(user_ids),
                        },
                    )
                    .placeholder("Add users to event...")
                    .max_values(25),
                ));
            }

            let mut buttons = vec![];
            let cancel_btn = CreateButton::new(SsId::Cancel(EventId(evt_id)).to_string())
                .label("Cancel Event")
                .style(ButtonStyle::Danger);

            match event_status {
                SSState::PreRun => {
                    buttons.push(
                        CreateButton::new(SsId::Start(EventId(evt_id)).to_string())
                            .label("Start Event")
                            .style(ButtonStyle::Success),
                    );
                    buttons.push(cancel_btn);
                }
                SSState::Running => {
                    buttons.push(
                        CreateButton::new(SsId::End(EventId(evt_id)).to_string())
                            .label("End Event")
                            .style(ButtonStyle::Success),
                    );
                    buttons.push(cancel_btn);
                }
                SSState::Finished => {}
            }
            if !buttons.is_empty() {
                components.push(CreateActionRow::Buttons(buttons));
            }
        }

        let status_text = match event_status {
            SSState::PreRun => "Preparing",
            SSState::Running => "Running",
            SSState::Finished => "Finished",
        };

        let mut content = format!(
            "**Event Information**\n**Name:** {}\n**Description:** {}\n**Status:** {}\n**Host:** <@{}>",
            event.name,
            event.description.unwrap_or("No description".to_string()),
            status_text,
            event.host_id
        );
        if !is_host || event_status != SSState::PreRun {
            let user_text_list = event_users_query
                .iter()
                .map(|u| format!("<@{}>", u.user_id))
                .collect::<Vec<String>>()
                .join(", ");
            content += format!("\n**Participants:** {}", user_text_list).as_str();
        }
        if let Some(assignee_id) = user.assignee_id {
            content += format!("\n**Get a gift for:** <@{}>", assignee_id).as_str();
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
        let host_id = i64::from(req.user);
        let evts = sqlx::query!(
            "SELECT e.*, ep.user_id FROM events e JOIN event_participants ep ON e.id = ep.event_id WHERE ep.user_id = ?",
            host_id
        )
        .fetch_all(&self.database)
        .await?;

        let mut result_str: String = "---Events---\r\n".to_string();
        for evt in evts {
            // Get host user from Discord API/cache
            let host_name = match ctx.discord.user_name(UserId::new(evt.host_id as u64)).await {
                Ok(name) => name,
                Err(_) => format!("Unknown User ({})", evt.host_id),
            };

            let status_text = match SSState::from(evt.status as i32) {
                SSState::PreRun => "Preparing",
                SSState::Running => "Running",
                SSState::Finished => "Finished",
            };

            result_str += format!(
                "**ID:** {}\n**Name:** {}\n**Description:** {}\n **Status:** {}\n**Host:** {}\n\n",
                evt.id,
                evt.name,
                evt.description.unwrap_or_default(),
                status_text,
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
            .unwrap_or(Strings::DEFAULT_SS_NAME)
            .to_string();
        let description = req
            .field(Strings::MODAL_SS_INFO_DESC_ID)
            .unwrap_or(Strings::DEFAULT_SS_DESCRIPTION)
            .to_string();

        // Insert host if not present
        let host_id = i64::from(req.user);
        sqlx::query!(
            "INSERT INTO event_users (id, global_wish) SELECT ?, ? WHERE NOT EXISTS ( SELECT 1 FROM event_users WHERE id = ? )",
            host_id,
            "",
            host_id
        )
        .execute(&self.database)
        .await?;

        // Check if host has reached event limit
        let pre_run_status = i32::from(SSState::PreRun);
        let running_status = i32::from(SSState::Running);
        let active_events = sqlx::query!(
            "SELECT COUNT(*) as count FROM events WHERE host_id = ? AND (status = ? OR status = ?)",
            host_id,
            pre_run_status,
            running_status
        )
        .fetch_one(&self.database)
        .await?;

        if active_events.count >= SS_HOST_EVENT_LIMIT as i64 {
            // Send error message that host has reached limit
            ctx.responder
                .respond(message(format!(
                    "Cannot create event: You have reached the limit of {} active events",
                    SS_HOST_EVENT_LIMIT
                )))
                .await?;
            return Ok(());
        }

        // Insert event
        let result = sqlx::query!(
            "INSERT INTO events (name, description, host_id, status) VALUES (?, ?, ?, ?)",
            name,
            description,
            host_id,
            pre_run_status
        )
        .execute(&self.database)
        .await?;
        let event_id = result.last_insert_rowid();

        // Add the host as a participant in their own event
        if let Err(err) = self.add_user_to_event(host_id, event_id).await {
            println!("Failed to add host to event: {}", err);
        }

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
        evt_id: i64,
    ) -> Result<(), FeatureError> {
        // Modify Event
        let name = req
            .field(Strings::MODAL_SS_INFO_NAME_ID)
            .map(str::to_string);
        let description = req
            .field(Strings::MODAL_SS_INFO_DESC_ID)
            .map(str::to_string);

        // Fetch existing event if it exists and the command's user is the host
        let Ok(existing_event) = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id)
            .fetch_one(&self.database)
            .await
        else {
            ctx.responder.respond(message("Event not found")).await?;
            return Ok(());
        };
        if existing_event.host_id != i64::from(req.user) {
            ctx.responder
                .respond(message(
                    "Cannot modify event: You are not the host of this event",
                ))
                .await?;
            return Ok(());
        }

        // Modify existing event
        let update_name = name.unwrap_or(existing_event.name);
        let update_description = description.or(existing_event.description);

        sqlx::query!(
            "UPDATE events SET name = ?, description = ? WHERE id = ?",
            update_name,
            update_description,
            evt_id
        )
        .execute(&self.database)
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
        evt_id: i64,
    ) -> Result<(), FeatureError> {
        let ComponentKind::UserSelect(values) = &req.kind else {
            ctx.responder
                .respond(ephemeral("User does not exist"))
                .await?;
            return Ok(());
        };

        // Check if event exists and user is the event host and get event info
        let event_query = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id)
            .fetch_one(&self.database)
            .await?;

        if event_query.status != i32::from(SSState::PreRun) as i64 {
            ctx.responder
                .respond(ephemeral("Cannot modify users of started event"))
                .await?;
            return Ok(());
        }

        // Fetch all event users
        let event_users_query = sqlx::query!(
            "SELECT ep.user_id FROM events e
                JOIN event_participants ep ON e.id = ep.event_id
                WHERE e.id = ?",
            evt_id
        )
        .fetch_all(&self.database)
        .await?;

        // Get existing participant IDs for comparison
        let existing_participants: HashSet<i64> =
            event_users_query.iter().map(|p| p.user_id).collect();

        // Find users to add (selected but not already participating)
        let users_to_add: Vec<i64> = values
            .iter()
            .map(|user| u64::from(*user) as i64)
            .filter(|user_id| !existing_participants.contains(user_id))
            .collect();
        // Bulk insert all users into event_users and event_participants
        if !users_to_add.is_empty() {
            // Build VALUES clauses for bulk insert
            let user_values: Vec<String> = users_to_add.iter().map(|_| "(?)".to_string()).collect();
            let participant_values: Vec<String> = users_to_add
                .iter()
                .map(|_| "(?, ?, TRUE)".to_string())
                .collect();

            let user_query = format!(
                "INSERT OR IGNORE INTO event_users (id) VALUES {}",
                user_values.join(", ")
            );
            let participant_query = format!(
                "INSERT OR IGNORE INTO event_participants (user_id, event_id, joined) VALUES {}",
                participant_values.join(", ")
            );

            // Execute user insert
            let mut user_query_builder = sqlx::query(&user_query);
            for &user_id in &users_to_add {
                user_query_builder = user_query_builder.bind(user_id);
            }
            user_query_builder.execute(&self.database).await?;

            // Execute participant insert
            let mut participant_query_builder = sqlx::query(&participant_query);
            for &user_id in &users_to_add {
                participant_query_builder = participant_query_builder.bind(user_id).bind(evt_id);
            }
            participant_query_builder.execute(&self.database).await?;
        }

        let updated_participant_set: HashSet<i64> = values
            .iter()
            .map(|user_id| u64::from(*user_id) as i64)
            .collect();
        let users_to_remove: Vec<i64> = existing_participants
            .iter()
            .copied()
            .filter(|uid| !updated_participant_set.contains(uid))
            .collect();

        println!(
            "Modify users add: {:?} remove: {:?}",
            users_to_add, users_to_remove
        );

        // Bulk remove participants no longer selected
        if !users_to_remove.is_empty() {
            let placeholders: Vec<String> =
                users_to_remove.iter().map(|_| "?".to_string()).collect();
            let remove_query = format!(
                "DELETE FROM event_participants WHERE event_id = ? AND user_id IN ({})",
                placeholders.join(", ")
            );

            let mut remove_query_builder = sqlx::query(&remove_query).bind(evt_id);
            for &user_id in &users_to_remove {
                remove_query_builder = remove_query_builder.bind(user_id);
            }
            remove_query_builder.execute(&self.database).await?;
        }

        ctx.responder
            .respond(CreateInteractionResponse::Acknowledge)
            .await?;

        // Notify users afterwards to avoid ack timeout
        for &user_id in &users_to_add {
            ctx.discord
                .send_dm(
                    UserId::new(user_id as u64),
                    format!(
                        "You have been invited to a Secret Santa Event: {}\r\n{}",
                        event_query.name,
                        event_query.description.as_deref().unwrap_or("")
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
        evt_id: i64,
    ) -> Result<(), FeatureError> {
        // Confirm requesting user is host
        let user_id = i64::from(req.user);

        let Ok(event) = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id)
            .fetch_one(&self.database)
            .await
        else {
            ctx.responder.respond(ephemeral("Event not found")).await?;
            return Ok(());
        };
        if event.host_id != user_id {
            ctx.responder
                .respond(ephemeral("You are not the host of this event"))
                .await?;
            return Ok(());
        }

        // Confrim that event's status is preparing
        if SSState::from(event.status as i32) != SSState::PreRun {
            ctx.responder
                .respond(ephemeral("Event is not in preparing state"))
                .await?;
            return Ok(());
        }

        // Notify each participant that event is now running with the event's name and description
        let participants = sqlx::query!(
            "SELECT user_id, event_wish FROM event_participants WHERE event_id = ?",
            evt_id
        )
        .fetch_all(&self.database)
        .await?;

        let participants_cnt = participants.len();
        if participants_cnt <= 1 {
            ctx.responder
                .respond(ephemeral("Secret Santa requires more than one participant"))
                .await?;
            return Ok(());
        }

        // Change event status to running
        let running_status = i32::from(SSState::Running);
        sqlx::query!(
            "UPDATE events SET status = ? WHERE id = ?",
            running_status,
            evt_id
        )
        .execute(&self.database)
        .await?;

        // Shuffle records and assign partipants their secret santas
        let mut participants_shuffled: Vec<(i64, i64)> =
            participants.iter().map(|f| (f.user_id, 0)).collect();
        participants_shuffled.shuffle(&mut rand::rng());

        // Assign each participant to give a gift to the next person in the shuffled list
        for i in 0..participants_cnt {
            participants_shuffled[i].1 = participants_shuffled[(i + 1) % participants_cnt].0;
        }

        ctx.responder
            .respond(ephemeral("Event started successfully!"))
            .await?;

        // Save the secret santas and send notifications to participants
        for participant in participants_shuffled {
            sqlx::query!(
                "UPDATE event_participants SET assignee_id = ? WHERE user_id = ?",
                participant.1,
                participant.0
            )
            .execute(&self.database)
            .await?;

            ctx.discord
                .send_dm(
                    UserId::new(participant.0 as u64),
                    format!(
                        "The Secret Santa Event **{}** has started!\n{}\nYou are expected to give a gift to **<@{}>**\nUse the command `\\ss info {}` to check the event's status.",
                        event.name,
                        event.description.as_deref().unwrap_or(""),
                        participant.1,
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
        evt_id: i64,
    ) -> Result<(), FeatureError> {
        // Confirm requesting user is host
        let user_id = i64::from(req.user);

        let Ok(event) = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id)
            .fetch_one(&self.database)
            .await
        else {
            ctx.responder.respond(ephemeral("Event not found")).await?;
            return Ok(());
        };
        if event.host_id != user_id {
            ctx.responder
                .respond(ephemeral("You are not the host of this event"))
                .await?;
            return Ok(());
        }

        // Confrim that event's status is running
        if SSState::from(event.status as i32) != SSState::Running {
            ctx.responder
                .respond(ephemeral("Event is not currently running"))
                .await?;
            return Ok(());
        }

        // Change event status to finished
        let finished_status = i32::from(SSState::Finished);
        sqlx::query!(
            "UPDATE events SET status = ? WHERE id = ?",
            finished_status,
            evt_id
        )
        .execute(&self.database)
        .await?;

        // Notify each participant that event has finished
        let participants = sqlx::query!(
            "SELECT user_id FROM event_participants WHERE event_id = ?",
            evt_id
        )
        .fetch_all(&self.database)
        .await?;

        ctx.responder
            .respond(ephemeral("Event ended successfully!"))
            .await?;

        // Send notifications to participants
        for participant in participants {
            if let Err(why) = ctx
                .discord
                .send_dm(
                    UserId::new(participant.user_id as u64),
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
        evt_id: i64,
    ) -> Result<(), FeatureError> {
        // Confirm requesting user is host
        let user_id = i64::from(req.user);

        let Ok(event) = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id)
            .fetch_one(&self.database)
            .await
        else {
            ctx.responder.respond(ephemeral("Event not found")).await?;
            return Ok(());
        };
        if event.host_id != user_id {
            ctx.responder
                .respond(ephemeral("You are not the host of this event"))
                .await?;
            return Ok(());
        }

        // Confrim that event's status is preparing or running
        let current_status = SSState::from(event.status as i32);
        let was_running = matches!(current_status, SSState::Running);
        if !matches!(current_status, SSState::PreRun | SSState::Running) {
            ctx.responder
                .respond(ephemeral("Event cannot be canceled (already finished)"))
                .await?;
            return Ok(());
        }

        // Change event status to finished
        let finished_status = i32::from(SSState::Finished);
        sqlx::query!(
            "UPDATE events SET status = ? WHERE id = ?",
            finished_status,
            evt_id
        )
        .execute(&self.database)
        .await?;

        ctx.responder
            .respond(ephemeral("Event canceled successfully!"))
            .await?;

        // If it was running, notify each participant that event is now canceled with the event's name and description
        if was_running {
            let participants = sqlx::query!(
                "SELECT user_id FROM event_participants WHERE event_id = ?",
                evt_id
            )
            .fetch_all(&self.database)
            .await?;

            // Send notifications to participants
            for participant in participants {
                ctx.discord
                    .send_dm(
                        UserId::new(participant.user_id as u64),
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
