# JABot Plan: `tell` Feature and Docker Deployment

**Status:** implemented on `feature/tell` (steps 1–8 and 10). Step 9, the cutover on the Pi, is still to do.

## Goals

1. **`tell`**: an HTTPS endpoint that DMs a Discord user. The main use is a `curl` at the end of
   a long-running task:
   ```sh
   cargo build --release; tell "build finished with exit code $?"
   ```
2. **Text first, attachments next.** Phase 1 sends text only. The request format and the
   Discord call are shaped so attachments (phase 2) only add code.
3. **Rate limited from the start**, per user and per client address.
4. **Docker deployment.** `jabot` runs under Docker Compose on the Pi and replaces the systemd
   unit and `deploy.sh`. It serves plain HTTP. TLS and certificates belong to the shared front
   proxy (see [Front proxy requirements](#front-proxy-requirements)).
5. **One URL prefix.** Every bot URL is `https://{host}/jabot/...`, because other web servers
   share the domain.

## Decisions

| Question | Decision | Consequence |
|---|---|---|
| URL | `POST https://{DOMAIN}/jabot/tell` (and the same path on `LAN_HOST`) | The bot serves everything under `BASE_PATH` (default `/jabot`), so the front proxy forwards `/jabot/*` without rewriting paths |
| Request body | JSON `{"token": "...", "message": "..."}`; both required | Sent with `curl -H 'Content-Type: application/json' -d ...`; not `--json`, which needs curl ≥ 7.82 (Ubuntu 22.04 has 7.81). Responses are JSON too. Phase 2 accepts the same fields as `multipart/form-data`, plus files. No user id and nothing in the query string, so tokens never appear in access logs |
| Authentication | **Per-user token** that identifies the user | One token per user, stored as-is, because it may be shown again. It stays the same until revoked |
| Getting the token | `/tell` shows a ready-to-run curl command, creating the token only if the user has none | Same token on every run |
| Revoking | A **Revoke token** button on the `/tell` reply | The next `/tell` creates a new token. A button left on an old reply can't revoke a newer token |
| Rate limiting | In memory, token buckets: per user, per client IP for failed auth, and global | `429` with `Retry-After`. The state resets on restart, which is fine at this scale |
| TLS | **The front proxy's job.** The bot has no proxy or certificates of its own | The Let's Encrypt and LAN certificates move to the front proxy, which is out of scope here |
| Client IP | `X-Forwarded-For` is used only when the connection comes from an address in `TRUSTED_PROXIES` | A client can't spoof its IP, even if the bot's port is reachable by more than the proxy |
| Proxy ↔ bot link | Plain HTTP, **provided the front proxy runs on the Pi** | If the proxy runs on another machine, that link needs encryption first (see [Encrypting the proxy ↔ bot link](#encrypting-the-proxy--bot-link)) |
| HTTP in the framework | Features can add routes, nested under `BASE_PATH/<namespace>` | One HTTP server for all features; namespaces already can't collide, so paths can't either |
| Port | `HTTP_PORT` | Unset → no HTTP server (convenient for local dev); logged at startup |
| Migrations | **Append-only from now on.** The bot is deployed on the Pi | `tell_tokens` goes in a new migration file; CLAUDE.md's "edit in place" note is removed |
| Building in Docker | sqlx offline mode (`.sqlx/` committed) | The image builds without a database; `cargo sqlx prepare` is rerun after query changes |

## Using It

`/tell` replies ephemerally with:
- the curl command, with the user's token filled in;
- a shell helper for `~/.bashrc`;
- the LAN variant, if `LAN_HOST` is set;
- a **Revoke token** button.

```sh
curl -fsS https://example.com/jabot/tell \
     -H 'Content-Type: application/json' \
     -d '{"token": "tell_9f2c...", "message": "Task finished"}'

# Shell helper: `long_task; tell "long_task exited with $?"`
# jq builds the JSON, so quotes, backslashes and newlines in the message are escaped.
tell() { jq -nc --arg token tell_9f2c... --arg message "${*:-done}" '$ARGS.named' |
         curl -fsS https://example.com/jabot/tell \
              -H 'Content-Type: application/json' --data-binary @-; }
```

- `-d` implies `POST`; the header makes the server read the body as JSON. `--data-binary @-`
  sends jq's output unchanged.
- A message pasted into the literal JSON has to be valid JSON, so `"` and `\` need escaping.
  The `jq` helper does that for any text, including shell variables. `/tell` shows both forms.

The first time `/tell` creates a token, the bot also sends a test DM. A user whose DMs are
closed then finds out at setup, through an ephemeral follow-up, rather than after a 3-hour
build.

Pressing **Revoke token**:
- deletes the token;
- replaces the reply with "Token revoked. Run /tell for a new one.", which removes the command
  and the button.

The button's custom id is `tell:revoke:<token id>`, so a button left on an old reply can't
revoke a newer token. Pressing it then says that token was already revoked.

### HTTP contract

| Case | Status | Body |
|---|---|---|
| DM sent | `204 No Content` | |
| Not `POST` | `405` | |
| `Content-Type` not `application/json` | `415` | `send the body as JSON, with Content-Type: application/json` |
| Body not valid JSON, or a field isn't a string | `400` | |
| `token` missing or unknown | `401` | `invalid token` |
| `message` missing or blank | `400` | the reason |
| `message` over 2000 characters (Discord's limit) | `413` | phase 2 sends long text as a `.txt` attachment instead |
| Body over 32 KiB | `413` | enforced before the body is read. 2000 characters of escaped JSON (`\uXXXX`, up to 12 bytes per character) can approach 24 KB |
| Rate limited | `429` | `Retry-After: <seconds>`, plus the same number in the body |
| Discord refuses the DM (error 50007: DMs closed or no shared server) | `422` | how to enable DMs from server members |
| Other Discord failure | `502` | logged |

- Error bodies are `{"error": "<reason>"}`, and a `429` adds `"retry_after": <seconds>`.
  With `-f`, curl prints only the status (`--fail-with-body` would print the body, but needs
  curl 7.76); without `-f` it prints the body but exits 0.
- Logs record user id, client IP and outcome, never the token or message text.

### Rate limits

Each limit is a token bucket. A bucket holds a number of requests, and one is added back every
refill period.

| Bucket | Key | Capacity | Refill | Charged when | Purpose |
|---|---|---|---|---|---|
| Failed auth | client IP | 10 | 1 per minute | a request has a missing or unknown token | Brute force and junk traffic. While empty, that IP gets `429` before any token lookup |
| User | user id | 5 | 1 per 12 s | a request is about to send a DM | Bursts of 5, then 5 a minute; plenty for task notifications |
| Global | none | 30 | 1 per second | a request is about to send a DM | Keeps the bot well inside Discord's limits, however many users there are |

- **Order:**
  1. IP bucket check.
  2. Token lookup; a failure charges the IP bucket and returns `401`.
  3. Message validation, so a malformed request doesn't use up the user's quota.
  4. User bucket.
  5. Global bucket.
  6. DM.
- **The limiter.** A small `RateLimiter<K>` (a `Mutex<HashMap<K, Bucket>>`) with
  `check(key, now: Instant) -> Result<(), Duration>`.
  - Taking `now` as a parameter makes tests deterministic.
  - Full buckets are pruned whenever the map grows past a threshold, so random IPs can't grow
    it without bound.
  - It lives in `features/tell/` until a second feature needs it, then moves to `framework/`.

### Client IP

`TRUSTED_PROXIES` is a comma-separated list of IPs or CIDR ranges, e.g. `172.30.0.1` or
`10.0.0.0/8`. The client IP is worked out as follows:

1. If the connection's peer address isn't trusted, that peer is the client, and any
   `X-Forwarded-For` header is ignored.
2. Otherwise, walk `X-Forwarded-For` from right to left, skipping trusted addresses. The first
   untrusted address is the client. Proxies append to the header, so entries a client forged
   sit to the left of this one and are never reached.
3. If the header runs out (or an entry isn't an IP) while the current address is still trusted, that last trusted address is the client: the peer if there's no header, otherwise the leftmost trusted hop.

With `TRUSTED_PROXIES` empty (the default), the header is never used. That is the right
setting until the front proxy exists, and for local dev.

## Framework Changes

### HTTP routes on `Feature`

```rust
// framework/feature.rs
/// HTTP routes this feature serves. The registry nests them under `BASE_PATH/<namespace>`,
/// so `.route("/", post(..))` in `tell` is served at `/jabot/tell`.
fn http_routes(&self, _ctx: HttpCtx) -> Option<axum::Router> {
    None
}
```

```rust
// framework/http.rs
/// Dependencies for HTTP handlers. Like `InteractionCtx`, minus the responder: the HTTP
/// response is the handler's return value.
#[derive(Clone)]
pub struct HttpCtx {
    pub discord: Arc<dyn DiscordApi>,
    pub users: Arc<dyn UserRepo>,
}

pub struct HttpConfig {
    pub port: u16,
    pub base_path: String,             // "/jabot"
    pub trusted_proxies: Vec<IpNet>,
}

/// Bind 0.0.0.0:`port` and serve until the process exits (main's `select!` handles SIGTERM).
/// Uses `into_make_service_with_connect_info::<SocketAddr>()` so handlers see the peer address.
pub async fn serve(router: Router, port: u16) -> io::Result<()>;

/// The client IP, following the rules in "Client IP".
pub fn client_ip(headers: &HeaderMap, peer: SocketAddr, trusted: &[IpNet]) -> IpAddr;
```

`FeatureRegistry::http_router(discord, &config)` builds one `Router`:
- each feature's routes, nested under `BASE_PATH/<namespace>`;
- `GET BASE_PATH/healthz` → `200 ok`, for the Docker health check;
- `DefaultBodyLimit` set to 32 KiB.

The trusted-proxy list is shared with handlers through a request extension, so `tell` doesn't
need it passed in.

### `main.rs`

- Wrap the registry in an `Arc`, so `Bot` and the HTTP router share it.
- After `Client::builder(..)`, build the router with
  `SerenityDiscordApi::new(client.http.clone())`. That reuses the client's rate limiter;
  a second `Http` would track Discord's limits separately.
- Run the gateway client and the HTTP server under `tokio::select!`, together with a
  SIGTERM/Ctrl-C handler. If either one stops, the process exits and Docker restarts it.
  Handling SIGTERM makes `docker stop` immediate instead of waiting 10 s for SIGKILL.
- New config, all optional:

  | Variable | Default | Use |
  |---|---|---|
  | `HTTP_PORT` | unset → no HTTP server | Port to listen on |
  | `BASE_PATH` | `/jabot` | Prefix for every route |
  | `TRUSTED_PROXIES` | empty | Addresses allowed to set `X-Forwarded-For` |
  | `DOMAIN` | | URL printed by `/tell` |
  | `LAN_HOST` | | Second URL printed by `/tell`, if the front proxy also serves a LAN name |

  `main.rs` builds `TellConfig { public_url, lan_url }` (e.g.
  `https://example.com/jabot/tell`) from these and passes it in. Features don't read the
  environment.

## `tell` Feature

```
src/features/tell/
├── mod.rs         # Tell: impl Feature (command, component, http_routes), routing only
├── commands.rs    # /tell
├── components.rs  # Revoke token button
├── custom_id.rs   # TellId: `tell:revoke:<token id>`
├── http.rs        # axum handler: extract → TellService::deliver → map TellError to a status
├── service.rs     # deliver(): rate limits, token lookup, message validation, send DM
├── rate_limit.rs  # RateLimiter<K>
├── token.rs       # generate: 32 random bytes → "tell_" + 64 hex characters
├── repo.rs        # trait TellRepo + SqliteTellRepo
├── model.rs       # TellError, TellConfig, TokenId
├── views.rs       # /tell reply, revoked reply, DM text
├── text.rs        # command name and user-facing strings
└── tests.rs
```

### Schema (new migration, e.g. `migrations/20261001000000_tell.sql`)

```sql
-- tell feature (feature-owned, prefixed tell_).
CREATE TABLE tell_tokens (
    id         INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, -- in the Revoke button's custom id
    user_id    INTEGER NOT NULL UNIQUE REFERENCES users(id),
    token      TEXT    NOT NULL UNIQUE,
    created_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

`AUTOINCREMENT` means ids are never reused, so an old button can never match a newer token.

```rust
pub struct StoredToken {
    pub id: TokenId,
    pub token: String,
    /// False if the user already had a token and `candidate` was discarded.
    pub created: bool,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait TellRepo: Send + Sync {
    /// The user's token, inserting `candidate` if they have none.
    async fn get_or_create(&self, user: UserId, candidate: String)
        -> Result<StoredToken, sqlx::Error>;
    /// Delete token `id` if it belongs to `user`. Returns whether a row was deleted.
    async fn revoke(&self, user: UserId, id: TokenId) -> Result<bool, sqlx::Error>;
    async fn user_for_token(&self, token: &str) -> Result<Option<UserId>, sqlx::Error>;
}
```

- `get_or_create` is a single `INSERT ... ON CONFLICT (user_id) DO NOTHING` followed by a
  `SELECT`, in one transaction. Two quick `/tell`s therefore return the same token.
- No `users.ensure` is needed on the HTTP path: a token exists only after `/tell`, and the
  registry has already ensured the user's row by then.
- Tokens are stored as plain text and can be shown again, by design. Anyone with a copy of the
  database (including backups) can use them.

### Request handling

`http.rs` stays thin. It extracts:
- the JSON body (`token: Option<String>`, `message: Option<String>`, so a missing field becomes
  a `401` or `400` rather than an extractor rejection);
- the client IP;
- `Instant::now()`.

It maps extractor rejections to the contract above, then calls:

```rust
impl TellService {
    pub async fn deliver(
        &self,
        discord: &dyn DiscordApi,
        client: IpAddr,
        request: TellRequest,
        now: Instant,
    ) -> Result<(), TellError>;
}
```

`TellError` has one variant per row of the HTTP contract, with `RateLimited(Duration)` for
`429`. It implements `axum::response::IntoResponse`.

To tell a closed DM from other failures, `DiscordApi::send_dm` needs a richer error.
Matching serenity's `HttpError::UnsuccessfulRequest` with code 50007 inside the serenity impl
and returning a `DmError::Closed` variant keeps serenity types out of the feature.

## Docker

### Files

```
Dockerfile
.dockerignore     # target/, .env, *.sqlite*, data/, secrets/
compose.yaml
.env.example      # HTTP_PORT, HTTP_PUBLISH, BASE_PATH, TRUSTED_PROXIES, DOMAIN, LAN_HOST, RUST_LOG
.sqlx/            # sqlx offline query data, committed
```

### Dockerfile (multi-stage)

```dockerfile
FROM rust:1.98-slim-trixie AS build
WORKDIR /src
COPY . .
ENV SQLX_OFFLINE=true
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked && cp target/release/jabot /jabot

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /data jabot \
 && mkdir /data && chown jabot:jabot /data
COPY --from=build /jabot /usr/local/bin/jabot
USER jabot
ENTRYPOINT ["/usr/local/bin/jabot"]
```

- Everything uses rustls, so the image needs no OpenSSL.
- The image builds on the Pi itself (arm64). The cache mounts make rebuilds incremental. If
  builds on the Pi are too slow, cross-build on the dev machine with
  `docker buildx build --platform linux/arm64` and ship the image; nothing else in this plan
  changes.

### compose.yaml

```yaml
services:
  jabot:
    build: .
    restart: unless-stopped
    init: true
    environment:
      DATABASE_URL: sqlite:///data/database.sqlite
      DISCORD_TOKEN_FILE: /run/secrets/discord_token
      HTTP_PORT: ${HTTP_PORT:-8080}
      BASE_PATH: ${BASE_PATH:-/jabot}
      TRUSTED_PROXIES: ${TRUSTED_PROXIES:-}
      DOMAIN: ${DOMAIN}
      LAN_HOST: ${LAN_HOST}
      RUST_LOG: ${RUST_LOG:-info}
    ports:
      # Loopback only by default: reachable by a proxy on the Pi, not from the network.
      - "${HTTP_PUBLISH:-127.0.0.1}:${HTTP_PORT:-8080}:${HTTP_PORT:-8080}"
    volumes:
      - ./data:/data
    secrets:
      - discord_token

networks:
  default:
    ipam:
      config:
        # A fixed subnet keeps the gateway at 172.30.0.1. Connections to the published port
        # from the Pi itself arrive from there, so that is the address for TRUSTED_PROXIES.
        - subnet: 172.30.0.0/24

secrets:
  discord_token:
    file: ./secrets/discord_token
```

- If the front proxy runs as a container on the Pi, a better setup is to drop `ports:`
  entirely. Both containers then join a shared external Docker network, and
  `TRUSTED_PROXIES` is the proxy container's address.
- `./data` and `./secrets/discord_token` are bind mounts. They must be readable and writable
  by uid 10001 on the host (`sudo chown -R 10001:10001 data secrets`), which is the same class
  of problem as the earlier `SQLITE_CANTOPEN` error.

## Front Proxy Requirements

The front proxy is out of scope, but the bot depends on it to:

- **Terminate TLS** for `DOMAIN` (Let's Encrypt), and for the LAN name if LAN clients connect
  by one (internal CA or self-signed). Routing by path means the proxy has to decrypt the
  request anyway, so TLS can't pass through to the bot.
- **Forward `/jabot/*` unchanged** to the bot's port. The bot owns the prefix, so no path
  rewriting is needed.
- **Append the client address to `X-Forwarded-For`.** Most proxies do this by default. Its own
  connecting address then goes in the bot's `TRUSTED_PROXIES`. The bot logs each request's
  peer address at `debug` level, which shows exactly what to put there.
- **Limit request bodies** to about 64 KB for `/jabot/*`. Phase 2 raises this for uploads.

### Encrypting the proxy ↔ bot link

Each request carries a token and a message, so the link between the proxy and the bot matters.

- **Proxy on the Pi** (the plan's assumption): the traffic never leaves the machine, either
  over loopback or a Docker network. Plain HTTP is fine, and the bot needs no certificate.
- **Proxy on another machine:** the traffic crosses the LAN in plain text. Anyone who can
  observe the LAN (a compromised device, the Wi-Fi) could read tokens and messages. Encrypt
  the link before relying on this:
  - **Native TLS in the bot.** Add `TLS_CERT_FILE` and `TLS_KEY_FILE`, served with
    `axum-server`'s rustls support. A self-signed certificate is enough when the proxy is
    configured to trust exactly that certificate. This is about 30 lines plus config.
  - **A WireGuard tunnel** between the two machines, if they already share one. That needs no
    bot changes.

Until the front proxy exists, the bot is reachable only at
`http://127.0.0.1:8080/jabot/tell` on the Pi. That's enough for testing.

## Testing

- **token.rs**: the format (`tell_` + 64 hex characters), and a different token on each call.
- **rate_limit.rs** (driven by fake `Instant`s):
  - A bucket allows its capacity, then refuses with the correct wait.
  - It refills one request per period.
  - Keys are independent.
  - Pruning drops full buckets only.
- **repo.rs** (`#[sqlx::test]`):
  - `get_or_create` returns the existing token and discards the candidate.
  - `revoke` deletes only a matching id and user.
  - A revoked token's id isn't reused.
  - `user_for_token` finds the owner.
- **service.rs** (`MockTellRepo`, `MockDiscordApi`):
  - A valid token sends exactly the message.
  - A missing or unknown token gives `401`, charges the IP bucket, and sends no DM. After
    10 failures the next request gets `429` without a repo call.
  - A missing, blank or 2001-character message gives `400`/`413` without charging the user
    bucket.
  - The 6th DM in a burst gives `429` with a `Retry-After`.
  - A closed DM gives `422`.
- **http.rs**: `tower::ServiceExt::oneshot` against the real router with mocked dependencies
  (dev-dependency: `tower` with `util`), checking:
  - status codes, `415` for a non-JSON body, the 32 KiB limit, and `Retry-After`;
  - that the routes are served under `BASE_PATH`.
- **client_ip**:
  - An untrusted peer's header is ignored.
  - A trusted peer's header is used.
  - With a chain, the rightmost untrusted entry wins.
  - Forged left-hand entries are ignored.
  - An empty `TRUSTED_PROXIES` never uses the header.
- **commands.rs / components.rs**:
  - `/tell` with no token creates one, replies ephemerally with the curl command and the
    button, then sends the test DM (with the follow-up when the DM fails).
  - `/tell` with an existing token shows the same token and sends no test DM.
  - Revoke deletes by token id and updates the message; a stale id reports "already revoked".
- **framework**: `http_router` nests by `BASE_PATH` and namespace, and serves `healthz`.
- **Manual**, on the Pi against `http://127.0.0.1:8080/jabot/tell`:
  - The curl command from `/tell`, with the URL swapped.
  - A burst of 6 requests, which should end in a `429`.
  - A bad token.
  - Revoke, then the old curl command, which should give a `401`.
  - Once the front proxy exists, repeat through it, and check the logged client IP is the
    real one.

## Cargo.toml Changes

```toml
[dependencies]
axum = "0.8"   # default features include `json`
ipnet = "2"    # TRUSTED_PROXIES ranges
tokio = { version = "1.0", features = ["macros", "rt-multi-thread", "signal"] }

[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
```

`rand` is already a dependency. Hex encoding is a two-line helper. The rate limiter is
hand-written, so there's no `governor` dependency and tests drive it with plain `Instant`s.

## Steps

Each step compiles and passes `cargo test`, and gets its own commit.

1. **HTTP plumbing.** Add `HttpCtx`, `HttpConfig`, `http.rs` (`serve`, `client_ip`),
   `Feature::http_routes`, `http_router` with `healthz`, and the `HTTP_PORT`/`BASE_PATH`/
   `TRUSTED_PROXIES` config, plus `select!` with signal handling in `main.rs`. No feature uses
   it yet.
2. **`DiscordApi` DM errors.** Add `DmError` with a `Closed` variant; Secret Santa's callers
   keep treating every failure the same way.
3. **`tell` storage.** Add the migration, `TellRepo` and token generation.
4. **`tell` command.** Add `/tell`, the Revoke button, `TellConfig`, and register `Tell` in
   `main.rs`.
5. **Rate limiter.** Add `RateLimiter<K>` and its tests.
6. **`tell` HTTP.** Add `TellService::deliver` with the three buckets, the handler and the
   error mapping.
7. **sqlx offline data.** Run `cargo sqlx prepare -- --all-targets` and commit `.sqlx/`.
   - The local sqlx-cli is 0.9 but the library is 0.8. That works: the CLI only runs the build,
     and the 0.8 macros write the query files.
   - CLAUDE.md: rerun `prepare` after changing a query.
8. **Docker.** Add `Dockerfile`, `.dockerignore`, `compose.yaml` and `.env.example`, and
   smoke-test locally.
9. **Cutover on the Pi.**
   1. Clone the repo into the deployment directory, then copy `.env.example` to `.env` and
      fill it in.
   2. Copy `/opt/jabot/database.sqlite` to `./data/` and the token to
      `./secrets/discord_token`, then `chown -R 10001:10001 data secrets`.
   3. Stop and disable the systemd unit: `sudo systemctl disable --now jabot`.
   4. Run `docker compose up -d --build`, check the logs, and run the manual tests against
      loopback.
10. **Deploy script and docs.**
    - Replace the body of `deploy.sh` with `git pull --ff-only && docker compose up -d --build`,
      followed by a status/log check.
    - CLAUDE.md: the architecture tree, the env vars, the deploy line, append-only migrations,
      and an "Adding a feature" note about `http_routes`.
    - README: how to use `/tell`, and the front proxy requirements.

## Phase 2: Attachments

- Also accept `multipart/form-data` (JSON stays the text-only format), with the same `token`
  and `message` fields.
  - Every file part becomes an attachment (axum's `multipart` feature).
  - `message` becomes optional when there's at least one file.
  ```sh
  curl ... -F token=tell_... -F "message=build failed" -F "file=@build.log"
  ```
- Add `DiscordApi::send_dm_files(user, content, Vec<DmFile>)`, where
  `DmFile { name, bytes }` maps to `CreateAttachment`.
- Limits: 10 attachments, and Discord's upload limit (currently 10 MiB per message for bots
  without boosts). Raise the body limit for multipart in the bot and in the front proxy.
- Text over 2000 characters is sent as `message.txt` instead of returning `413`.
- The `/tell` reply gains a file example.

## Deferred

- **Native TLS in the bot**, only if the front proxy ends up on another machine (see
  [Encrypting the proxy ↔ bot link](#encrypting-the-proxy--bot-link)).
- **Several named tokens per user** (e.g. one per machine). Needs a `name` column, dropping
  `UNIQUE (user_id)`, and one Revoke button per token.
- **Telling other users**, e.g. a channel or a friend who opted in. This would be a `to`
  field in the body, checked against an opt-in table.
