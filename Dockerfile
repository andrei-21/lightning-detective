# syntax=docker/dockerfile:1

# ---- Build Stage ----
FROM rust:1.98-slim-bookworm@sha256:ff521445a372125ed4f76e1453a1f8098f2d05332d1601d30db1c1f62757e730 AS builder
WORKDIR /workspace

COPY --link Cargo.toml Cargo.lock ./
COPY --link cli ./cli
COPY --link detective ./detective
COPY --link server ./server

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/workspace/target,sharing=locked \
    cargo build --locked --release --package server --bin server \
    && cp /workspace/target/release/server /usr/local/bin/server

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
COPY --link --from=builder \
    --chown=10001:10001 \
    --chmod=0555 \
    /usr/local/bin/server \
    /app/server

USER 10001:10001

EXPOSE 3000

ENTRYPOINT ["/app/server"]
