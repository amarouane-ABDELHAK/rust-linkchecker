# Build from source for whatever CPU the host has, so Chromium is not emulated.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
# Debian's package is named "chromium" on PATH, one of the names the checker looks for.
RUN apt-get update \
 && apt-get install -y --no-install-recommends chromium ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/linkchecker /usr/local/bin/linkchecker
ENTRYPOINT ["linkchecker"]
