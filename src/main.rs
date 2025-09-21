use serenity::all::{
    Action, ActionRowComponent, Command, CommandOptionType, CreateCommand, CreateCommandOption,
    CreateInputText, CreateInteractionResponse, CreateInteractionResponseMessage, CreateModal,
    InputTextStyle, Interaction,
};
use serenity::{async_trait, cache};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use sqlx::database;
use std::env;
use std::fs;

mod strings;
use strings::Strings;

const SS_HOST_EVENT_LIMIT: i32 = 32;

struct Bot {
    database: sqlx::SqlitePool,
}

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
            "INSERT OR IGNORE INTO event_participants (user_id, event_id, joined) VALUES (?, ?, FALSE)",
            user,
            event
        )
        .execute(&self.database)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl EventHandler for Bot {
    async fn message(&self, ctx: Context, msg: Message) {
        /*if msg.content == Strings::CMD_ {
            if let Err(why) = msg.channel_id.say(&ctx.http, Strings::HELLO_RESPONSE).await {
                println!("Error sending message: {:?}", why);
            }
        }*/
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(command) => {
                if let Err(why) = command.create_response(&ctx.http, match command.data.name.as_str() {
                    Strings::CMD_ECHO_NAME => {
                        let text = command.data.options.get(0).unwrap().value.as_str().unwrap();
                        CreateInteractionResponse::Message(
                            serenity::builder::CreateInteractionResponseMessage::new().content(format!("Echo: {}", text)),
                        )
                    },
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
                                    CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new()
                                        .components(vec![
                                            serenity::all::CreateActionRow::InputText(
                                                CreateInputText::new(
                                                    InputTextStyle::Short,
                                                    Strings::MODAL_SS_INFO_NAME_LABEL,
                                                    Strings::MODAL_SS_INFO_NAME_ID,
                                                )
                                                .placeholder(Strings::MODAL_SS_INFO_NAME_PLACEHOLDER)
                                                .required(true),
                                            ),
                                            serenity::all::CreateActionRow::InputText(
                                                serenity::all::CreateInputText::new(
                                                    InputTextStyle::Paragraph,
                                                    Strings::MODAL_SS_INFO_DESC_LABEL,
                                                    Strings::MODAL_SS_INFO_DESC_ID,
                                                )
                                                .placeholder(Strings::MODAL_SS_INFO_DESC_PLACEHOLDER)
                                                .required(false),
                                            ),
                                            serenity::all::CreateActionRow::SelectMenu(
                                                serenity::all::CreateSelectMenu::new(
                                                    Strings::MODAL_SS_INFO_USERS_ID,
                                                    serenity::all::CreateSelectMenuKind::User {
                                                        default_users: vec![].into()
                                                    }
                                                )
                                            ),
                                            serenity::all::CreateActionRow::Buttons(vec![
                                                serenity::all::CreateButton::new(
                                                    Strings::MODAL_SS_INFO_USERS_ID
                                                ).label("AHHHH"),
                                            ]),
                                        ])
                                    )

                                    /*let modal = CreateModal::new(
                                        Strings::MODAL_SS_INFO_ID,
                                        Strings::MODAL_SS_INFO_TITLE,
                                    )
                                    .components(vec![
                                        serenity::all::CreateActionRow::InputText(
                                            CreateInputText::new(
                                                InputTextStyle::Short,
                                                Strings::MODAL_SS_INFO_NAME_LABEL,
                                                Strings::MODAL_SS_INFO_NAME_ID,
                                            )
                                            .placeholder(Strings::MODAL_SS_INFO_NAME_PLACEHOLDER)
                                            .required(true),
                                        ),
                                        serenity::all::CreateActionRow::InputText(
                                            serenity::all::CreateInputText::new(
                                                InputTextStyle::Paragraph,
                                                Strings::MODAL_SS_INFO_DESC_LABEL,
                                                Strings::MODAL_SS_INFO_DESC_ID,
                                            )
                                            .placeholder(Strings::MODAL_SS_INFO_DESC_PLACEHOLDER)
                                            .required(false),
                                        ),
                                        /*serenity::all::CreateActionRow::SelectMenu(
                                            serenity::all::CreateSelectMenu::new(
                                                Strings::MODAL_SS_INFO_USERS_ID, 
                                                serenity::all::CreateSelectMenuKind::User {
                                                    default_users: vec![].into()
                                                }
                                            )
                                        ),*/
                                        serenity::all::CreateActionRow::Buttons(vec![
                                            serenity::all::CreateButton::new(
                                                Strings::MODAL_SS_INFO_USERS_ID
                                            ).label("AHHHH"),
                                        ]),
                                    ]);
                                    CreateInteractionResponse::Modal(modal)*/
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

                                        result_str += format!("ID: {}\r\n Name: {}\r\n Description: {}\r\n Status: {}\r\n Host: {}\r\n\r\n", evt.id, evt.name, evt.description.unwrap_or("".to_string()), evt.status, host_name).as_str();
                                    }

                                    CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new().content(result_str)
                                    )
                                },
                                Strings::CMD_SS_JOIN_NAME => {
                                    CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new().content("Join")
                                    )
                                },
                                _ => {
                                    println!("Unreconnized command {}", cmd_name_opt.name);
                                    CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new().content(format!("Unreconnized Command {}", cmd_name_opt.name))
                                    )
                                }
                            }
                        } else {
                            println!("No SS command name given");
                            CreateInteractionResponse::Message(
                                serenity::builder::CreateInteractionResponseMessage::new().content("No sub command given")
                            )
                        }
                    },
                    _ => {
                        CreateInteractionResponse::Message(
                            serenity::builder::CreateInteractionResponseMessage::new().content("Leave")
                        )
                    }
                }).await {
                    println!("Cannot respond to slash command: {}", why);
                }
            }
            Interaction::Modal(modal) => match modal.data.custom_id.as_str() {
                Strings::MODAL_SS_INFO_ID => {
                    let mut name = Strings::DEFAULT_SS_NAME.to_string();
                    let mut desc = Strings::DEFAULT_SS_DESCRIPTION.to_string();
                    for comp in modal.data.components.iter() {
                        for row_comp in comp.components.iter() {
                            match row_comp {
                                ActionRowComponent::InputText(input) => {
                                    match input.custom_id.as_str() {
                                        Strings::MODAL_SS_INFO_NAME_ID => {
                                            name = input.value.clone().unwrap();
                                        }
                                        Strings::MODAL_SS_INFO_DESC_ID => {
                                            desc = input.value.clone().unwrap();
                                        }
                                        _ => {}
                                    }
                                }
                                ActionRowComponent::Button(input) => {}
                                ActionRowComponent::SelectMenu(input) => {}
                                _ => {}
                            }
                        }
                    }

                    let content = format!("Created Event {}\r\n{}", name, desc);

                    let builder = CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new().content(content),
                    );

                    if let Err(why) = modal.create_response(&ctx.http, builder).await {
                        println!("Cannot respond to modal: {}", why);
                    }
                }
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
                                    .content(format!("Event created {}", name)),
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
            _ => {}
        }
    }

    // Build command interface
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        let mut cmds: Vec<CreateCommand> = vec![];

        cmds.push(
            CreateCommand::new(Strings::CMD_ECHO_NAME)
                .description(Strings::CMD_ECHO_DESC)
                .add_context(serenity::all::InteractionContext::PrivateChannel)
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        Strings::OPT_TEXT,
                        Strings::OPT_TEXT_DESC,
                    )
                    .required(true),
                ),
        );
        /*
        ss
            create - modal - name, description
            info - name, description, wish, users, host, start, delete, leave, request wish
            - join -
            wish - modal - wish
            list -
         */

        cmds.push(
            CreateCommand::new(Strings::CMD_SS_NAME)
                .description(Strings::CMD_SS_DESC)
                .add_context(serenity::all::InteractionContext::PrivateChannel)
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
                .add_option(
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
                ),
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
