# syntax=docker/dockerfile:1

# ---- Build Stage ----
FROM rust:1.91-slim-bookworm@sha256:8514999d4786ef12efe89239e86b3d0a021b94b9d35108c8efe6c79ca7dc1a65 AS builder
WORKDIR /workspace

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY cli ./cli
COPY detective ./detective
COPY server ./server

RUN cargo build --locked --release --bin server

# ---- Runtime Stage ----
FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251 AS final

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 app \
    && useradd --uid 10001 \
        --gid app \
        --no-create-home \
        --home-dir /nonexistent \
        --shell /usr/sbin/nologin \
        app

WORKDIR /app
COPY --from=builder \
    --chown=10001:10001 \
    --chmod=0555 \
    /workspace/target/release/server \
    /app/server

USER 10001:10001

EXPOSE 3000

ENTRYPOINT ["/app/server"]
