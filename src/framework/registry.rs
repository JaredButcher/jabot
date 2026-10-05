use std::collections::HashMap;
use std::sync::Arc;

use serenity::all::{
    ChannelId, CreateAutocompleteResponse, CreateCommand, CreateInteractionResponse,
    CreateInteractionResponseFollowup, CreateInteractionResponseMessage, CreateMessage,
    GatewayIntents, Http, Interaction, InteractionId, Message, UserId,
};

use axum::Router;
use axum::extract::{DefaultBodyLimit, Extension};

use super::context::{AutocompleteCtx, InteractionCtx, MessageCtx};
use super::discord::{DiscordApi, Responder, SerenityDiscordApi, SerenityResponder};
use super::error::FeatureError;
use super::feature::Feature;
use super::http::{HttpConfig, HttpCtx, TrustedProxies, log_request};
use super::request::{
    AutocompleteRequest, CommandRequest, ComponentRequest, MessageRequest, ModalRequest,
};
use super::users::UserRepo;

const GENERIC_ERROR: &str = "Something went wrong. Please try again later.";

/// Largest request body any route accepts. A feature can raise it for its own routes.
const BODY_LIMIT: usize = 32 * 1024;

/// Discord shows at most this many autocomplete suggestions.
const MAX_SUGGESTIONS: usize = 25;
/// Discord's limit on an autocomplete suggestion's label and value, in characters.
const MAX_SUGGESTION_CHARS: usize = 100;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("command /{command} is registered by both {first} and {second}")]
    DuplicateCommand {
        command: String,
        first: &'static str,
        second: &'static str,
    },
    #[error("custom-id namespace \"{namespace}\" is registered by both {first} and {second}")]
    DuplicateNamespace {
        namespace: &'static str,
        first: &'static str,
        second: &'static str,
    },
    #[error("namespace \"{namespace}\" of {feature} must be non-empty and contain no ':'")]
    InvalidNamespace {
        namespace: &'static str,
        feature: &'static str,
    },
}

/// Routes Discord interactions to the feature that owns them.
pub struct FeatureRegistry {
    features: Vec<Arc<dyn Feature>>,
    by_command: HashMap<String, usize>,
    by_namespace: HashMap<&'static str, usize>,
    users: Arc<dyn UserRepo>,
}

pub struct RegistryBuilder {
    features: Vec<Arc<dyn Feature>>,
    users: Arc<dyn UserRepo>,
}

impl RegistryBuilder {
    pub fn register(mut self, feature: impl Feature + 'static) -> Self {
        self.features.push(Arc::new(feature));
        self
    }

    /// Fails on a duplicate command name or namespace, so a clash is caught at startup.
    pub fn build(self) -> Result<FeatureRegistry, RegistryError> {
        let mut by_command: HashMap<String, usize> = HashMap::new();
        let mut by_namespace: HashMap<&'static str, usize> = HashMap::new();

        for (index, feature) in self.features.iter().enumerate() {
            let namespace = feature.namespace();
            if namespace.is_empty() || namespace.contains(':') {
                return Err(RegistryError::InvalidNamespace {
                    namespace,
                    feature: feature.name(),
                });
            }
            if let Some(&other) = by_namespace.get(namespace) {
                return Err(RegistryError::DuplicateNamespace {
                    namespace,
                    first: self.features[other].name(),
                    second: feature.name(),
                });
            }
            by_namespace.insert(namespace, index);

            for command in feature.commands().iter().filter_map(command_name) {
                if let Some(&other) = by_command.get(&command) {
                    return Err(RegistryError::DuplicateCommand {
                        command,
                        first: self.features[other].name(),
                        second: feature.name(),
                    });
                }
                by_command.insert(command, index);
            }
        }

        Ok(FeatureRegistry {
            features: self.features,
            by_command,
            by_namespace,
            users: self.users,
        })
    }
}

/// `CreateCommand` doesn't expose its name, but it serializes to the Discord API shape.
fn command_name(command: &CreateCommand) -> Option<String> {
    serde_json::to_value(command)
        .ok()?
        .get("name")?
        .as_str()
        .map(str::to_string)
}

