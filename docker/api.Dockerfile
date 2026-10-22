# syntax=docker/dockerfile:1 The API (hypha-host-axum) for a VPS. Built from the
# repository's root: `docker build -f docker/api.Dockerfile -t hypha-api .` The cache
# mounts keep downloaded crates and compiled dependencies between builds on the same
# machine, so a rebuild compiles only what changed.

FROM rust:1.99-trixie AS build
# Spores, as `hypha deploy` passes them: this repository's own as the host's features
# (`SPORE_FEATURES=spore-stripe`), or a deployment's host crate
# (`HOST_CRATE=.hypha-build/<deployment>/host`, written by the CLI) when it has others.
ARG SPORE_FEATURES=""
ARG HOST_CRATE=""
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    if [ -n "$HOST_CRATE" ]; then \
        # Not --locked: its lock is the checkout's, plus the deployment's own spores.
        CARGO_TARGET_DIR=/src/target cargo build --release \
            --manifest-path "$HOST_CRATE/Cargo.toml" \
        && cp target/release/hypha-deployment /hypha-api; \
    else \
        cargo build --release --locked -p hypha-host-axum \
            ${SPORE_FEATURES:+--features "$SPORE_FEATURES"} \
        && cp target/release/hypha-host-axum /hypha-api; \
    fi

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
