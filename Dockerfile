# syntax=docker/dockerfile:1

FROM docker.io/library/rust:1.99.1-bookworm AS build
WORKDIR /src
# No `query!` macros are used today, so no database is needed to build. If they are added, commit
# `.sqlx/` (cargo sqlx prepare --workspace): with this set, a missing cache fails the build loudly.
ENV SQLX_OFFLINE=true
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p sekondo

FROM docker.io/library/debian:bookworm-slim
# util-linux provides setpriv, used to drop root after the entrypoint has prepared the TLS key.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates util-linux \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin sekondo
COPY --from=build /src/target/release/sekondo /app/sekondo
COPY crates/sekondo/static /app/static
COPY docker/app/entrypoint.sh /app/entrypoint.sh
ENV STATIC_DIR=/app/static
EXPOSE 8443
ENTRYPOINT ["/app/entrypoint.sh"]
