FROM rust:1.92-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /app/target/release/aml_analytical_node /usr/local/bin/
USER 65532:65532
EXPOSE 7001
ENTRYPOINT ["aml_analytical_node"]
