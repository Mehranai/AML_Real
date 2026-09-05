# syntax=docker/dockerfile:1.7

FROM rust:1.92-bookworm AS builder
WORKDIR /build
ENV CARGO_HTTP_MULTIPLEXING=false \
    CARGO_NET_RETRY=10 \
    CARGO_HTTP_TIMEOUT=600

COPY Cargo.toml Cargo.lock ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    mkdir -p src/bin \
    && printf 'fn main() {}\n' > src/bin/ethereum_ingestor.rs \
    && cargo build --locked --release --bin ethereum_ingestor \
    && rm -rf src

COPY src ./src
COPY sql ./sql
COPY web ./web
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --locked --release --bins \
    && mkdir -p /out \
    && find target/release -maxdepth 1 -type f -executable -exec cp '{}' /out/ \; \
    && strip --strip-unneeded /out/*

FROM debian:bookworm-slim AS runtime
ARG APP_UID=10001
ARG APP_GID=10001
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /out/ /usr/local/bin/
WORKDIR /app
ENV RUST_BACKTRACE=1 TZ=UTC
USER ${APP_UID}:${APP_GID}
EXPOSE 5001
STOPSIGNAL SIGTERM
CMD ["ethereum_ingestor", "follow"]
