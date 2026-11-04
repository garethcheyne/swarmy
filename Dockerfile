# Swarmy, built from source.
#
#   docker build -t swarmy .
#   docker run -d -p 8080:8080 -p 6881:6881 -p 6881:6881/udp \
#     -v ./config:/config -v ./downloads:/downloads swarmy
#
# Release images are built by .github/workflows/release.yml from the same file.

# ---- web UI -----------------------------------------------------------------
FROM node:24-bookworm-slim AS web
WORKDIR /src/web
RUN corepack enable
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml web/.npmrc ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# ---- daemon -----------------------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /src
# Vendored OpenSSL needs perl + make (already in the image); nothing else to add.
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
# The server embeds the built UI from ../../web/dist at compile time.
COPY --from=web /src/web/dist web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p swarmy \
    && cp target/release/swarmy /usr/local/bin/swarmy

# ---- runtime ----------------------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --home-dir /config --shell /usr/sbin/nologin swarmy \
    && mkdir -p /config /downloads \
    && chown swarmy:swarmy /config /downloads
COPY --from=build /usr/local/bin/swarmy /usr/local/bin/swarmy

ENV SWARMY_DATA_DIR=/config \
    SWARMY_DOWNLOAD_DIR=/downloads \
    SWARMY_WEB_ADDR=0.0.0.0:8080 \
    SWARMY_IN_CONTAINER=1
# Web UI / API, and BitTorrent (TCP + uTP).
EXPOSE 8080 6881/tcp 6881/udp
VOLUME ["/config", "/downloads"]
USER swarmy
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s \
    CMD curl -fsS -o /dev/null http://127.0.0.1:8080/ || exit 1
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/swarmy"]
