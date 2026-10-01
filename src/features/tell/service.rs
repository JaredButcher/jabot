//! Delivering a tell request: rate limits, token lookup, message checks, then the DM.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serenity::all::UserId;

use super::model::{MAX_MESSAGE_CHARS, TellError, TellRequest};
use super::rate_limit::RateLimiter;
use super::repo::TellRepo;
use crate::framework::{DiscordApi, DmError};

/// Requests with a missing or unknown token, per client IP: 10, then 1 a minute. While an IP
/// is over the limit, its requests are refused before any token lookup.
const FAILED_AUTH: (u32, Duration) = (10, Duration::from_secs(60));
/// DMs per user: bursts of 5, then 5 a minute.
const PER_USER: (u32, Duration) = (5, Duration::from_secs(12));
/// DMs across all users, to stay well inside Discord's limits.
const GLOBAL: (u32, Duration) = (30, Duration::from_secs(1));

pub struct TellService {
    repo: Arc<dyn TellRepo>,
    failed_auth: RateLimiter<IpAddr>,
    per_user: RateLimiter<UserId>,
    global: RateLimiter<()>,
}

impl TellService {
    pub fn new(repo: Arc<dyn TellRepo>) -> Self {
        Self {
            repo,
            failed_auth: RateLimiter::new(FAILED_AUTH.0, FAILED_AUTH.1),
            per_user: RateLimiter::new(PER_USER.0, PER_USER.1),
            global: RateLimiter::new(GLOBAL.0, GLOBAL.1),
        }
    }

    /// DM the token's owner the request's message. The message is checked before the user's
    /// limits are charged, so a malformed request doesn't use up their quota.
    pub async fn deliver(
        &self,
        discord: &dyn DiscordApi,
        client: IpAddr,
        request: TellRequest,
        now: Instant,
    ) -> Result<(), TellError> {
        self.failed_auth
            .peek(&client, now)
            .map_err(|wait| rate_limited(wait, "failed auth", client))?;

        let owner = match &request.token {
            Some(token) => self
                .repo
                .user_for_token(token)
                .await
                .map_err(TellError::Repo)?,
            None => None,
        };
        let Some(user) = owner else {
            // peek just allowed this IP, so the charge succeeds; its result adds nothing.
            let _ = self.failed_auth.check(client, now);
            tracing::info!(%client, "tell request with invalid token");
            return Err(TellError::InvalidToken);
        };

        let message = request
            .message
            .filter(|message| !message.trim().is_empty())
            .ok_or(TellError::MissingMessage)?;
        if message.chars().count() > MAX_MESSAGE_CHARS {
            return Err(TellError::MessageTooLong);
        }

        self.per_user
            .check(user, now)
            .map_err(|wait| rate_limited(wait, "per user", client))?;
        self.global
            .check((), now)
            .map_err(|wait| rate_limited(wait, "global", client))?;

        discord
            .send_dm(user, message)
            .await
            .map_err(|error| match error {
                DmError::Closed => TellError::DmsClosed,
                DmError::Discord(error) => TellError::Discord(error),
            })?;
        tracing::info!(%user, %client, "tell DM sent");
        Ok(())
    }
}

