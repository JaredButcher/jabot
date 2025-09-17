use serenity::all::{
    Action, ActionRowComponent, Command, CommandOptionType, CreateCommand, CreateCommandOption, CreateInputText, CreateInteractionResponse, CreateInteractionResponseMessage, CreateModal, InputTextStyle, Interaction
};
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use sqlx::database;
use std::env;
use std::fs;

mod strings;
use strings::Strings;

struct Bot{
    database: sqlx::SqlitePool,
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
                                    if let Some(evt_id_opt) = command.data.options.get(1) && evt_id_opt.kind() == CommandOptionType::Integer{
                                        let evt_id = evt_id_opt.value.as_i64().unwrap();
                                        if let Ok(rec) = sqlx::query!("SELECT * FROM events WHERE id = ?", evt_id).fetch_one(&self.database).await {
                                            if rec.host_id == i64::from(command.user.id) {
                                                // Send modal to modify event
                                                CreateInteractionResponse::Modal(
                                                    CreateModal::new(format!("{}:{}", Strings::MODAL_SS_CREATE_ID, rec.host_id), Strings::MODAL_SS_CREATE_TITLE)
                                                    .components(vec![
                                                        serenity::builder::CreateActionRow::InputText(
                                                            serenity::builder::CreateInputText::new(
                                                                InputTextStyle::Short, 
                                                                Strings::MODAL_SS_INFO_NAME_LABEL, 
                                                                Strings::MODAL_SS_INFO_NAME_ID
                                                            )
                                                            .placeholder(rec.name)
                                                            .required(true)
                                                        ),
                                                        serenity::builder::CreateActionRow::InputText(
                                                            serenity::builder::CreateInputText::new(
                                                                InputTextStyle::Short, 
                                                                Strings::MODAL_SS_INFO_DESC_LABEL, 
                                                                Strings::MODAL_SS_INFO_DESC_ID
                                                            )
                                                            .placeholder(rec.description.unwrap_or("".to_string()))
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
                                    CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new().content("List")
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
            },
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
                                        },
                                        Strings::MODAL_SS_INFO_DESC_ID => {
                                            desc = input.value.clone().unwrap();
                                        },
                                        _ => {}
                                    }
                                },
                                ActionRowComponent::Button(input) => {

                                },
                                ActionRowComponent::SelectMenu(input) => {
                                    
                                },
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
                },
                Strings::MODAL_SS_CREATE_ID => {
                    // Create Event
                    let mut name: Option<String> = None;
                    let mut description: Option<String> = None;
                    for comp in modal.data.components.iter().flat_map(|e| e.components.iter()) {
                        if let ActionRowComponent::InputText(input_text_comp) = comp {
                            match input_text_comp.custom_id.as_str() {
                                Strings::MODAL_SS_INFO_NAME_ID => {
                                    name = input_text_comp.value.clone();
                                },
                                Strings::MODAL_SS_INFO_DESC_ID => {
                                    description = input_text_comp.value.clone();
                                },
                                c => { println!("Unreconnized component ss create {}", c) }
                            }
                        }
                    }
                    // Insert host if not present
                    // Insert make sure below event limit per host
                    // Insert event
                    sqlx::query!("INSERT INTO events (name, description, host_id, status) VALUES (?, ?, ?, ?)")
                },
                c if c.contains(Strings::MODAL_SS_CREATE_ID) => {
                    // Modify Event
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
                    .add_sub_option(CreateCommandOption::new(
                        CommandOptionType::Integer,
                        Strings::OPT_SS_EVT_ID_NAME,
                        Strings::OPT_SS_EVT_ID_DESC,
                    )),
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

    sqlx::migrate!("./migrations").run(&database).await.expect("Couldn't run database migrations");

    let bot = Bot {
        database: database,
    };

    let mut client = Client::builder(&token, intents)
        .event_handler(bot)
        .await
        .expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}
