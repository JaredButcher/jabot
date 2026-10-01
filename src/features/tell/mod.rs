//! tell: users get a token with `/tell`, then `POST <base path>/tell` with it to DM themselves,
//! e.g. when a long-running task finishes.

mod commands;
mod components;
mod custom_id;
mod http;
mod model;
mod rate_limit;
mod repo;
mod service;
#[cfg(test)]
mod tests;
mod text;
mod token;
mod views;

use std::sync::Arc;

use async_trait::async_trait;
use serenity::all::CreateCommand;

use crate::framework::{
    CommandRequest, ComponentRequest, Feature, FeatureError, HttpCtx, InteractionCtx,
};
use custom_id::TellId;
pub use repo::{SqliteTellRepo, TellRepo};
use service::TellService;

/// Where the bot is reachable, for the commands `/tell` prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TellConfig {
    /// Full URL of the endpoint, e.g. `https://example.com/jabot/tell`.
    pub url: String,
    /// The same endpoint under a LAN host name, if there is one.
    pub lan_url: Option<String>,
}

pub struct Tell {
    repo: Arc<dyn TellRepo>,
    config: TellConfig,
    /// Shared with the HTTP handler; holds the rate limits.
    service: Arc<TellService>,
}

impl Tell {
    pub fn new(repo: Arc<dyn TellRepo>, config: TellConfig) -> Self {
        Self {
            service: Arc::new(TellService::new(repo.clone())),
            repo,
            config,
        }
    }
}

#[async_trait]
impl Feature for Tell {
    fn name(&self) -> &'static str {
        "tell"
    }

    fn namespace(&self) -> &'static str {
        custom_id::NAMESPACE
    }

    fn commands(&self) -> Vec<CreateCommand> {
        vec![commands::tell_command()]
    }

    fn http_routes(&self, ctx: HttpCtx) -> Option<axum::Router> {
        Some(http::routes(self.service.clone(), ctx.discord))
    }

    async fn on_command(
        &self,
        ctx: &InteractionCtx,
        req: CommandRequest,
    ) -> Result<(), FeatureError> {
        commands::tell(self, ctx, &req).await
    }

    async fn on_component(
        &self,
        ctx: &InteractionCtx,
        req: ComponentRequest,
    ) -> Result<(), FeatureError> {
        let TellId::Revoke(id) = req
            .custom_id
            .parse::<TellId>()
            .map_err(FeatureError::internal)?;
        components::revoke(self, ctx, id, req.user).await
    }
}
