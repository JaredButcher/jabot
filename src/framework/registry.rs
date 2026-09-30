use std::collections::HashMap;
use std::sync::Arc;

use serenity::all::{
    CreateCommand, CreateInteractionResponse, CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage, Http, Interaction, InteractionId, UserId,
};

use super::context::InteractionCtx;
use super::discord::{SerenityDiscordApi, SerenityResponder};
use super::error::FeatureError;
use super::feature::Feature;
use super::request::{CommandRequest, ComponentRequest, ModalRequest};
use super::users::UserRepo;

const GENERIC_ERROR: &str = "Something went wrong. Please try again later.";

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
            other => tracing::debug!(kind = ?other.kind(), "ignoring interaction"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::testing::{any_users, content, ctx, followup_content};
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
}
