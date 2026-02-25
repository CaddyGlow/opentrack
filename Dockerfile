# syntax=docker/dockerfile:1.7

FROM rust:1.92-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake perl pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --locked --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates chromium \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --uid 10001 --shell /usr/sbin/nologin opentrack

ENV CHROME=/usr/bin/chromium \
    CHROME_PATH=/usr/bin/chromium \
    XDG_CONFIG_HOME=/home/opentrack/.config \
    XDG_CACHE_HOME=/home/opentrack/.cache

COPY --from=builder /app/target/release/opentrack /usr/local/bin/opentrack

USER opentrack
WORKDIR /home/opentrack

ENTRYPOINT ["/usr/local/bin/opentrack"]
CMD ["--help"]