fn rate_limited(wait: Duration, limit: &str, client: IpAddr) -> TellError {
    tracing::warn!(limit, %client, ?wait, "tell request rate limited");
    TellError::RateLimited(wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::tell::repo::MockTellRepo;
    use crate::framework::MockDiscordApi;
    use mockall::predicate::eq;

    const TOKEN: &str = "tell_abc";

    fn client() -> IpAddr {
        "1.1.1.1".parse().unwrap()
    }

    /// Token "tell_abc" belongs to user 1, and "u<n>" to user n; anything else is unknown.
    fn repo() -> MockTellRepo {
        let mut repo = MockTellRepo::new();
        repo.expect_user_for_token().returning(|token| {
            Ok(match token {
                TOKEN => Some(UserId::new(1)),
                _ => token
                    .strip_prefix('u')
                    .map(|n| UserId::new(n.parse().unwrap())),
            })
        });
        repo
    }

    fn dms_ok() -> MockDiscordApi {
        let mut discord = MockDiscordApi::new();
        discord.expect_send_dm().returning(|_, _| Ok(()));
        discord
    }

    fn request(token: Option<&str>, message: Option<&str>) -> TellRequest {
        TellRequest {
            token: token.map(Into::into),
            message: message.map(Into::into),
        }
    }

    fn valid() -> TellRequest {
        request(Some(TOKEN), Some("done"))
    }

    #[tokio::test]
    async fn sends_the_message_to_the_token_owner() {
        let mut discord = MockDiscordApi::new();
        discord
            .expect_send_dm()
            .with(eq(UserId::new(1)), eq("build done\nexit 0".to_string()))
            .times(1)
            .returning(|_, _| Ok(()));
        let service = TellService::new(Arc::new(repo()));

        let request = request(Some(TOKEN), Some("build done\nexit 0"));
        let result = service
            .deliver(&discord, client(), request, Instant::now())
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn missing_or_unknown_token_is_refused_without_a_dm() {
        let mut discord = MockDiscordApi::new();
        discord.expect_send_dm().times(0);
        let service = TellService::new(Arc::new(repo()));
        let now = Instant::now();

        for token in [None, Some("tell_wrong"), Some("")] {
            let result = service
                .deliver(&discord, client(), request(token, Some("hi")), now)
                .await;
            assert!(matches!(result, Err(TellError::InvalidToken)), "{token:?}");
        }
    }

    #[tokio::test]
    async fn repeated_failures_lock_out_the_ip_before_any_lookup() {
        // 10 lookups for the locked-out client, 1 for the other client; none for the 11th.
        let mut repo = MockTellRepo::new();
        repo.expect_user_for_token()
            .times(11)
            .returning(|_| Ok(None));
        let service = TellService::new(Arc::new(repo));
        let discord = MockDiscordApi::new();
        let now = Instant::now();
        let bad = || request(Some("tell_wrong"), Some("hi"));

        for _ in 0..10 {
            let result = service.deliver(&discord, client(), bad(), now).await;
            assert!(matches!(result, Err(TellError::InvalidToken)));
        }
        let result = service.deliver(&discord, client(), bad(), now).await;
        assert!(matches!(result, Err(TellError::RateLimited(wait)) if wait == FAILED_AUTH.1));

        let other = "2.2.2.2".parse().unwrap();
        let result = service.deliver(&discord, other, bad(), now).await;
        assert!(matches!(result, Err(TellError::InvalidToken)));
    }

    #[tokio::test]
    async fn bad_messages_are_refused_without_using_the_quota() {
        let service = TellService::new(Arc::new(repo()));
        let discord = dms_ok();
        let now = Instant::now();
        let too_long = "é".repeat(MAX_MESSAGE_CHARS + 1);

        for (message, expected) in [
            (None, "missing"),
            (Some("  \n "), "missing"),
            (Some(too_long.as_str()), "too long"),
        ] {
            let result = service
                .deliver(&discord, client(), request(Some(TOKEN), message), now)
                .await;
            match (expected, result) {
                ("missing", Err(TellError::MissingMessage)) => {}
                ("too long", Err(TellError::MessageTooLong)) => {}
                (_, other) => panic!("{message:?}: {other:?}"),
            }
        }

        // The full burst of 5 is still available.
        let longest = "é".repeat(MAX_MESSAGE_CHARS);
        for _ in 0..4 {
            assert!(
                service
                    .deliver(&discord, client(), valid(), now)
                    .await
                    .is_ok()
            );
        }
        let result = service
            .deliver(
                &discord,
                client(),
                request(Some(TOKEN), Some(&longest)),
                now,
            )
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn sixth_dm_in_a_burst_is_rate_limited() {
        let service = TellService::new(Arc::new(repo()));
        let discord = dms_ok();
        let now = Instant::now();

        for _ in 0..5 {
            assert!(
                service
                    .deliver(&discord, client(), valid(), now)
                    .await
                    .is_ok()
            );
        }
        let result = service.deliver(&discord, client(), valid(), now).await;
        assert!(matches!(result, Err(TellError::RateLimited(wait)) if wait == PER_USER.1));

        let later = now + PER_USER.1;
        assert!(
            service
                .deliver(&discord, client(), valid(), later)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn global_limit_spans_users() {
        let service = TellService::new(Arc::new(repo()));
        let discord = dms_ok();
        let now = Instant::now();
        let from_user = |n: u32| request(Some(&format!("u{n}")), Some("hi"));

        for n in 1..=GLOBAL.0 {
            assert!(
                service
                    .deliver(&discord, client(), from_user(n), now)
                    .await
                    .is_ok()
            );
        }
        let result = service
            .deliver(&discord, client(), from_user(GLOBAL.0 + 1), now)
            .await;
        assert!(matches!(result, Err(TellError::RateLimited(_))));
    }

    #[tokio::test]
    async fn closed_dms_are_reported() {
        let mut discord = MockDiscordApi::new();
        discord
            .expect_send_dm()
            .returning(|_, _| Err(DmError::Closed));
        let service = TellService::new(Arc::new(repo()));

        let result = service
            .deliver(&discord, client(), valid(), Instant::now())
            .await;

        assert!(matches!(result, Err(TellError::DmsClosed)));
    }
}
