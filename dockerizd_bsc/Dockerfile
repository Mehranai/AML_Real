# syntax=docker/dockerfile:1.7

FROM rust:1.92-bookworm AS builder
COPY --from=warehouse_source / /build/shared/warehouse/
WORKDIR /build/dockerizd_bsc
ENV CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10 CARGO_HTTP_TIMEOUT=600
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY sql ./sql
COPY web ./web
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --locked --release --bins \
    && install -D -m 0755 target/release/bsc_ingest /out/bsc_ingest \
    && install -D -m 0755 target/release/bsc_replay /out/bsc_replay \
    && install -D -m 0755 target/release/bsc_repair /out/bsc_repair \
    && install -D -m 0755 target/release/bsc_benchmark /out/bsc_benchmark \
    && install -D -m 0755 target/release/bsc_rpc_probe /out/bsc_rpc_probe \
    && install -D -m 0755 target/release/bsc_schema /out/bsc_schema \
    && install -D -m 0755 target/release/bsc_import_protocols /out/bsc_import_protocols \
    && install -D -m 0755 target/release/bsc_api /out/bsc_api \
    && install -D -m 0755 target/release/bsc_intelligence /out/bsc_intelligence \
    && install -D -m 0755 target/release/bsc_token_metadata_worker /out/bsc_token_metadata_worker \
    && strip --strip-unneeded /out/*

FROM debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171 AS runtime
ARG APP_UID=10001
ARG APP_GID=10001
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /out/ /usr/local/bin/
WORKDIR /app
ENV RUST_BACKTRACE=1 TZ=UTC
USER ${APP_UID}:${APP_GID}
STOPSIGNAL SIGTERM
CMD ["bsc_schema"]
