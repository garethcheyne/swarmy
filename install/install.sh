#!/usr/bin/env bash
# Install or upgrade Swarmy on Linux (any systemd distro, x86-64 or ARM64).
#
#   curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/install.sh | sudo bash
#
# Run it again to upgrade. Options (environment variables):
#   SWARMY_VERSION=v0.2.0    install a specific release (default: latest)
#   SWARMY_PORT=8080         web UI port (first install only)
#   SWARMY_DOWNLOADS=/path   download folder (first install only)
#   SWARMY_UNINSTALL=1       remove Swarmy (keeps your data and downloads)
set -euo pipefail

REPO="${SWARMY_REPO:-garethcheyne/swarmy}"
VERSION="${SWARMY_VERSION:-latest}"
PORT="${SWARMY_PORT:-8080}"
DATA_DIR=/var/lib/swarmy
DOWNLOADS="${SWARMY_DOWNLOADS:-$DATA_DIR/downloads}"
BIN_DIR=/opt/swarmy
ENV_FILE=/etc/swarmy/swarmy.env
UNIT=/etc/systemd/system/swarmy.service

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run this as root (put sudo in front of bash)."
command -v systemctl >/dev/null || die "this installer needs systemd. Use the Docker image instead."

if [ "${SWARMY_UNINSTALL:-0}" = "1" ]; then
  say "Removing Swarmy (your data in $DATA_DIR and downloads are kept)"
  systemctl disable --now swarmy 2>/dev/null || true
  rm -f "$UNIT" /usr/local/bin/swarmy
  rm -rf "$BIN_DIR"
  systemctl daemon-reload
  say "Done. Delete $DATA_DIR and /etc/swarmy yourself if you want them gone too."
  exit 0
fi

for tool in curl tar sha256sum; do
  command -v "$tool" >/dev/null || die "$tool is missing. Install it (e.g. apt install $tool) and try again."
done

case "$(uname -m)" in
  x86_64 | amd64) ARCH=x86_64 ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) die "unsupported CPU $(uname -m). Swarmy ships for x86-64 and ARM64; try the Docker image." ;;
esac

if [ "$VERSION" = "latest" ]; then
  VERSION="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)"
  [ -n "$VERSION" ] || die "couldn't find the latest release of $REPO."
fi

ASSET="swarmy-$VERSION-linux-$ARCH.tar.gz"
BASE="https://github.com/$REPO/releases/download/$VERSION"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

say "Downloading Swarmy $VERSION for $ARCH"
curl -fsSL -o "$TMP/$ASSET" "$BASE/$ASSET" || die "download failed: $BASE/$ASSET"
curl -fsSL -o "$TMP/SHA256SUMS" "$BASE/SHA256SUMS" || die "couldn't download checksums"
(cd "$TMP" && grep " $ASSET\$" SHA256SUMS | sha256sum -c --quiet -) || die "checksum mismatch, refusing to install"
tar -xzf "$TMP/$ASSET" -C "$TMP"

UPGRADE=0
[ -x "$BIN_DIR/swarmy" ] && UPGRADE=1

if ! id swarmy >/dev/null 2>&1; then
  useradd --system --home-dir "$DATA_DIR" --shell /usr/sbin/nologin swarmy
fi
install -d -o swarmy -g swarmy "$DATA_DIR" "$DOWNLOADS"
install -d "$BIN_DIR" /etc/swarmy
install -m 0755 "$TMP/swarmy" "$BIN_DIR/swarmy"
ln -sf "$BIN_DIR/swarmy" /usr/local/bin/swarmy

if [ ! -f "$ENV_FILE" ]; then
  cat > "$ENV_FILE" <<EOF
# Swarmy settings for the systemd service. Most settings live in the web UI
# (Settings); flags set here override and lock the matching setting there.
SWARMY_DATA_DIR=$DATA_DIR
SWARMY_DOWNLOAD_DIR=$DOWNLOADS
SWARMY_WEB_ADDR=0.0.0.0:$PORT
# SWARMY_BIND_INTERFACE=wg0
# SWARMY_PASSWORD=change-me
EOF
fi

cat > "$UNIT" <<EOF
[Unit]
Description=Swarmy BitTorrent client
Documentation=https://github.com/$REPO
After=network-online.target
Wants=network-online.target

[Service]
User=swarmy
Group=swarmy
EnvironmentFile=$ENV_FILE
ExecStart=$BIN_DIR/swarmy
Restart=on-failure
RestartSec=5
LimitNOFILE=65536
NoNewPrivileges=yes
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
if [ "$UPGRADE" = "1" ]; then
  systemctl restart swarmy
  say "Upgraded to Swarmy $VERSION."
else
  systemctl enable --now swarmy
  say "Swarmy $VERSION is installed and running."
fi

HOST_IP="$(hostname -I 2>/dev/null | awk '{print $1}')"
PORT_NOW="$(sed -n 's/^SWARMY_WEB_ADDR=.*:\([0-9]*\)$/\1/p' "$ENV_FILE")"
echo
echo "  Open:      http://${HOST_IP:-localhost}:${PORT_NOW:-$PORT}"
if [ "$UPGRADE" = "0" ]; then
  sleep 3
  PW="$(journalctl -u swarmy --no-pager -o cat 2>/dev/null | sed -n 's/^ *password: *//p' | tail -n1)"
  echo "  Username:  admin"
  echo "  Password:  ${PW:-run: journalctl -u swarmy | grep password}"
  echo
  echo "  Change the password in Settings > Web UI and security."
fi
echo "  Logs:      journalctl -u swarmy -f"
echo "  Settings:  $ENV_FILE (service), and Settings in the web UI"