/// The part of a custom id before the first ':' names the owning feature.
fn namespace_of(custom_id: &str) -> &str {
    custom_id.split(':').next().unwrap_or_default()
}

impl FeatureRegistry {
    /// `users` backs the shared `users` table; every user who triggers an interaction gets a
    /// row before their feature handler runs.
    pub fn builder(users: Arc<dyn UserRepo>) -> RegistryBuilder {
        RegistryBuilder {
            features: Vec::new(),
            users,
        }
    }

    /// Every feature's slash commands, for `Command::set_global_commands`.
    pub fn commands(&self) -> Vec<CreateCommand> {
        self.features.iter().flat_map(|f| f.commands()).collect()
    }

    /// Gateway intents to connect with: the union of every feature's.
    pub fn intents(&self) -> GatewayIntents {
        self.features
            .iter()
            .fold(GatewayIntents::empty(), |all, f| all | f.intents())
    }

    /// One router for every feature's HTTP routes, each nested under
    /// `<base path>/<namespace>`, plus `<base path>/healthz`.
    pub fn http_router(&self, discord: Arc<dyn DiscordApi>, config: &HttpConfig) -> Router {
        let ctx = HttpCtx {
            discord,
            users: self.users.clone(),
        };
        let mut routes = Router::new().route("/healthz", axum::routing::get(|| async { "ok" }));
        for feature in &self.features {
            if let Some(feature_routes) = feature.http_routes(ctx.clone()) {
                routes = routes.nest(&format!("/{}", feature.namespace()), feature_routes);
            }
        }
        let routes = if config.base_path.is_empty() {
            routes
        } else {
            Router::new().nest(&config.base_path, routes)
        };
        routes
            .layer(DefaultBodyLimit::max(BODY_LIMIT))
            .layer(Extension(TrustedProxies(
                config.trusted_proxies.clone().into(),
            )))
            .layer(axum::middleware::from_fn(log_request))
    }

    pub async fn dispatch(&self, http: Arc<Http>, interaction: Interaction) {
        match interaction {
            Interaction::Command(command) => {
                let ctx = self.context(&http, command.id, &command.token);
                self.route_command(&ctx, CommandRequest::from(&command))
                    .await;
            }
            Interaction::Component(component) => {
                let ctx = self.context(&http, component.id, &component.token);
                self.route_component(&ctx, ComponentRequest::from(&component))
                    .await;
            }
            Interaction::Modal(modal) => {
                let ctx = self.context(&http, modal.id, &modal.token);
                self.route_modal(&ctx, ModalRequest::from(&modal)).await;
            }
            Interaction::Autocomplete(command) => {
                let responder = SerenityResponder::new(http, command.id, command.token.clone());
                let ctx = AutocompleteCtx {
                    users: self.users.clone(),
                };
                self.route_autocomplete(&responder, &ctx, AutocompleteRequest::from(&command))
                    .await;
            }
            other => tracing::debug!(kind = ?other.kind(), "ignoring interaction"),
        }
    }

    /// No `users.ensure` here: autocomplete fires every few keystrokes and writes nothing.
    async fn route_autocomplete(
        &self,
        responder: &dyn Responder,
        ctx: &AutocompleteCtx,
        req: AutocompleteRequest,
    ) {
        let (feature, suggestions) = match self.by_command.get(&req.command) {
            Some(&index) => {
                let feature = &self.features[index];
                (feature.name(), feature.on_autocomplete(ctx, req).await)
            }
            None => {
                tracing::warn!(command = %req.command, "no feature handles autocomplete");
                ("registry", Ok(vec![]))
            }
        };
        let suggestions = suggestions.unwrap_or_else(|error| {
            tracing::error!(feature, %error, "autocomplete handler failed");
            vec![]
        });
        let truncate = |s: String| s.chars().take(MAX_SUGGESTION_CHARS).collect::<String>();
        let response = suggestions.into_iter().take(MAX_SUGGESTIONS).fold(
            CreateAutocompleteResponse::new(),
            |response, (label, value)| response.add_string_choice(truncate(label), truncate(value)),
        );
        if let Err(error) = responder
            .respond(CreateInteractionResponse::Autocomplete(response))
            .await
        {
            tracing::error!(feature, %error, "could not send autocomplete suggestions");
        }
    }

