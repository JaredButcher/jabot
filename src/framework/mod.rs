//! Shared plumbing for bot features: routing, request types, and mockable Discord access.
//! Nothing in here knows about any specific feature.

mod context;
mod discord;
mod error;
mod feature;
mod registry;
mod request;
mod users;

pub use context::InteractionCtx;
pub use discord::{DiscordApi, DiscordError, Responder, SerenityDiscordApi, SerenityResponder};
pub use error::FeatureError;
pub use feature::Feature;
pub use registry::{FeatureRegistry, RegistryBuilder, RegistryError};
pub use request::{CommandRequest, ComponentKind, ComponentRequest, ModalRequest, Options};
pub use users::{SqliteUserRepo, UserRepo};

#[cfg(test)]
pub use discord::{MockDiscordApi, MockResponder};
#[cfg(test)]
pub use feature::MockFeature;
#[cfg(test)]
pub use users::MockUserRepo;

/// Helpers for feature tests.
#[cfg(test)]
pub mod testing {
    use std::sync::Arc;

    use serenity::all::{CreateInteractionResponse, CreateInteractionResponseFollowup};

    use super::{InteractionCtx, MockDiscordApi, MockResponder, MockUserRepo};

    pub fn ctx(
        responder: MockResponder,
        discord: MockDiscordApi,
        users: MockUserRepo,
    ) -> InteractionCtx {
        InteractionCtx {
            responder: Arc::new(responder),
            discord: Arc::new(discord),
            users: Arc::new(users),
        }
    }

    /// A `UserRepo` that accepts any `ensure` call.
    pub fn any_users() -> MockUserRepo {
        let mut users = MockUserRepo::new();
        users.expect_ensure().returning(|_| Ok(()));
        users
    }

    /// The response as Discord would receive it.
    pub fn json(response: &CreateInteractionResponse) -> serde_json::Value {
        serde_json::to_value(response).unwrap()
    }

    /// Message content of a response, or "" if it has none.
    pub fn content(response: &CreateInteractionResponse) -> String {
        json(response)["data"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    pub fn is_ephemeral(response: &CreateInteractionResponse) -> bool {
        json(response)["data"]["flags"]
            .as_u64()
            .is_some_and(|flags| flags & 64 != 0)
    }

    pub fn followup_content(followup: &CreateInteractionResponseFollowup) -> String {
        let json = serde_json::to_value(followup).unwrap();
        json["content"].as_str().unwrap_or_default().to_string()
    }
}
