use std::env;
use std::fs;
use std::sync::Arc;

use jabot::backup::{Alerts, Backup, BackupConfig, ResticCli, run_scheduler};
use jabot::features::kv::{Kv, SqliteKvRepo};
use jabot::features::secret_santa::{SecretSanta, SqliteSecretSantaRepo};
use jabot::features::tell::{SqliteTellRepo, Tell, TellConfig};
use jabot::framework::{
    FeatureRegistry, HttpConfig, SerenityDiscordApi, SqliteUserRepo, parse_base_path,
    parse_trusted_proxies, serve,
};
use serenity::all::{Command, Interaction, Message, UserId};
use serenity::async_trait;
use serenity::model::gateway::Ready;
use serenity::prelude::*;

struct Bot {
    registry: Arc<FeatureRegistry>,
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

    async fn message(&self, ctx: Context, message: Message) {
        self.registry
            .dispatch_message(ctx.http.clone(), &message)
            .await;
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

/// `HTTP_PORT` enables the HTTP server; `BASE_PATH` and `TRUSTED_PROXIES` configure it.
fn http_config() -> Option<HttpConfig> {
    let Ok(port) = env::var("HTTP_PORT") else {
        tracing::info!("HTTP_PORT not set; HTTP server disabled");
        return None;
    };
    let port = port.trim().parse().expect("Invalid HTTP_PORT");
    let base_path = env::var("BASE_PATH").unwrap_or_else(|_| "/jabot".to_string());
    let trusted_proxies = env::var("TRUSTED_PROXIES").unwrap_or_default();
    Some(HttpConfig {
        port,
        base_path: parse_base_path(&base_path).expect("Invalid BASE_PATH"),
        trusted_proxies: parse_trusted_proxies(&trusted_proxies).expect("Invalid TRUSTED_PROXIES"),
    })
}

/// The URLs `/tell` prints: `https://<DOMAIN><base path>/tell`, and the same under `LAN_HOST`.
/// Without `DOMAIN` (local dev), the bot's own port on localhost.
fn tell_config(http: &HttpConfig) -> TellConfig {
    let url_for = |host: String| format!("https://{host}{}/tell", http.base_path);
    let url = env::var("DOMAIN")
        .ok()
        .filter(|domain| !domain.is_empty())
        .map(url_for)
        .unwrap_or_else(|| format!("http://localhost:{}{}/tell", http.port, http.base_path));
    let lan_url = env::var("LAN_HOST")
        .ok()
        .filter(|host| !host.is_empty())
        .map(url_for);
    TellConfig { url, lan_url }
}

/// `BOT_OWNER_ID`: the Discord user id of the bot's admin, who is DMed about failed backups.
fn owner_id() -> Option<UserId> {
    let id = env::var("BOT_OWNER_ID")
        .ok()
        .filter(|id| !id.trim().is_empty())?;
    let id: u64 = id.trim().parse().expect("Invalid BOT_OWNER_ID");
    Some(UserId::new(id))
}

/// `RESTIC_REPOSITORY` enables daily backups; restic reads the rest of its settings itself.
fn backup_config() -> Option<BackupConfig> {
    let config = BackupConfig::from_env().expect("Invalid backup settings");
    if config.is_none() {
        tracing::info!("RESTIC_REPOSITORY not set; backups disabled");
    }
    config
}

fn backup(pool: &sqlx::SqlitePool, config: &BackupConfig) -> Backup {
    Backup::new(
        pool.clone(),
        Arc::new(ResticCli::new()),
        config.staging.clone(),
    )
}

/// `jabot backup`: one backup now, without connecting to Discord. Exits 0 on success.
async fn backup_once() -> ! {
    let Some(config) = backup_config() else {
        eprintln!("RESTIC_REPOSITORY is not set, so there's nowhere to back up to");
        std::process::exit(1);
    };
    let pool = connect_database().await;
    match backup(&pool, &config).run(false).await {
        Ok(report) => {
            println!(
                "Backed up {} bytes as snapshot {} ({} bytes new)",
                report.size,
                report.snapshot.short_id(),
                report.snapshot.data_added
            );
            std::process::exit(0);
        }
        Err(error) => {
            eprintln!("Backup failed: {error}");
            std::process::exit(1);
        }
    }
}

/// Resolves on Ctrl-C or SIGTERM (what `docker stop` sends).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for Ctrl-C");
    };
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("Failed to listen for SIGTERM");
    tokio::select! {
        () = ctrl_c => {}
        _ = sigterm.recv() => {}
    }
}

#[tokio::main]
async fn main() {
    // RUST_LOG overrides the default level, e.g. RUST_LOG=jabot=debug.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    match env::args().nth(1).as_deref() {
        None => {}
        Some("backup") => backup_once().await,
        Some(other) => {
            eprintln!("Unknown command {other:?}. Usage: jabot [backup]");
            std::process::exit(2);
        }
    }

    let token = get_discord_token().expect("Failed to get Discord token");
    let http_config = http_config();
    let backup_config = backup_config();
    let owner = owner_id();
    let pool = connect_database().await;

    let mut registry = FeatureRegistry::builder(Arc::new(SqliteUserRepo::new(pool.clone())))
        .register(SecretSanta::new(Arc::new(SqliteSecretSantaRepo::new(
            pool.clone(),
        ))))
        .register(Kv::new(Arc::new(SqliteKvRepo::new(pool.clone()))));
    // tell is only useful with the HTTP server it receives requests on.
    if let Some(http) = &http_config {
        registry = registry.register(Tell::new(
            Arc::new(SqliteTellRepo::new(pool.clone())),
            tell_config(http),
        ));
    }
    let registry = registry.build().expect("Feature registration conflict");
    let registry = Arc::new(registry);

    // Interactions (commands, components, modals) arrive without any gateway intents; features
    // that read messages ask for the intents they need.
    let intents = registry.intents();
    tracing::info!(?intents, "connecting to Discord");
    let mut client = Client::builder(&token, intents)
        .event_handler(Bot {
            registry: registry.clone(),
        })
        .await
        .expect("Err creating client");

    // HTTP handlers share the client's `Http`, and so its view of Discord's rate limits.
    let discord = Arc::new(SerenityDiscordApi::new(client.http.clone()));
    // Backups run on their own task: if it ever stopped, the bot should keep going.
    if let Some(config) = backup_config {
        let alerts = Alerts::new(discord.clone(), owner);
        tokio::spawn(run_scheduler(
            config.clone(),
            backup(&pool, &config),
            alerts,
        ));
    }
    let http_server = async {
        match &http_config {
            Some(config) => serve(registry.http_router(discord, config), config.port).await,
            None => std::future::pending().await,
        }
    };
    let shard_manager = client.shard_manager.clone();

    // Whichever stops first ends the process; under Docker, the restart policy brings it back.
    tokio::select! {
        result = client.start() => {
            if let Err(why) = result {
                tracing::error!("Client error: {:?}", why);
            }
        }
        result = http_server => {
            if let Err(error) = result {
                tracing::error!(%error, "HTTP server failed");
            }
        }
        () = shutdown_signal() => {
            tracing::info!("Shutting down");
            shard_manager.shutdown_all().await;
        }
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
