use std::env;
use std::fs;
use std::sync::Arc;

use jabot::features::secret_santa::{SecretSanta, SqliteSecretSantaRepo};
use jabot::framework::{FeatureRegistry, SqliteUserRepo};
use serenity::all::{Command, Interaction};
use serenity::async_trait;
use serenity::model::gateway::Ready;
use serenity::prelude::*;

struct Bot {
    registry: FeatureRegistry,
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, ready: Ready) {
        tracing::info!("{} is connected", ready.user.name);
        // One call replaces the whole global set, so commands removed from code disappear from Discord.
        if let Err(error) = Command::set_global_commands(&ctx.http, self.registry.commands()).await
        {
            tracing::error!(%error, "could not register slash commands");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        self.registry.dispatch(ctx.http.clone(), interaction).await;
    }
}

fn get_discord_token() -> Result<String, Box<dyn std::error::Error>> {
    // Check for DISCORD_TOKEN_FILE environment variable
    if let Ok(token_file_path) = env::var("DISCORD_TOKEN_FILE") {
        tracing::info!("Reading Discord token from file: {}", token_file_path);
        let token = fs::read_to_string(&token_file_path)
            .map_err(|e| format!("Failed to read token file '{}': {}", token_file_path, e))?
            .trim()
            .to_string();
        return Ok(token);
    }

    Err("DISCORD_TOKEN_FILE environment variable not found".into())
}

/// Parse `DATABASE_URL` (e.g. `sqlite:database.sqlite`) as a URL, not a file path, so the bot
/// opens the same database the sqlx CLI and `query!` macros use.
fn connect_options(url: &str) -> Result<sqlx::sqlite::SqliteConnectOptions, sqlx::Error> {
    use std::str::FromStr;
    Ok(sqlx::sqlite::SqliteConnectOptions::from_str(url)?.create_if_missing(true))
}

async fn connect_database() -> sqlx::SqlitePool {
    let database_url = env::var("DATABASE_URL").expect("Database url not in enviroment");
    let database = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connect_options(&database_url).expect("Invalid DATABASE_URL"))
        .await
        .expect("Failed to connect to database");

    sqlx::migrate!("./migrations")
        .run(&database)
        .await
        .expect("Couldn't run database migrations");

    database
}

#[tokio::main]
async fn main() {
    // RUST_LOG overrides the default level, e.g. RUST_LOG=jabot=debug.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Try to load .env file if it exists
    match dotenv::dotenv() {
        Ok(_) => tracing::info!("Loaded .env file"),
        Err(err) => tracing::info!("No .env file loaded: {}", err),
    };

    let token = get_discord_token().expect("Failed to get Discord token");
    let pool = connect_database().await;

    let registry = FeatureRegistry::builder(Arc::new(SqliteUserRepo::new(pool.clone())))
        .register(SecretSanta::new(Arc::new(SqliteSecretSantaRepo::new(
            pool.clone(),
        ))))
        .build()
        .expect("Feature registration conflict");

    // Interactions (commands, components, modals) arrive without any gateway intents.
    let mut client = Client::builder(&token, GatewayIntents::empty())
        .event_handler(Bot { registry })
        .await
        .expect("Err creating client");

    if let Err(why) = client.start().await {
        tracing::error!("Client error: {:?}", why);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_options_parses_url_scheme() {
        let opts = connect_options("sqlite:foo.sqlite").unwrap();
        assert_eq!(opts.get_filename(), std::path::Path::new("foo.sqlite"));
    }
}
