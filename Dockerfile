# ---- Build Stage ----
FROM rust:1.87-slim-bookworm as builder
WORKDIR /workspace

RUN apt-get update \
    && apt-get install -y --no-install-recommends libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release --bin server

# ---- Runtime Stage ----
FROM debian:bookworm-slim as final

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates sqlite3 \
    && update-ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /workspace/target/release/server .
COPY graph.db3 .

RUN useradd -m -u 1000 appuser
USER appuser

CMD ["./server"]
