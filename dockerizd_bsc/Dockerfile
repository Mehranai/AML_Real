FROM rust:1.92-bookworm AS builder
WORKDIR /build
ENV CARGO_NET_OFFLINE=true

COPY vendor-linux.tar.gz vendor-linux.sha256 /tmp/
RUN cd /tmp \
    && sha256sum --check vendor-linux.sha256 \
    && mkdir -p /build/vendor /build/.cargo \
    && tar -xzf vendor-linux.tar.gz -C /build/vendor \
    && printf '[source.crates-io]\nreplace-with = "vendored-sources"\n\n[source.vendored-sources]\ndirectory = "/build/vendor"\n' > /build/.cargo/config.toml \
    && rm -f vendor-linux.tar.gz vendor-linux.sha256

COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src/bin \
    && printf 'fn main() {}\n' > src/bin/bsc_ingest.rs \
    && printf 'fn main() {}\n' > src/bin/bsc_replay.rs \
    && printf 'fn main() {}\n' > src/bin/bsc_repair.rs \
    && printf 'fn main() {}\n' > src/bin/bsc_benchmark.rs \
    && printf 'fn main() {}\n' > src/bin/bsc_rpc_probe.rs \
    && printf 'fn main() {}\n' > src/bin/bsc_schema.rs \
    && printf 'fn main() {}\n' > src/bin/bsc_import_protocols.rs \
    && cargo build --locked --offline --release --bins \
    && rm -rf src

COPY src ./src
COPY sql ./sql
RUN cargo build --locked --offline --release --bins \
    && install -D -m 0755 target/release/bsc_ingest /out/bsc_ingest \
    && install -D -m 0755 target/release/bsc_replay /out/bsc_replay \
    && install -D -m 0755 target/release/bsc_repair /out/bsc_repair \
    && install -D -m 0755 target/release/bsc_benchmark /out/bsc_benchmark \
    && install -D -m 0755 target/release/bsc_rpc_probe /out/bsc_rpc_probe \
    && install -D -m 0755 target/release/bsc_schema /out/bsc_schema \
    && install -D -m 0755 target/release/bsc_import_protocols /out/bsc_import_protocols \
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
