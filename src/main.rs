use serenity::all::{
    Action, ActionRowComponent, Command, CommandOptionType, CreateActionRow, CreateCommand, CreateCommandOption, CreateInputText, CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, CreateModal, InputTextStyle, Interaction
};
use serenity::{async_trait, cache};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use sqlx::database;
use std::env;
use std::fs;
use rand::seq::SliceRandom;

mod strings;
use strings::Strings;

const SS_HOST_EVENT_LIMIT: i32 = 32;

struct Bot {
    database: sqlx::SqlitePool,
}

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

impl Bot {
    async fn add_user_to_event(
        &self,
        user: i64,
        event: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
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

    async fn user_join_event(&self, user: i64, event: i64) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query!(
            "UPDATE event_participants SET joined = TRUE WHERE user_id = ? and event_id = ?",
            user, event
        )
        .execute(&self.database)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl EventHandler for Bot {
    async fn message(&self, ctx: Context, msg: Message) {
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(command) => {
                if let Err(why) = command.create_response(&ctx.http, match command.data.name.as_str() {
                    Strings::CMD_SS_NAME => {
                        if let Some(cmd_name_opt) = command.data.options.get(0){
                            println!("SS CMD {}", cmd_name_opt.name);
                        }

                        if let Some(cmd_name_opt) = command.data.options.get(0) {
                            match cmd_name_opt.name.as_str() {
                                Strings::CMD_SS_CREATE_NAME => {
                                    if let serenity::all::CommandDataOptionValue::SubCommand(sub_cmd) = &command.data.options.get(0).unwrap().value &&
                                     let Some(evt_id_opt) = sub_cmd.get(0) &&
                                      evt_id_opt.kind() == CommandOptionType::Integer{
                                        let evt_id = evt_id_opt.value.as_i64().unwrap();
                                        if let Ok(rec) = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id).fetch_one(&self.database).await {
                                            if rec.host_id == i64::from(command.user.id) {
                                                // Send modal to modify event
                                                CreateInteractionResponse::Modal(
                                                    CreateModal::new(format!("{}:{}", Strings::MODAL_SS_CREATE_ID, evt_id), Strings::MODAL_SS_CREATE_EDIT_TITLE)
                                                    .components(vec![
                                                        serenity::builder::CreateActionRow::InputText(
                                                            serenity::builder::CreateInputText::new(
                                                                InputTextStyle::Short,
                                                                Strings::MODAL_SS_INFO_NAME_LABEL,
                                                                Strings::MODAL_SS_INFO_NAME_ID
                                                            )
                                                            .value(rec.name)
                                                            .required(true)
                                                        ),
                                                        serenity::builder::CreateActionRow::InputText(
                                                            serenity::builder::CreateInputText::new(
                                                                InputTextStyle::Short,
                                                                Strings::MODAL_SS_INFO_DESC_LABEL,
                                                                Strings::MODAL_SS_INFO_DESC_ID
                                                            )
                                                            .value(rec.description.unwrap_or("".to_string()))
                                                            .required(false)
                                                        ),
                                                    ])
                                                )
                                            }else{
                                                // Send message that use is not event host
                                                CreateInteractionResponse::Message(
                                                    serenity::builder::CreateInteractionResponseMessage::new().content("Cannot modify event, not event's host"),
                                                )
                                            }
                                        } else {
                                            // Send message that use is not event host
                                            CreateInteractionResponse::Message(
                                                serenity::builder::CreateInteractionResponseMessage::new().content("Event doesn't exist"),
                                            )
                                        }
                                        // Send messsage that event invalid
                                    } else {
                                        // Send modal to create event
                                        CreateInteractionResponse::Modal(
                                            CreateModal::new(Strings::MODAL_SS_CREATE_ID, Strings::MODAL_SS_CREATE_TITLE)
                                            .components(vec![
                                                serenity::builder::CreateActionRow::InputText(
                                                    serenity::builder::CreateInputText::new(
                                                        InputTextStyle::Short,
                                                        Strings::MODAL_SS_INFO_NAME_LABEL,
                                                        Strings::MODAL_SS_INFO_NAME_ID
                                                    )
                                                    .required(true)
                                                ),
                                                serenity::builder::CreateActionRow::InputText(
                                                    serenity::builder::CreateInputText::new(
                                                        InputTextStyle::Short,
                                                        Strings::MODAL_SS_INFO_DESC_LABEL,
                                                        Strings::MODAL_SS_INFO_DESC_ID
                                                    )
                                                    .required(false)
                                                ),
                                            ])
                                        )
                                    }
                                },
                                Strings::CMD_SS_INFO_NAME => {
                                    if let serenity::all::CommandDataOptionValue::SubCommand(sub_cmd) = &command.data.options.get(0).unwrap().value &&
                                     let Some(evt_id_opt) = sub_cmd.get(0) &&
                                      let Some(evt_id) = evt_id_opt.value.as_i64(){
                                        let user_id = i64::from(command.user.id);

                                        // Check if user is a participant and get event info
                                        let event_query = sqlx::query!(
                                            "SELECT e.*, ep.joined FROM events e
                                             JOIN event_participants ep ON e.id = ep.event_id
                                             WHERE e.id = ? AND ep.user_id = ?",
                                            evt_id, user_id
                                        ).fetch_optional(&self.database).await.expect("Failed to fetch event");

                                        // Fetch all event users
                                        let event_users_query = sqlx::query!(
                                            "SELECT ep.user_id, ep.joined, ep.assignee_id FROM events e
                                             JOIN event_participants ep ON e.id = ep.event_id
                                             WHERE e.id = ?", evt_id
                                        ).fetch_all(&self.database).await.expect("Failed to fetch event users");

                                        if let Some(event) = event_query {
                                            let is_host = event.host_id == user_id;
                                            let event_status = SSState::from(event.status as i32);

                                            if let Some(user) = event_users_query.iter().find(|u| u.user_id == user_id) {
                                                let mut components: Vec<CreateActionRow> = vec![];

                                                if is_host {
                                                    // Host view components
                                                    // Add user selection dropdown
                                                    let mut user_ids: Vec<serenity::all::UserId> = vec![];
                                                    for user in event_users_query.iter() {
                                                        user_ids.push(serenity::all::UserId::from(user.user_id as u64));
                                                    }

                                                    if event_status == SSState::PreRun {
                                                        components.push(CreateActionRow::SelectMenu(
                                                            serenity::all::CreateSelectMenu::new(
                                                                format!("{}:{}", Strings::COMP_SS_INFO_USER_ID, evt_id),
                                                                serenity::all::CreateSelectMenuKind::User {
                                                                    default_users: user_ids.into()
                                                                }
                                                            ).placeholder("Add users to event...")
                                                            .max_values(25))
                                                        );
                                                    }

                                                    let mut buttons = vec![];

                                                    let cancel_btn = serenity::all::CreateButton::new(format!("{}:{}", Strings::COMP_SS_BTN_CANCEL_ID, evt_id.to_string()))
                                                                .label("Cancel Event")
                                                                .style(serenity::all::ButtonStyle::Danger);

                                                    match event_status {
                                                        SSState::PreRun => {
                                                            buttons.push(serenity::all::CreateButton::new(format!("{}:{}", Strings::COMP_SS_BTN_START_ID, evt_id.to_string()))
                                                                .label("Start Event")
                                                                .style(serenity::all::ButtonStyle::Success));
                                                            buttons.push(cancel_btn);
                                                        },
                                                        SSState::Running => {
                                                            buttons.push(serenity::all::CreateButton::new(format!("{}:{}", Strings::COMP_SS_BTN_END_ID, evt_id.to_string()))
                                                                .label("End Event")
                                                                .style(serenity::all::ButtonStyle::Success));
                                                            buttons.push(cancel_btn);
                                                        },
                                                        SSState::Finished => {}
                                                    }
                                                    if buttons.len() > 0 {
                                                        components.push(CreateActionRow::Buttons(buttons));
                                                    }
                                                } else {
                                                    // Participant view components
                                                    let mut buttons = vec![];

                                                    /*buttons.push(serenity::all::CreateButton::new(format!("{}:{}", Strings::COMP_SS_BTN_LEAVE_ID, evt_id.to_string()))
                                                        .label("Leave Event")
                                                        .style(serenity::all::ButtonStyle::Danger));*/

                                                    if buttons.len() > 0 {
                                                        components.push(CreateActionRow::Buttons(buttons));
                                                    }
                                                }

                                                let status_text = match event_status {
                                                    SSState::PreRun => "Preparing",
                                                    SSState::Running => "Running",
                                                    SSState::Finished => "Finished"
                                                };

                                                let mut content = format!(
                                                    "**Event Information**\n**Name:** {}\n**Description:** {}\n**Status:** {}\n**Host:** <@{}>",
                                                    event.name,
                                                    event.description.unwrap_or("No description".to_string()),
                                                    status_text,
                                                    event.host_id
                                                );
                                                if !is_host || event_status != SSState::PreRun {
                                                    let user_text_list = event_users_query.iter().map(|u| format!("<@{}>", u.user_id)).collect::<Vec<String>>().join(", ");
                                                    content += format!("\n**Participants:** {}", user_text_list).as_str();
                                                }
                                                if let Some(assignee_id) = user.assignee_id {
                                                    content += format!("\n**Get a gift for:** <@{}>", assignee_id).as_str();
                                                } 

                                                let mut response = serenity::builder::CreateInteractionResponseMessage::new()
                                                .content(content)
                                                .ephemeral(true);
                                                if components.len() > 0 {
                                                    response = response.components(components);
                                                }
                                                CreateInteractionResponse::Message(response)
                                            }else{
                                                CreateInteractionResponse::Message(
                                                    serenity::builder::CreateInteractionResponseMessage::new()
                                                        .content("You are not a participant in this event")
                                                        .ephemeral(true)
                                                )
                                            }
                                        } else {
                                            CreateInteractionResponse::Message(
                                                serenity::builder::CreateInteractionResponseMessage::new()
                                                    .content("Event not found or you are not a participant in this event.")
                                                    .ephemeral(true)
                                            )
                                        }
                                    } else {
                                        CreateInteractionResponse::Message(
                                            serenity::builder::CreateInteractionResponseMessage::new()
                                                .content("Please provide an event ID.")
                                                .ephemeral(true)
                                        )
                                    }
                                },
                                Strings::CMD_SS_LIST_NAME => {
                                    let host_id = i64::from(command.user.id);
                                    let evts = sqlx::query!("SELECT e.*, ep.user_id FROM events e JOIN event_participants ep ON e.id = ep.event_id WHERE ep.user_id = ?", host_id)
                                    .fetch_all(&self.database).await.expect("Failed to fetch events");

                                    let mut result_str: String = "---Events---\r\n".to_string();
                                    for evt in evts {
                                        // Get host user from Discord API/cache
                                        let host_user_id = serenity::model::id::UserId::new(evt.host_id as u64);
                                        let host_name = match host_user_id.to_user(&ctx.http).await {
                                            Ok(user) => user.name,
                                            Err(_) => format!("Unknown User ({})", evt.host_id)
                                        };

                                        let status_text = match SSState::from(evt.status as i32) {
                                            SSState::PreRun => "Preparing",
                                            SSState::Running => "Running",
                                            SSState::Finished => "Finished"
                                        };

                                        result_str += format!("**ID:** {}\n**Name:** {}\n**Description:** {}\n **Status:** {}\n**Host:** {}\n\n", evt.id, evt.name, evt.description.unwrap_or("".to_string()), status_text, host_name).as_str();
                                    }

                                    CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new().content(result_str)
                                        .ephemeral(true)
                                    )
                                },
                                Strings::CMD_SS_JOIN_NAME => {
                                    if let Some(evt_id_opt) = command.data.options.get(0) && let Some(evt_id) = evt_id_opt.value.as_i64() {
                                        match self.user_join_event(i64::from(command.user.id), evt_id).await {
                                            Ok(_) => {
                                                CreateInteractionResponse::Message(
                                                    serenity::builder::CreateInteractionResponseMessage::new().content("Joined Event")
                                                    .ephemeral(true)
                                                )
                                            },
                                            Err(err) => {
                                                println!("Failed to join event {}", err);
                                                CreateInteractionResponse::Message(
                                                    serenity::builder::CreateInteractionResponseMessage::new().content("Failed to join event")
                                                    .ephemeral(true)
                                                )
                                            }
                                        }
                                    } else {
                                        CreateInteractionResponse::Message(
                                            serenity::builder::CreateInteractionResponseMessage::new().content("Missing argument")
                                            .ephemeral(true)
                                        )
                                    }
                                },
                                _ => {
                                    println!("Unreconnized command {}", cmd_name_opt.name);
                                    CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new().content(format!("Unreconnized Command {}", cmd_name_opt.name))
                                        .ephemeral(true)
                                    )
                                }
                            }
                        } else {
                            println!("No SS command name given");
                            CreateInteractionResponse::Message(
                                serenity::builder::CreateInteractionResponseMessage::new().content("No sub command given")
                                .ephemeral(true)
                            )
                        }
                    },
                    _ => {
                        CreateInteractionResponse::Message(
                            serenity::builder::CreateInteractionResponseMessage::new().content("Leave")
                            .ephemeral(true)
                        )
                    }
                }).await {
                    println!("Cannot respond to slash command: {}", why);
                }
            }
            Interaction::Modal(modal) => match modal.data.custom_id.as_str() {
                Strings::MODAL_SS_CREATE_ID => {
                    // Create Event
                    let mut name: String = Strings::DEFAULT_SS_NAME.to_string();
                    let mut description: String = Strings::DEFAULT_SS_DESCRIPTION.to_string();
                    for comp in modal
                        .data
                        .components
                        .iter()
                        .flat_map(|e| e.components.iter())
                    {
                        if let ActionRowComponent::InputText(input_text_comp) = comp {
                            match input_text_comp.custom_id.as_str() {
                                Strings::MODAL_SS_INFO_NAME_ID => {
                                    if let Some(value) = input_text_comp.value.clone() {
                                        name = value;
                                    }
                                }
                                Strings::MODAL_SS_INFO_DESC_ID => {
                                    if let Some(value) = input_text_comp.value.clone() {
                                        description = value;
                                    }
                                }
                                c => {
                                    println!("Unreconnized component ss create {}", c)
                                }
                            }
                        }
                    }
                    // Insert host if not present
                    let host_id = i64::from(modal.user.id);
                    sqlx::query!("INSERT INTO event_users (id, global_wish) SELECT ?, ? WHERE NOT EXISTS ( SELECT 1 FROM event_users WHERE id = ? )",
                    host_id, "", host_id).execute(&self.database).await.expect("Failed to check / insert host");

                    // Check if host has reached event limit
                    let pre_run_status = i32::from(SSState::PreRun);
                    let running_status = i32::from(SSState::Running);
                    let active_events = sqlx::query!("SELECT COUNT(*) as count FROM events WHERE host_id = ? AND (status = ? OR status = ?)",
                        host_id, pre_run_status, running_status)
                        .fetch_one(&self.database).await.expect("Failed to count host events");

                    if active_events.count >= SS_HOST_EVENT_LIMIT as i64 {
                        // Send error message that host has reached limit
                        if let Err(why) = modal.create_response(&ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!("Cannot create event: You have reached the limit of {} active events", SS_HOST_EVENT_LIMIT))
                            )
                        ).await {
                            println!("Cannot respond to modal: {}", why);
                        }
                        return;
                    }