    /// Pass a user's message to every feature whose intents cover where it was sent.
    pub async fn dispatch_message(&self, http: Arc<Http>, message: &Message) {
        let Some(req) = MessageRequest::from_message(message) else {
            return;
        };
        let ctx = MessageCtx {
            discord: Arc::new(SerenityDiscordApi::new(http)),
            users: self.users.clone(),
        };
        self.route_message(&ctx, req).await;
    }

    /// No `users.ensure` here: that would be a database write for every message.
    async fn route_message(&self, ctx: &MessageCtx, req: MessageRequest) {
        let wanted = if req.guild.is_some() {
            GatewayIntents::GUILD_MESSAGES
        } else {
            GatewayIntents::DIRECT_MESSAGES
        };
        for feature in self
            .features
            .iter()
            .filter(|f| f.intents().contains(wanted))
        {
            let channel = req.channel;
            let result = feature.on_message(ctx, req.clone()).await;
            finish_message(ctx, feature.name(), channel, result).await;
        }
    }

    fn context(&self, http: &Arc<Http>, id: InteractionId, token: &str) -> InteractionCtx {
        InteractionCtx {
            responder: Arc::new(SerenityResponder::new(http.clone(), id, token.to_string())),
            discord: Arc::new(SerenityDiscordApi::new(http.clone())),
            users: self.users.clone(),
        }
    }

    async fn route_command(&self, ctx: &InteractionCtx, req: CommandRequest) {
        let Some(&index) = self.by_command.get(&req.command) else {
            tracing::warn!(command = %req.command, "no feature handles command");
            let error = FeatureError::user(format!("Unknown command /{}", req.command));
            return finish(ctx, "registry", Err(error)).await;
        };
        let feature = &self.features[index];
        let result = match ensure_user(ctx, req.user).await {
            Ok(()) => feature.on_command(ctx, req).await,
            Err(error) => Err(error),
        };
        finish(ctx, feature.name(), result).await;
    }

    async fn route_component(&self, ctx: &InteractionCtx, req: ComponentRequest) {
        let Some(feature) = self.feature_for(&req.custom_id) else {
            tracing::warn!(custom_id = %req.custom_id, "no feature handles component");
            return finish(
                ctx,
                "registry",
                Err(FeatureError::user("Unknown component")),
            )
            .await;
        };
        let result = match ensure_user(ctx, req.user).await {
            Ok(()) => feature.on_component(ctx, req).await,
            Err(error) => Err(error),
        };
        finish(ctx, feature.name(), result).await;
    }

    async fn route_modal(&self, ctx: &InteractionCtx, req: ModalRequest) {
        let Some(feature) = self.feature_for(&req.custom_id) else {
            tracing::warn!(custom_id = %req.custom_id, "no feature handles modal");
            return finish(ctx, "registry", Err(FeatureError::user("Unknown form"))).await;
        };
        let result = match ensure_user(ctx, req.user).await {
            Ok(()) => feature.on_modal(ctx, req).await,
            Err(error) => Err(error),
        };
        finish(ctx, feature.name(), result).await;
    }

    fn feature_for(&self, custom_id: &str) -> Option<&Arc<dyn Feature>> {
        let index = *self.by_namespace.get(namespace_of(custom_id))?;
        Some(&self.features[index])
    }
}

/// Give the interacting user a `users` row, so feature tables can reference them.
async fn ensure_user(ctx: &InteractionCtx, user: UserId) -> Result<(), FeatureError> {
    Ok(ctx.users.ensure(&[user]).await?)
}

/// Turn a handler error into an ephemeral reply, using a followup if the handler already
/// responded.
async fn finish(ctx: &InteractionCtx, feature: &str, result: Result<(), FeatureError>) {
    let content = match result {
        Ok(()) => return,
        Err(FeatureError::User(message)) => message,
        Err(FeatureError::Internal(error)) => {
            tracing::error!(feature, %error, "interaction handler failed");
            GENERIC_ERROR.to_string()
        }
    };

    let sent = if ctx.responder.has_responded() {
        let followup = CreateInteractionResponseFollowup::new()
            .content(content)
            .ephemeral(true);
        ctx.responder.followup(followup).await
    } else {
        let message = CreateInteractionResponseMessage::new()
            .content(content)
            .ephemeral(true);
        ctx.responder
            .respond(CreateInteractionResponse::Message(message))
            .await
    };
    if let Err(error) = sent {
        tracing::error!(feature, %error, "could not send error reply");
    }
}

