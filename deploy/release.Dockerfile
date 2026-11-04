# The release image: the binaries built by .github/workflows/release.yml,
# on the same runtime as ../Dockerfile (which builds from source instead).
# The build context holds amd64/ and arm64/, each an unpacked release archive.

FROM debian:bookworm-slim
ARG TARGETARCH
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --home-dir /config --shell /usr/sbin/nologin swarmy \
    && mkdir -p /config /downloads \
    && chown swarmy:swarmy /config /downloads
COPY ${TARGETARCH}/swarmy /usr/local/bin/swarmy
COPY ${TARGETARCH}/LICENSE-MIT ${TARGETARCH}/LICENSE-APACHE ${TARGETARCH}/THIRD-PARTY-NOTICES.md /usr/share/doc/swarmy/

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