                    // Insert event
                    let result = sqlx::query!("INSERT INTO events (name, description, host_id, status) VALUES (?, ?, ?, ?)", name, description, host_id, pre_run_status).execute(&self.database).await.expect("Failed to insert event");
                    let event_id = result.last_insert_rowid();

                    // Add the host as a participant in their own event
                    if let Err(err) = self.add_user_to_event(host_id, event_id).await {
                        println!("Failed to add host to event: {}", err);
                    }

                    if let Err(why) = modal
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!("Event created\n**Id:** {}\n**Name:** {}", event_id, name))
                                    .ephemeral(true),
                            ),
                        )
                        .await
                    {
                        println!("Cannot respond to modal: {}", why);
                    }
                    return;
                }
                c if c.contains(Strings::MODAL_SS_CREATE_ID) => {
                    // Modify Event
                    if let Some(evt_id_str) = c.splitn(8, ":").last()
                        && let Ok(evt_id) = evt_id_str.parse::<i64>()
                    {
                        let mut name: Option<String> = None;
                        let mut description: Option<String> = None;
                        for comp in modal
                            .data
                            .components
                            .iter()
                            .flat_map(|e| e.components.iter())
                        {
                            if let ActionRowComponent::InputText(input_text_comp) = comp {
                                match input_text_comp.custom_id.as_str() {
                                    Strings::MODAL_SS_INFO_NAME_ID => {
                                        name = input_text_comp.value.clone();
                                    }
                                    Strings::MODAL_SS_INFO_DESC_ID => {
                                        description = input_text_comp.value.clone();
                                    }
                                    c => {
                                        println!("Unreconnized component ss create {}", c)
                                    }
                                }
                            }
                        }

                        // Fetch existing event if it exists and the command's user is the host
                        if let Ok(existing_event) =
                            sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id)
                                .fetch_one(&self.database)
                                .await
                        {
                            if existing_event.host_id == i64::from(modal.user.id) {
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
                                .await
                                .expect("Failed to update event");

                                println!("Event '{}' updated", update_name);
                                if let Err(why) = modal
                                    .create_response(
                                        &ctx.http,
                                        CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new().content(
                                                format!(
                                                    "Event '{}' updated successfully",
                                                    update_name
                                                ),
                                            ),
                                        ),
                                    )
                                    .await
                                {
                                    println!("Cannot respond to modal: {}", why);
                                }
                            } else {
                                if let Err(why) = modal.create_response(&ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content("Cannot modify event: You are not the host of this event")
                                    )
                                ).await {
                                    println!("Cannot respond to modal: {}", why);
                                }
                            }
                        } else {
                            if let Err(why) = modal
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content("Event not found"),
                                    ),
                                )
                                .await
                            {
                                println!("Cannot respond to modal: {}", why);
                            }
                        }
                    } else {
                        if let Err(why) = modal
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Invalid event ID"),
                                ),
                            )
                            .await
                        {
                            println!("Cannot respond to modal: {}", why);
                        }
                    }
                }
                _ => {
                    println!("Unknown modal ID: {}", modal.data.custom_id);
                }
            },
            Interaction::Component(component) => match component.data.custom_id.as_str() {
                c if c.contains(Strings::COMP_SS_INFO_USER_ID) => {
                    if let Some(evt_id_str) = c.splitn(8, ":").last()
                    && let Ok(evt_id) = evt_id_str.parse::<i64>() {
                        if let serenity::all::ComponentInteractionDataKind::UserSelect { values } = &component.data.kind {
                            // Check if event exists and user is the event host and get event info
                            let event_query = sqlx::query!(
                                "SELECT * FROM events WHERE id = ?", evt_id
                            ).fetch_one(&self.database).await.expect("Failed to fetch event");

                            if event_query.status != SSState::PreRun as i64 {
                                component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new().content("Cannot modify users of started event")
                                    .ephemeral(true)
                                )).await.expect("Failed to send message");
                                return;
                            }

                            // Fetch all event users
                            let event_users_query = sqlx::query!(
                                "SELECT ep.user_id, ep.joined FROM events e
                                    JOIN event_participants ep ON e.id = ep.event_id
                                    WHERE e.id = ?", evt_id
                            ).fetch_all(&self.database).await.expect("Failed to fetch event users");

                            // Get existing participant IDs for comparison
                            let existing_participants: std::collections::HashSet<i64> = event_users_query.iter().map(|p| p.user_id).collect();

                            // Find users to add (selected but not already participating)
                            let users_to_add: Vec<i64> = values.iter()
                                .map(|user| u64::from(*user) as i64)
                                .filter(|&user_id| !existing_participants.contains(&user_id))
                                .collect();
                            // Bulk insert all users into event_users and event_participants
                            if !users_to_add.is_empty() {
                                // Build VALUES clauses for bulk insert
                                let user_values: Vec<String> = users_to_add.iter().map(|_| "(?)"[..].to_string()).collect();
                                let participant_values: Vec<String> = users_to_add.iter().map(|_| "(?, ?, TRUE)"[..].to_string()).collect();

                                let user_query = format!("INSERT OR IGNORE INTO event_users (id) VALUES {}", user_values.join(", "));
                                let participant_query = format!("INSERT OR IGNORE INTO event_participants (user_id, event_id, joined) VALUES {}", participant_values.join(", "));

                                // Execute user insert
                                let mut user_query_builder = sqlx::query(&user_query);
                                for &user_id in &users_to_add {
                                    user_query_builder = user_query_builder.bind(user_id);
                                }
                                user_query_builder.execute(&self.database).await.expect("Failed to bulk insert event_users");

                                // Execute participant insert
                                let mut participant_query_builder = sqlx::query(&participant_query);
                                for &user_id in &users_to_add {
                                    participant_query_builder = participant_query_builder.bind(user_id).bind(evt_id);
                                }
                                participant_query_builder.execute(&self.database).await.expect("Failed to bulk insert event_participants");
                            }

                            let updated_participant_set: std::collections::HashSet<i64> = values.iter().map(|user_id| u64::from(*user_id) as i64 ).collect();
                            let users_to_remove: Vec<i64> = existing_participants.iter()
                                .map(|uid| *uid)
                                .filter(|uid| !updated_participant_set.contains(uid))
                                .collect();

                            println!("Modify users add: {:?} remove: {:?}", users_to_add, users_to_remove);

                            // Bulk remove participants no longer selected
                            if !users_to_remove.is_empty() {
                                let placeholders: Vec<String> = users_to_remove.iter().map(|_| "?".to_string()).collect();
                                let remove_query = format!("DELETE FROM event_participants WHERE event_id = ? AND user_id IN ({})", placeholders.join(", "));

                                let mut remove_query_builder = sqlx::query(&remove_query).bind(evt_id);
                                for &user_id in &users_to_remove {
                                    remove_query_builder = remove_query_builder.bind(user_id);
                                }
                                remove_query_builder.execute(&self.database).await.expect("Failed to bulk remove event_participants");
                            }

                            component.create_response(&ctx.http, CreateInteractionResponse::Acknowledge).await.expect("Failed to acknowlage component");

                            // Notify users afterwards to avoid ack timeout
                            for &user_id in &users_to_add {
                                let user = serenity::all::UserId::from(user_id as u64);
                                user.direct_message(&ctx.http, CreateMessage::new()
                                    .content(format!("You have been invited to a Secret Santa Event: {}\r\n{}", &event_query.name, event_query.description.as_deref().unwrap_or(""))
                                )).await.expect("Failed to send invite dm");
                            }
                        } else {
                            component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new().content("User does not exist")
                                .ephemeral(true)
                            )).await.expect("Failed to send message");
                        }
                    } else {
                        println!("No event provided {}", c);
                        component.create_response(&ctx.http, CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new().content("No event provided")
                            .ephemeral(true)
                        )).await.expect("Failed to send message");
                    }
                }
                c if c.contains(Strings::COMP_SS_BTN_START_ID) => {
                    if let Some(evt_id_str) = c.splitn(8, ":").last()
                    && let Ok(evt_id) = evt_id_str.parse::<i64>() {
                        // Confirm requesting user is host
                        let user_id = i64::from(component.user.id);

                        if let Ok(event) = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id)
                            .fetch_one(&self.database).await {

                            if event.host_id != user_id {
                                component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("You are not the host of this event")
                                        .ephemeral(true)
                                )).await.expect("Failed to send message");
                                return;
                            }

                            // Confrim that event's status is preparing
                            let current_status = SSState::from(event.status as i32);
                            if current_status != SSState::PreRun {
                                component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Event is not in preparing state")
                                        .ephemeral(true)
                                )).await.expect("Failed to send message");
                                return;
                            }

                            // Notify each participant that event is now running with the event's name and description
                            let participants = sqlx::query!(
                                "SELECT user_id, event_wish FROM event_participants WHERE event_id = ? AND joined = TRUE",
                                evt_id
                            ).fetch_all(&self.database).await.expect("Failed to fetch participants");

                            let participants_cnt = participants.iter().count();

                            if participants_cnt > 1 {
                                // Change event status to running
                                let running_status = i32::from(SSState::Running);
                                sqlx::query!("UPDATE events SET status = ? WHERE id = ?", running_status, evt_id)
                                    .execute(&self.database).await.expect("Failed to update event status");

                                // Shuffle records and assign partipants their secret santas
                                let mut participants_shuffled: Vec<(i64, i64)> = participants.iter().map(|f| (f.user_id, 0)).collect();
                                participants_shuffled.shuffle(&mut rand::rng());

                                // Assign each participant to give a gift to the next person in the shuffled list
                                for i in 0..participants_cnt {
                                    participants_shuffled[i].1 = participants_shuffled[(i + 1) % participants_cnt].0;
                                }

                                component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Event started successfully!")
                                        .ephemeral(true)
                                )).await.expect("Failed to send message");

                                // Save the secret santas and send notifications to participants
                                for participant in participants_shuffled {
                                    sqlx::query!("UPDATE event_participants SET assignee_id = ? WHERE user_id = ?", participant.1, participant.0)
                                    .execute(&self.database).await.expect("Failed to update assingees");

                                    let user = serenity::all::UserId::from(participant.0 as u64);
                                    user.direct_message(&ctx.http, CreateMessage::new()
                                        .content(format!("The Secret Santa Event **{}** has started!\n{}\nYou are expected to give a gift to **<@{}>**\nUse the command `\\ss info {}` to check the event's status.",
                                            event.name,
                                            event.description.as_deref().unwrap_or(""),
                                            participant.1,
                                            event.id
                                        ))
                                    ).await.expect("Failed to send notification dm");
                                }
                            } else {
                                component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Secret Santa requires more than one participant")
                                        .ephemeral(true)
                                )).await.expect("Failed to send message");
                            }
                        } else {
                            component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Event not found")
                                    .ephemeral(true)
                            )).await.expect("Failed to send message");
                        }
                    }
                },
                c if c.contains(Strings::COMP_SS_BTN_END_ID) => {
                    if let Some(evt_id_str) = c.splitn(8, ":").last()
                    && let Ok(evt_id) = evt_id_str.parse::<i64>() {
                        // Confirm requesting user is host
                        let user_id = i64::from(component.user.id);

                        if let Ok(event) = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id)
                            .fetch_one(&self.database).await {

                            if event.host_id != user_id {
                                component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("You are not the host of this event")
                                        .ephemeral(true)
                                )).await.expect("Failed to send message");
                                return;
                            }

                            // Confrim that event's status is running
                            let current_status = SSState::from(event.status as i32);
                            if current_status != SSState::Running {
                                component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Event is not currently running")
                                        .ephemeral(true)
                                )).await.expect("Failed to send message");
                                return;
                            }

                            // Change event status to finished
                            let finished_status = i32::from(SSState::Finished);
                            sqlx::query!("UPDATE events SET status = ? WHERE id = ?", finished_status, evt_id)
                                .execute(&self.database).await.expect("Failed to update event status");

                            // Notify each participant that event has finished
                            let participants = sqlx::query!(
                                "SELECT user_id FROM event_participants WHERE event_id = ? AND joined = TRUE",
                                evt_id
                            ).fetch_all(&self.database).await.expect("Failed to fetch participants");

                            component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Event ended successfully!")
                                    .ephemeral(true)
                            )).await.expect("Failed to send message");

                            // Send notifications to participants
                            for participant in participants {
                                let user = serenity::all::UserId::from(participant.user_id as u64);
                                if let Err(why) = user.direct_message(&ctx.http, CreateMessage::new()
                                    .content(format!("The Secret Santa Event '{}' has concluded successfully!\n{}",
                                        event.name,
                                        event.description.as_deref().unwrap_or("")
                                    ))
                                ).await {
                                    println!("End notification error {}", why);
                                }
                            }
                        } else {
                            component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Event not found")
                                    .ephemeral(true)
                            )).await.expect("Failed to send message");
                        }
                    }
                },
                c if c.contains(Strings::COMP_SS_BTN_CANCEL_ID) => {
                    if let Some(evt_id_str) = c.splitn(8, ":").last()
                    && let Ok(evt_id) = evt_id_str.parse::<i64>() {
                        // Confirm requesting user is host
                        let user_id = i64::from(component.user.id);

                        if let Ok(event) = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id)
                            .fetch_one(&self.database).await {

                            if event.host_id != user_id {
                                component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("You are not the host of this event")
                                        .ephemeral(true)
                                )).await.expect("Failed to send message");
                                return;
                            }

                            // Confrim that event's status is preparing or running
                            let current_status = SSState::from(event.status as i32);
                            let was_running = matches!(current_status, SSState::Running);
                            if !matches!(current_status, SSState::PreRun | SSState::Running) {
                                component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Event cannot be canceled (already finished)")
                                        .ephemeral(true)
                                )).await.expect("Failed to send message");
                                return;
                            }

                            // Change event status to finished
                            let finished_status = i32::from(SSState::Finished);
                            sqlx::query!("UPDATE events SET status = ? WHERE id = ?", finished_status, evt_id)
                                .execute(&self.database).await.expect("Failed to update event status");

                            component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Event canceled successfully!")
                                    .ephemeral(true)
                            )).await.expect("Failed to send message");

                            // If it was running, notify each participant that event is now canceled with the event's name and description
                            if was_running {
                                let participants = sqlx::query!(
                                    "SELECT user_id FROM event_participants WHERE event_id = ? AND joined = TRUE",
                                    evt_id
                                ).fetch_all(&self.database).await.expect("Failed to fetch participants");

                                // Send notifications to participants
                                for participant in participants {
                                    let user = serenity::all::UserId::from(participant.user_id as u64);
                                    user.direct_message(&ctx.http, CreateMessage::new()
                                        .content(format!("The Secret Santa Event '{}' has been canceled by the host.\n{}",
                                            event.name,
                                            event.description.as_deref().unwrap_or("")
                                        ))
                                    ).await.expect("Failed to send notification dm");
                                }
                            }
                        } else {
                            component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Event not found")
                                    .ephemeral(true)
                            )).await.expect("Failed to send message");
                        }
                    }
                },
                c if c.contains(Strings::COMP_SS_BTN_LEAVE_ID) => {
                    if let Some(evt_id_str) = c.splitn(8, ":").last()
                    && let Ok(evt_id) = evt_id_str.parse::<i64>() {
                        // Confirm requesting user is a participant in the event
                        let user_id = i64::from(component.user.id);

                        // Check if user is a participant and get event info
                        let participant_query = sqlx::query!(
                            "SELECT ep.*, e.name, e.description, e.status FROM event_participants ep
                             JOIN events e ON ep.event_id = e.id
                             WHERE ep.event_id = ? AND ep.user_id = ?",
                            evt_id, user_id
                        ).fetch_optional(&self.database).await.expect("Failed to fetch participant");

                        if let Some(participant) = participant_query {
                            // Confrim that event's status is preparing or running
                            let current_status = SSState::from(participant.status as i32);
                            if !matches!(current_status, SSState::PreRun | SSState::Running) {
                                component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Cannot leave event (already finished)")
                                        .ephemeral(true)
                                )).await.expect("Failed to send message");
                                return;
                            }

                            if current_status == SSState::Running {
                                // Get participant's assignee
                                if let Some(assignee_query) = sqlx::query!(
                                    "SELECT assignee_id FROM event_participants WHERE event_id = ? AND user_id = ?",
                                    evt_id, user_id
                                ).fetch_optional(&self.database).await.expect("Failed to get removeal participant's assignee") {
                                    // Update participant assigned to this participant
                                    if let Some(removed_participants_santa) = sqlx::query!(
                                        "SELECT user_id FROM event_participants WHERE event_id = ? AND assignee_id = ?",
                                        evt_id, user_id
                                    ).fetch_optional(&self.database).await.expect("Failed to get removeal participant's assignee"){
                                        sqlx::query!("UPDATE event_participants SET assignee_id = ? WHERE event_id = ? AND user_id = ?", 
                                            assignee_query.assignee_id, evt_id, removed_participants_santa.user_id
                                        ).execute(&self.database).await.expect("Failed to update assignee");
                                        // Message user of new assignment
                                        let assignee_user = serenity::all::UserId::from(assignee_query.assignee_id.unwrap() as u64).to_user(&ctx.http).await.expect("Failed to fetch info on assignee");
                                        let previous_santa = serenity::all::UserId::from(removed_participants_santa.user_id as u64).to_user(&ctx.http).await.expect("Failed to fetch info on previous santa");
                                        previous_santa.direct_message(&ctx.http, CreateMessage::new()
                                            .content(format!("The user **{}** has left the Secret Santa Event **{}**.\nYou have been reassigned to get a gift for **{}**",
                                                component.user.display_name(),
                                                participant.name,
                                                assignee_user.display_name()
                                            ))
                                        ).await.expect("Failed to send notification dm");
                                    }
                                }
                            }

                            // Remove participant
                            sqlx::query!(
                                "DELETE FROM event_participants WHERE event_id = ? AND user_id = ?",
                                evt_id, user_id
                            ).execute(&self.database).await.expect("Failed to remove participant");

                            component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content(format!("You have left the event '{}'", participant.name))
                                    .ephemeral(true)
                            )).await.expect("Failed to send message");
                        } else {
                            component.create_response(&ctx.http, CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("You are not a participant in this event")
                                    .ephemeral(true)
                            )).await.expect("Failed to send message");
                        }
                    }
                },
                c => { println!("Unreconnized component interaction {}", c) }
            }
            _ => {}
        }
    }

    // Build command interface
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        let mut cmds: Vec<CreateCommand> = vec![];

        cmds.push(
            CreateCommand::new(Strings::CMD_SS_NAME)
                .description(Strings::CMD_SS_DESC)
                .contexts(vec![serenity::all::InteractionContext::Guild, serenity::all::InteractionContext::BotDm, serenity::all::InteractionContext::PrivateChannel])
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
                ))
                /*.add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        Strings::CMD_SS_WISH_NAME,
                        Strings::CMD_SS_WISH_DESC,
                    )
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::Integer,
                        Strings::OPT_SS_EVT_ID_NAME,
                        Strings::OPT_SS_EVT_ID_DESC,
                    )),
                )*/,
        );

        for cmd in cmds {
            if let Err(why) = Command::create_global_command(&ctx.http, cmd).await {
                println!("Cannot create slash command: {}", why);
            }
        }
    }
}

