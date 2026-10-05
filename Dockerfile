# Build: sqlx checks queries against the committed .sqlx data, so no database is needed.
FROM rust:1.98-slim-trixie AS build
WORKDIR /src
COPY . .
ENV SQLX_OFFLINE=true
# The cache mounts keep the registry and build artifacts between builds, so rebuilds are
# incremental. target/ isn't part of the image, hence the copy out of it.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked && cp target/release/jabot /jabot

FROM debian:trixie-slim
# restic uploads the daily database backups (src/backup); trixie ships 0.18.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates restic \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /data jabot \
 && mkdir /data && chown jabot:jabot /data
COPY --from=build /jabot /usr/local/bin/jabot
USER jabot
WORKDIR /data
ENTRYPOINT ["/usr/local/bin/jabot"]
