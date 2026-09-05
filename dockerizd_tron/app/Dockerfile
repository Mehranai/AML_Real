# syntax=docker/dockerfile:1.7

FROM rust:1.92-bookworm AS builder
WORKDIR /build
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libssl-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && printf 'fn main() {}\n' > src/main.rs \
    && cargo build --locked --release \
    && rm -rf src

COPY src ./src
COPY sql ./sql
COPY web ./web
RUN cargo build --locked --release --bins \
    && mkdir -p /out \
    && find target/release -maxdepth 1 -type f -executable -exec cp '{}' /out/ \; \
    && strip --strip-unneeded /out/*

FROM debian:bookworm-slim AS runtime
ARG APP_UID=10001
ARG APP_GID=10001
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl libssl3 tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid "${APP_GID}" tron \
    && useradd --uid "${APP_UID}" --gid "${APP_GID}" --create-home --shell /usr/sbin/nologin tron
COPY --from=builder /out/ /usr/local/bin/
WORKDIR /app
ENV RUST_BACKTRACE=1 TZ=UTC
USER tron:tron
EXPOSE 4001
STOPSIGNAL SIGTERM
CMD ["arz_axum_for_services"]