/*
impl Handler {
    async fn interaction_echo_handler(&self, ctx: &Context, interaction: &Interaction, command: &CommandInteraction){

    }
}*/

fn get_discord_token() -> Result<String, Box<dyn std::error::Error>> {
    // Check for DISCORD_TOKEN_FILE environment variable
    if let Ok(token_file_path) = env::var("DISCORD_TOKEN_FILE") {
        println!("Reading Discord token from file: {}", token_file_path);
        let token = fs::read_to_string(&token_file_path)
            .map_err(|e| format!("Failed to read token file '{}': {}", token_file_path, e))?
            .trim()
            .to_string();
        return Ok(token);
    }

    Err("DISCORD_TOKEN_FILE environment variable not found".into())
}

#[tokio::main]
async fn main() {
    // Try to load .env file if it exists
    match dotenv::dotenv() {
        Ok(_) => println!("Loaded .env file"),
        Err(err) => println!("{}", err),
    };

    let token = get_discord_token().expect("Failed to get Discord token");

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let database = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(std::env::var("DATABASE_URL").expect("Database url not in enviroment"))
                .create_if_missing(true),
        )
        .await
        .expect("Failed to connect to database");

    sqlx::migrate!("./migrations")
        .run(&database)
        .await
        .expect("Couldn't run database migrations");

    let bot = Bot { database: database };

    let mut client = Client::builder(&token, intents)
        .event_handler(bot)
        .await
        .expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}
