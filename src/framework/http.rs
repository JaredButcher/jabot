//! HTTP entry point. Features add routes with `Feature::http_routes`; the registry nests them
//! under `<base path>/<namespace>`, and `serve` runs the result next to the Discord client.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::extract::{ConnectInfo, FromRequestParts, Request};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use ipnet::IpNet;

use super::discord::DiscordApi;
use super::users::UserRepo;

/// Dependencies for HTTP handlers. Like `InteractionCtx`, minus the responder: the HTTP
/// response is the handler's return value.
#[derive(Clone)]
pub struct HttpCtx {
    pub discord: Arc<dyn DiscordApi>,
    pub users: Arc<dyn UserRepo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpConfig {
    pub port: u16,
    /// Prefix of every route, e.g. "/jabot". Empty serves from the root.
    pub base_path: String,
    /// Peers allowed to set `X-Forwarded-For`.
    pub trusted_proxies: Vec<IpNet>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("base path {0:?} must be empty or start with '/'")]
    BasePath(String),
    #[error("trusted proxy {0:?} is not an IP address or CIDR range")]
    TrustedProxy(String),
}

/// Normalize a base path: "/jabot/" → "/jabot", "/" → "".
pub fn parse_base_path(raw: &str) -> Result<String, ConfigError> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed.starts_with('/') {
        Ok(trimmed.to_string())
    } else {
        Err(ConfigError::BasePath(raw.to_string()))
    }
}

/// Parse a comma-separated list of IPs and CIDR ranges, e.g. "172.30.0.1, 10.0.0.0/8".
pub fn parse_trusted_proxies(raw: &str) -> Result<Vec<IpNet>, ConfigError> {
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            entry
                .parse::<IpNet>()
                .or_else(|_| entry.parse::<IpAddr>().map(IpNet::from))
                .map_err(|_| ConfigError::TrustedProxy(entry.to_string()))
        })
        .collect()
}

/// The trusted proxy list, handed to extractors as a request extension.
#[derive(Debug, Clone, Default)]
pub struct TrustedProxies(pub Arc<[IpNet]>);

/// The address of the client that made the request.
///
/// Starting from the connection's peer, each trusted hop is replaced by the address it put in
/// `X-Forwarded-For`, walking the header right to left. Proxies append to the header, so
/// entries a client wrote itself are only reached through an untrusted hop, and never used.
pub fn client_ip(headers: &HeaderMap, peer: IpAddr, trusted: &[IpNet]) -> IpAddr {
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|net| net.contains(ip));
    let forwarded: Vec<&str> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .collect();

    let mut client = peer;
    for entry in forwarded.iter().rev() {
        if !is_trusted(&client) {
            break;
        }
        match parse_forwarded(entry) {
            Some(ip) => client = ip,
            None => break,
        }
    }
    client
}

/// An `X-Forwarded-For` entry: a bare IP, or one with a port ("1.2.3.4:80", "[::1]:80").
fn parse_forwarded(entry: &str) -> Option<IpAddr> {
    entry
        .parse::<IpAddr>()
        .ok()
        .or_else(|| entry.parse::<SocketAddr>().ok().map(|addr| addr.ip()))
}

/// Extractor for [`client_ip`]. Needs the server to provide `ConnectInfo<SocketAddr>`, which
/// `serve` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Some(ConnectInfo(peer)) = parts.extensions.get::<ConnectInfo<SocketAddr>>() else {
            tracing::error!("ClientIp used on a server without connect info");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        };
        let trusted = parts
            .extensions
            .get::<TrustedProxies>()
            .cloned()
            .unwrap_or_default();
        Ok(ClientIp(client_ip(&parts.headers, peer.ip(), &trusted.0)))
    }
}

/// Logs every request with its peer address, which is what `TRUSTED_PROXIES` has to match.
pub(super) async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    let response = next.run(request).await;
    tracing::debug!(%method, path, ?peer, status = response.status().as_u16(), "HTTP request");
    response
}

/// Bind 0.0.0.0:`port` and serve until the process exits.
pub async fn serve(router: Router, port: u16) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await?;
    tracing::info!(port, "HTTP server listening");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn forwarded(values: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append("x-forwarded-for", HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    fn proxies(raw: &str) -> Vec<IpNet> {
        parse_trusted_proxies(raw).unwrap()
    }

    #[test]
    fn untrusted_peer_header_is_ignored() {
        let headers = forwarded(&["1.1.1.1"]);
        assert_eq!(
            client_ip(&headers, ip("9.9.9.9"), &proxies("172.30.0.1")),
            ip("9.9.9.9")
        );
    }

    #[test]
    fn empty_trust_list_never_uses_header() {
        let headers = forwarded(&["1.1.1.1"]);
        assert_eq!(client_ip(&headers, ip("172.30.0.1"), &[]), ip("172.30.0.1"));
    }

    #[test]
    fn trusted_peer_header_is_used() {
        let headers = forwarded(&["1.1.1.1"]);
        assert_eq!(
            client_ip(&headers, ip("172.30.0.1"), &proxies("172.30.0.1")),
            ip("1.1.1.1")
        );
    }

    #[test]
    fn rightmost_untrusted_entry_wins_and_forged_entries_are_ignored() {
        // The client forged 6.6.6.6; the front proxy appended the real 1.1.1.1, then a second
        // trusted hop appended the front proxy's address.
        let headers = forwarded(&["6.6.6.6, 1.1.1.1", "10.0.0.5"]);
        assert_eq!(
            client_ip(
                &headers,
                ip("172.30.0.1"),
                &proxies("172.30.0.1, 10.0.0.0/8")
            ),
            ip("1.1.1.1")
        );
    }

    #[test]
    fn no_header_or_garbage_falls_back_to_last_trusted_hop() {
        let trusted = proxies("172.30.0.1");
        assert_eq!(
            client_ip(&HeaderMap::new(), ip("172.30.0.1"), &trusted),
            ip("172.30.0.1")
        );
        assert_eq!(
            client_ip(&forwarded(&["nonsense"]), ip("172.30.0.1"), &trusted),
            ip("172.30.0.1")
        );
    }

    #[test]
    fn forwarded_entries_may_carry_ports() {
        let headers = forwarded(&["[2001:db8::1]:443"]);
        assert_eq!(
            client_ip(&headers, ip("172.30.0.1"), &proxies("172.30.0.1")),
            ip("2001:db8::1")
        );
    }

    #[test]
    fn parses_trusted_proxies() {
        assert_eq!(
            proxies(" 172.30.0.1 , 10.0.0.0/8,,"),
            vec![
                "172.30.0.1/32".parse::<IpNet>().unwrap(),
                "10.0.0.0/8".parse().unwrap()
            ]
        );
        assert_eq!(proxies(""), vec![]);
        assert_eq!(
            parse_trusted_proxies("10.0.0.0/8, nope"),
            Err(ConfigError::TrustedProxy("nope".into()))
        );
    }

    #[test]
    fn normalizes_base_path() {
        assert_eq!(parse_base_path("/jabot"), Ok("/jabot".into()));
        assert_eq!(parse_base_path("/jabot/"), Ok("/jabot".into()));
        assert_eq!(parse_base_path("/"), Ok("".into()));
        assert_eq!(parse_base_path(""), Ok("".into()));
        assert_eq!(
            parse_base_path("jabot"),
            Err(ConfigError::BasePath("jabot".into()))
        );
    }
}
