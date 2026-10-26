# syntax=docker/dockerfile:1.7
FROM rust:1.92-slim-bookworm AS builder
COPY --from=warehouse_source / /build/shared/warehouse/
WORKDIR /build/dockerizd_bitcoin
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY sql ./sql
COPY web ./web
RUN --mount=type=cache,target=/usr/local/cargo/registry cargo build --locked --release

FROM debian:bookworm-slim
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /build/dockerizd_bitcoin/target/release/bitcoin_aml /usr/local/bin/bitcoin_aml
COPY --from=builder /build/dockerizd_bitcoin/target/release/bitcoin_init_schema /usr/local/bin/bitcoin_init_schema
RUN mkdir -p /var/lock/bitcoin && chown 10001:10001 /var/lock/bitcoin
USER 10001:10001
WORKDIR /app
STOPSIGNAL SIGTERM
CMD ["bitcoin_aml", "api"]
