//! The tell endpoint: `POST <base path>/tell` with `{"token": "...", "message": "..."}`.
//! Thin: extract, call `TellService::deliver`, and let `TellError` pick the status.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};

use super::model::{TellError, TellRequest};
use super::service::TellService;
use crate::framework::{ClientIp, DiscordApi};

#[derive(Clone)]
struct AppState {
    service: Arc<TellService>,
    discord: Arc<dyn DiscordApi>,
}

pub fn routes(service: Arc<TellService>, discord: Arc<dyn DiscordApi>) -> Router {
    Router::new()
        .route("/", post(tell))
        .with_state(AppState { service, discord })
}

async fn tell(
    State(state): State<AppState>,
    ClientIp(client): ClientIp,
    body: Result<Json<TellRequest>, JsonRejection>,
) -> Result<StatusCode, TellError> {
    let Json(request) = body.map_err(rejection_error)?;
    state
        .service
        .deliver(state.discord.as_ref(), client, request, Instant::now())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn rejection_error(rejection: JsonRejection) -> TellError {
    match rejection {
        JsonRejection::MissingJsonContentType(_) => TellError::UnsupportedMediaType,
        rejection if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => TellError::BodyTooLarge,
        _ => TellError::InvalidBody,
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::body::Body;
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::{HeaderMap, Request, header};
    use serenity::all::UserId;
    use tower::ServiceExt;

    use crate::features::tell::repo::MockTellRepo;
    use crate::features::tell::{Tell, TellConfig, text};
    use crate::framework::{
        FeatureRegistry, HttpConfig, MockDiscordApi, MockUserRepo, parse_trusted_proxies,
    };

    const URL: &str = "/jabot/tell";
    const PROXY: [u8; 4] = [172, 30, 0, 1];

    struct Response {
        status: u16,
        headers: HeaderMap,
        body: serde_json::Value,
    }

    /// The full HTTP stack (registry router, body limit, client IP), as reached through a
    /// proxy at 172.30.0.1. Token "tell_abc" belongs to user 1.
    fn router(trusted_proxies: &str) -> axum::Router {
        let mut repo = MockTellRepo::new();
        repo.expect_user_for_token()
            .returning(|token| Ok((token == "tell_abc").then(|| UserId::new(1))));
        let mut discord = MockDiscordApi::new();
        discord.expect_send_dm().returning(|_, _| Ok(()));
        let config = TellConfig {
            url: String::new(),
            lan_url: None,
        };
        let registry = FeatureRegistry::builder(std::sync::Arc::new(MockUserRepo::new()))
            .register(Tell::new(std::sync::Arc::new(repo), config))
            .build()
            .unwrap();
        let http = HttpConfig {
            port: 0,
            base_path: "/jabot".into(),
            trusted_proxies: parse_trusted_proxies(trusted_proxies).unwrap(),
        };
        registry
            .http_router(std::sync::Arc::new(discord), &http)
            .layer(MockConnectInfo(SocketAddr::from((PROXY, 5000))))
    }

    async fn send(router: &axum::Router, request: Request<Body>) -> Response {
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        Response {
            status,
            headers,
            body,
        }
    }

    fn post_json(body: impl Into<Body>, forwarded_for: Option<&str>) -> Request<Body> {
        let mut request = Request::post(URL).header(header::CONTENT_TYPE, "application/json");
        if let Some(ip) = forwarded_for {
            request = request.header("x-forwarded-for", ip);
        }
        request.body(body.into()).unwrap()
    }

    fn json(token: &str, message: &str) -> String {
        serde_json::json!({ "token": token, "message": message }).to_string()
    }

    #[tokio::test]
    async fn valid_request_gets_no_content() {
        let response = send(&router(""), post_json(json("tell_abc", "done"), None)).await;
        assert_eq!(response.status, 204);
    }

    #[tokio::test]
    async fn malformed_requests_get_their_status_and_reason() {
        let router = router("");
        let form = Request::post(URL)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("token=tell_abc&message=done"))
            .unwrap();
        let cases = [
            (form, 415, text::ERR_CONTENT_TYPE),
            (post_json("{not json", None), 400, text::ERR_BODY),
            (post_json(r#"{"token": 5}"#, None), 400, text::ERR_BODY),
            (
                post_json(json("tell_nope", "x"), None),
                401,
                text::ERR_TOKEN,
            ),
            (
                post_json(json("tell_abc", " "), None),
                400,
                text::ERR_MESSAGE_MISSING,
            ),
            (
                post_json(json("tell_abc", &"x".repeat(33 * 1024)), None),
                413,
                text::ERR_BODY_TOO_LARGE,
            ),
        ];
        for (request, status, reason) in cases {
            let response = send(&router, request).await;
            assert_eq!(response.status, status, "{reason}");
            assert_eq!(response.body["error"], reason);
        }
    }

    #[tokio::test]
    async fn get_is_not_allowed() {
        let request = Request::get(URL).body(Body::empty()).unwrap();
        assert_eq!(send(&router(""), request).await.status, 405);
    }

    #[tokio::test]
    async fn rate_limit_sets_retry_after() {
        let router = router("");
        for _ in 0..5 {
            send(&router, post_json(json("tell_abc", "hi"), None)).await;
        }

        let response = send(&router, post_json(json("tell_abc", "hi"), None)).await;

        assert_eq!(response.status, 429);
        let retry_after: u64 = response.headers[header::RETRY_AFTER]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((1..=12).contains(&retry_after), "{retry_after}");
        assert_eq!(response.body["retry_after"], retry_after);
    }

    #[tokio::test]
    async fn failed_auth_is_counted_per_forwarded_client() {
        let router = router("172.30.0.1");
        for _ in 0..10 {
            let response = send(&router, post_json(json("bad", "x"), Some("1.1.1.1"))).await;
            assert_eq!(response.status, 401);
        }

        let locked_out = send(&router, post_json(json("bad", "x"), Some("1.1.1.1"))).await;
        let other_client = send(&router, post_json(json("bad", "x"), Some("2.2.2.2"))).await;

        assert_eq!(locked_out.status, 429);
        assert_eq!(other_client.status, 401);
    }

    #[tokio::test]
    async fn untrusted_proxy_cannot_pick_the_client_ip() {
        // 172.30.0.1 isn't trusted, so every request counts against it, whatever the header.
        let router = router("");
        for n in 0..10 {
            let ip = format!("1.1.1.{n}");
            send(&router, post_json(json("bad", "x"), Some(&ip))).await;
        }

        let response = send(&router, post_json(json("bad", "x"), Some("9.9.9.9"))).await;

        assert_eq!(response.status, 429);
    }
}
