# syntax=docker/dockerfile:1
# The API (hypha-host-axum) for a VPS (design note 0025). Built from the repository's
# root: `docker build -f docker/api.Dockerfile -t hypha-api .` The cache mounts keep
# downloaded crates and compiled dependencies between builds on the same machine, so a
# rebuild compiles only what changed.

FROM rust:1.99-trixie AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p hypha-host-axum \
    && cp target/release/hypha-host-axum /hypha-api

FROM debian:trixie-slim
# The OS trust store: Postgres with `sslmode=require` and HTTPS calls (S3, webhooks)
# are verified against it.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --no-create-home hypha
COPY --from=build /hypha-api /usr/local/bin/hypha-api
# `hypha.config.toml` is read from the working directory (mount it here).
WORKDIR /etc/hypha
USER hypha
ENV HYPHA_BIND=0.0.0.0:3000
EXPOSE 3000
STOPSIGNAL SIGTERM
CMD ["hypha-api"]