/// Turn a message handler's error into a reply in the message's channel.
async fn finish_message(
    ctx: &MessageCtx,
    feature: &str,
    channel: ChannelId,
    result: Result<(), FeatureError>,
) {
    let content = match result {
        Ok(()) => return,
        Err(FeatureError::User(message)) => message,
        Err(FeatureError::Internal(error)) => {
            tracing::error!(feature, %error, "message handler failed");
            GENERIC_ERROR.to_string()
        }
    };
    let reply = CreateMessage::new().content(content);
    if let Err(error) = ctx.discord.send_message(channel, reply).await {
        tracing::error!(feature, %error, "could not send error reply");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::testing::{any_users, content, ctx, followup_content, message_ctx};
    use crate::framework::{MockDiscordApi, MockFeature, MockResponder, MockUserRepo};
    use mockall::Sequence;

    fn builder() -> RegistryBuilder {
        FeatureRegistry::builder(Arc::new(MockUserRepo::new()))
    }

    fn feature(
        name: &'static str,
        namespace: &'static str,
        commands: &[&'static str],
    ) -> MockFeature {
        let commands = commands.to_vec();
        let mut feature = MockFeature::new();
        feature.expect_name().return_const(name);
        feature.expect_namespace().return_const(namespace);
        feature
            .expect_commands()
            .returning(move || commands.iter().map(|c| CreateCommand::new(*c)).collect());
        feature
    }

    fn replies_once(content_is: &'static str) -> MockResponder {
        let mut responder = MockResponder::new();
        responder.expect_has_responded().return_const(false);
        responder
            .expect_respond()
            .withf(move |r| content(r) == content_is)
            .times(1)
            .returning(|_| Ok(()));
        responder
    }

    #[tokio::test]
    async fn routes_component_by_namespace() {
        let mut ss = feature("Secret Santa", "ss", &["ss"]);
        ss.expect_on_component()
            .withf(|_, req| req.custom_id == "ss:btn:start:5")
            .times(1)
            .returning(|_, _| Ok(()));
        let mut other = feature("Other", "ot", &["ot"]);
        other.expect_on_component().times(0);
        let registry = builder().register(ss).register(other).build().unwrap();

        let ctx = ctx(MockResponder::new(), MockDiscordApi::new(), any_users());
        registry
            .route_component(
                &ctx,
                ComponentRequest::button(UserId::new(1), "ss:btn:start:5"),
            )
            .await;
    }

    #[tokio::test]
    async fn routes_command_by_name() {
        let mut ss = feature("Secret Santa", "ss", &["ss"]);
        ss.expect_on_command()
            .withf(|_, req| req.command == "ss")
            .times(1)
            .returning(|_, _| Ok(()));
        let registry = builder().register(ss).build().unwrap();

        let ctx = ctx(MockResponder::new(), MockDiscordApi::new(), any_users());
        registry
            .route_command(&ctx, CommandRequest::new(UserId::new(1), "ss"))
            .await;
    }

    #[tokio::test]
    async fn routes_modal_by_namespace() {
        let mut ss = feature("Secret Santa", "ss", &[]);
        ss.expect_on_modal().times(1).returning(|_, _| Ok(()));
        let registry = builder().register(ss).build().unwrap();

        let ctx = ctx(MockResponder::new(), MockDiscordApi::new(), any_users());
        let req = ModalRequest::new(UserId::new(1), "ss:create:modal", []);
        registry.route_modal(&ctx, req).await;
    }

    #[tokio::test]
    async fn unknown_namespace_gets_ephemeral_reply() {
        let registry = builder()
            .register(feature("Secret Santa", "ss", &[]))
            .build()
            .unwrap();

        let ctx = ctx(
            replies_once("Unknown component"),
            MockDiscordApi::new(),
            any_users(),
        );
        registry
            .route_component(&ctx, ComponentRequest::button(UserId::new(1), "zz:btn"))
            .await;
    }

    #[test]
    fn rejects_duplicate_namespace() {
        let result = builder()
            .register(feature("A", "ss", &["a"]))
            .register(feature("B", "ss", &["b"]))
            .build();

        assert_eq!(
            result.err(),
            Some(RegistryError::DuplicateNamespace {
                namespace: "ss",
                first: "A",
                second: "B"
            })
        );
    }

    #[test]
    fn rejects_duplicate_command() {
        let result = builder()
            .register(feature("A", "a", &["ss"]))
            .register(feature("B", "b", &["ss"]))
            .build();

        assert_eq!(
            result.err(),
            Some(RegistryError::DuplicateCommand {
                command: "ss".into(),
                first: "A",
                second: "B"
            })
        );
    }

    #[test]
    fn rejects_namespace_with_separator() {
        let result = builder().register(feature("A", "a:b", &[])).build();

        assert!(matches!(
            result,
            Err(RegistryError::InvalidNamespace { .. })
        ));
    }

    #[test]
    fn collects_commands_from_all_features() {
        let registry = builder()
            .register(feature("A", "a", &["one", "two"]))
            .register(feature("B", "b", &["three"]))
            .build()
            .unwrap();

        let names: Vec<_> = registry
            .commands()
            .iter()
            .filter_map(command_name)
            .collect();
        assert_eq!(names, ["one", "two", "three"]);
    }

    #[tokio::test]
    async fn user_error_becomes_ephemeral_reply() {
        let mut ss = feature("Secret Santa", "ss", &[]);
        ss.expect_on_component()
            .returning(|_, _| Err(FeatureError::user("Event not found")));
        let registry = builder().register(ss).build().unwrap();

        let ctx = ctx(
            replies_once("Event not found"),
            MockDiscordApi::new(),
            any_users(),
        );
        registry
            .route_component(&ctx, ComponentRequest::button(UserId::new(1), "ss:x"))
            .await;
    }

    #[tokio::test]
    async fn internal_error_is_hidden_from_user() {
        let mut ss = feature("Secret Santa", "ss", &[]);
        ss.expect_on_component()
            .returning(|_, _| Err(FeatureError::internal("db is on fire")));
        let registry = builder().register(ss).build().unwrap();

        let ctx = ctx(
            replies_once(GENERIC_ERROR),
            MockDiscordApi::new(),
            any_users(),
        );
        registry
            .route_component(&ctx, ComponentRequest::button(UserId::new(1), "ss:x"))
            .await;
    }

    #[tokio::test]
    async fn error_after_response_uses_followup() {
        let mut ss = feature("Secret Santa", "ss", &[]);
        ss.expect_on_component()
            .returning(|_, _| Err(FeatureError::user("Late failure")));
        let registry = builder().register(ss).build().unwrap();

        let mut responder = MockResponder::new();
        responder.expect_has_responded().return_const(true);
        responder.expect_respond().times(0);
        responder
            .expect_followup()
            .withf(|f| followup_content(f) == "Late failure")
            .times(1)
            .returning(|_| Ok(()));
        let ctx = ctx(responder, MockDiscordApi::new(), any_users());
        registry
            .route_component(&ctx, ComponentRequest::button(UserId::new(1), "ss:x"))
            .await;
    }

    #[tokio::test]
    async fn ensures_user_row_before_dispatch() {
        let mut seq = Sequence::new();
        let mut users = MockUserRepo::new();
        users
            .expect_ensure()
            .withf(|ids| ids == [UserId::new(7)])
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(()));
        let mut ss = feature("Secret Santa", "ss", &["ss"]);
        ss.expect_on_command()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        let registry = builder().register(ss).build().unwrap();

        let ctx = ctx(MockResponder::new(), MockDiscordApi::new(), users);
        registry
            .route_command(&ctx, CommandRequest::new(UserId::new(7), "ss"))
            .await;
    }

    #[tokio::test]
    async fn failing_to_ensure_user_skips_handler() {
        let mut users = MockUserRepo::new();
        users
            .expect_ensure()
            .returning(|_| Err(sqlx::Error::PoolTimedOut));
        let mut ss = feature("Secret Santa", "ss", &[]);
        ss.expect_on_component().times(0);
        let registry = builder().register(ss).build().unwrap();

        let ctx = ctx(replies_once(GENERIC_ERROR), MockDiscordApi::new(), users);
        registry
            .route_component(&ctx, ComponentRequest::button(UserId::new(1), "ss:x"))
            .await;
    }

    fn listening(
        name: &'static str,
        namespace: &'static str,
        intents: GatewayIntents,
    ) -> MockFeature {
        let mut feature = feature(name, namespace, &[]);
        feature.expect_intents().return_const(intents);
        feature
    }

    fn dm(content: &str) -> MessageRequest {
        MessageRequest::dm(UserId::new(1), ChannelId::new(5), content)
    }

    fn message_content(message: &CreateMessage) -> String {
        let json = serde_json::to_value(message).unwrap();
        json["content"].as_str().unwrap_or_default().to_string()
    }

    fn autocomplete_ctx() -> AutocompleteCtx {
        AutocompleteCtx {
            users: Arc::new(MockUserRepo::new()),
        }
    }

    fn key_typed(partial: &str) -> AutocompleteRequest {
        AutocompleteRequest::new(UserId::new(1), "k", Some("get"), "key", partial)
    }

    /// A responder expecting one autocomplete response with exactly these choices.
    fn suggests(expected: Vec<(String, String)>) -> MockResponder {
        let mut responder = MockResponder::new();
        responder
            .expect_respond()
            .times(1)
            .returning(move |response| {
                let json = serde_json::to_value(&response).unwrap();
                assert_eq!(json["type"], 8);
                let choices: Vec<(String, String)> = json["data"]["choices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| {
                        let s = |k: &str| c[k].as_str().unwrap().to_string();
                        (s("name"), s("value"))
                    })
                    .collect();
                assert_eq!(choices, expected);
                Ok(())
            });
        responder
    }

    #[tokio::test]
    async fn routes_autocomplete_by_command_name() {
        let mut kv = feature("kv", "kv", &["k"]);
        kv.expect_on_autocomplete()
            .withf(|_, req| req.partial == "pa" && req.focused == "key")
            .times(1)
            .returning(|_, _| Ok(vec![("pasta".into(), "pasta".into())]));
        let registry = builder().register(kv).build().unwrap();

        let responder = suggests(vec![("pasta".into(), "pasta".into())]);
        registry
            .route_autocomplete(&responder, &autocomplete_ctx(), key_typed("pa"))
            .await;
    }

    #[tokio::test]
    async fn autocomplete_is_truncated_to_discord_limits() {
        let long = "x".repeat(150);
        let mut kv = feature("kv", "kv", &["k"]);
        let many: Vec<(String, String)> = (0..30)
            .map(|i| (format!("{i}{long}"), i.to_string()))
            .collect();
        kv.expect_on_autocomplete()
            .returning(move |_, _| Ok(many.clone()));
        let registry = builder().register(kv).build().unwrap();

        let expected = (0..25)
            .map(|i| {
                let label: String = format!("{i}{long}").chars().take(100).collect();
                (label, i.to_string())
            })
            .collect();
        registry
            .route_autocomplete(&suggests(expected), &autocomplete_ctx(), key_typed(""))
            .await;
    }

    #[tokio::test]
    async fn autocomplete_error_suggests_nothing() {
        let mut kv = feature("kv", "kv", &["k"]);
        kv.expect_on_autocomplete()
            .returning(|_, _| Err(FeatureError::user("nope")));
        let registry = builder().register(kv).build().unwrap();

        registry
            .route_autocomplete(&suggests(vec![]), &autocomplete_ctx(), key_typed("pa"))
            .await;
    }

    #[tokio::test]
    async fn autocomplete_for_unknown_command_suggests_nothing() {
        let registry = builder().build().unwrap();

        registry
            .route_autocomplete(&suggests(vec![]), &autocomplete_ctx(), key_typed("pa"))
            .await;
    }

    #[test]
    fn intents_are_the_union_over_features() {
        let registry = builder()
            .register(listening("A", "a", GatewayIntents::DIRECT_MESSAGES))
            .register(listening("B", "b", GatewayIntents::GUILD_MESSAGES))
            .register(listening("C", "c", GatewayIntents::empty()))
            .build()
            .unwrap();

        assert_eq!(
            registry.intents(),
            GatewayIntents::DIRECT_MESSAGES | GatewayIntents::GUILD_MESSAGES
        );
    }

    #[tokio::test]
    async fn messages_reach_only_features_with_matching_intents() {
        let mut dms = listening("DMs", "dm", GatewayIntents::DIRECT_MESSAGES);
        dms.expect_on_message()
            .withf(|_, msg| msg.content == "hi")
            .times(1)
            .returning(|_, _| Ok(()));
        let mut guilds = listening("Guilds", "g", GatewayIntents::GUILD_MESSAGES);
        guilds.expect_on_message().times(0);
        let mut none = listening("None", "n", GatewayIntents::empty());
        none.expect_on_message().times(0);
        let registry = builder()
            .register(dms)
            .register(guilds)
            .register(none)
            .build()
            .unwrap();

        // No `ensure` expectation: messages don't touch the users table.
        registry
            .route_message(&message_ctx(MockDiscordApi::new()), dm("hi"))
            .await;
    }

    #[tokio::test]
    async fn guild_messages_need_guild_intents() {
        let mut dms = listening("DMs", "dm", GatewayIntents::DIRECT_MESSAGES);
        dms.expect_on_message().times(0);
        let registry = builder().register(dms).build().unwrap();

        let mut msg = dm("hi");
        msg.guild = Some(serenity::all::GuildId::new(3));
        registry
            .route_message(&message_ctx(MockDiscordApi::new()), msg)
            .await;
    }

    #[tokio::test]
    async fn message_errors_are_replied_in_the_channel() {
        for (error, reply) in [
            (FeatureError::user("Too long"), "Too long"),
            (FeatureError::internal("db is on fire"), GENERIC_ERROR),
        ] {
            let mut dms = listening("DMs", "dm", GatewayIntents::DIRECT_MESSAGES);
            let error = std::sync::Mutex::new(Some(error));
            dms.expect_on_message()
                .returning(move |_, _| Err(error.lock().unwrap().take().unwrap()));
            let registry = builder().register(dms).build().unwrap();
            let mut discord = MockDiscordApi::new();
            discord
                .expect_send_message()
                .withf(move |channel, message| {
                    *channel == ChannelId::new(5) && message_content(message) == reply
                })
                .times(1)
                .returning(|_, _| Ok(()));

            registry
                .route_message(&message_ctx(discord), dm("hi"))
                .await;
        }
    }

    async fn get(router: &Router, path: &str) -> (u16, String) {
        use tower::ServiceExt;
        let request = axum::http::Request::get(path)
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    fn http_config(base_path: &str) -> HttpConfig {
        HttpConfig {
            port: 0,
            base_path: base_path.into(),
            trusted_proxies: vec![],
        }
    }

    fn with_route(namespace: &'static str) -> MockFeature {
        let mut feature = feature("F", namespace, &[]);
        feature.expect_http_routes().returning(|_| {
            Some(Router::new().route("/", axum::routing::get(|| async { "feature" })))
        });
        feature
    }

    #[tokio::test]
    async fn http_routes_nest_under_base_path_and_namespace() {
        let mut without_routes = feature("None", "nr", &[]);
        without_routes.expect_http_routes().returning(|_| None);
        let registry = builder()
            .register(with_route("tell"))
            .register(without_routes)
            .build()
            .unwrap();
        let router = registry.http_router(Arc::new(MockDiscordApi::new()), &http_config("/jabot"));

        assert_eq!(get(&router, "/jabot/tell").await, (200, "feature".into()));
        assert_eq!(get(&router, "/jabot/healthz").await, (200, "ok".into()));
        assert_eq!(get(&router, "/tell").await.0, 404);
        assert_eq!(get(&router, "/jabot/nr").await.0, 404);
    }

    #[tokio::test]
    async fn empty_base_path_serves_from_root() {
        let registry = builder().register(with_route("tell")).build().unwrap();
        let router = registry.http_router(Arc::new(MockDiscordApi::new()), &http_config(""));

        assert_eq!(get(&router, "/tell").await, (200, "feature".into()));
        assert_eq!(get(&router, "/healthz").await, (200, "ok".into()));
    }
}
