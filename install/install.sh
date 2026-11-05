#!/usr/bin/env bash
# Install or upgrade Swarmy on Linux (any systemd distro, x86-64 or ARM64).
#
#   curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/install.sh | sudo bash
#
# Run it again to upgrade. Options (environment variables):
#   SWARMY_VERSION=v2026.11.05.1   install a specific release (default: latest)
#   SWARMY_PORT=8080         web UI port (first install only)
#   SWARMY_DOWNLOADS=/path   download folder (first install only)
#   SWARMY_UNINSTALL=1       remove Swarmy (keeps your data and downloads)
#
# Put options after sudo so they reach the script:
#   curl -fsSL .../install.sh | sudo SWARMY_PORT=9090 bash
set -euo pipefail

REPO="${SWARMY_REPO:-garethcheyne/swarmy}"
VERSION="${SWARMY_VERSION:-latest}"
PORT="${SWARMY_PORT:-8080}"
DATA_DIR=/var/lib/swarmy
DOWNLOADS="${SWARMY_DOWNLOADS:-$DATA_DIR/downloads}"
BIN_DIR=/opt/swarmy
ENV_FILE=/etc/swarmy/swarmy.env
UNIT=/etc/systemd/system/swarmy.service
# One-click updates: Swarmy creates UPDATE_REQUEST, swarmy-update.path sees it
# and swarmy-update.service (root) runs UPDATER, which installs the newest release.
UPDATE_UNIT=/etc/systemd/system/swarmy-update.service
UPDATE_PATH=/etc/systemd/system/swarmy-update.path
UPDATER="$BIN_DIR/update"
UPDATE_REQUEST="$DATA_DIR/.update-request"
UPDATE_LOG=/var/log/swarmy-update.log

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mNote:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run this as root (put sudo in front of bash)."
command -v systemctl >/dev/null || die "this installer needs systemd. Use the Docker image instead."

if [ "${SWARMY_UNINSTALL:-0}" = "1" ]; then
  say "Removing Swarmy (your data in $DATA_DIR and downloads are kept)"
  systemctl disable --now swarmy swarmy-update.path 2>/dev/null || true
  rm -f "$UNIT" "$UPDATE_UNIT" "$UPDATE_PATH" "$UPDATE_LOG" "$UPDATE_REQUEST" /usr/local/bin/swarmy /usr/local/bin/swarmy-reset-password
  rm -rf "$BIN_DIR"
  systemctl daemon-reload
  say "Done. Delete $DATA_DIR and /etc/swarmy yourself if you want them gone too."
  exit 0
fi

# Swarmy verifies TLS (Usenet servers, the location database) with the system's certificates.
if [ ! -s /etc/ssl/certs/ca-certificates.crt ] && [ ! -s /etc/pki/tls/certs/ca-bundle.crt ]; then
  if command -v apt-get >/dev/null; then
    say "Installing CA certificates"
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq ca-certificates >/dev/null 2>&1 || { apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq ca-certificates >/dev/null; }
  elif command -v dnf >/dev/null; then
    dnf install -y -q ca-certificates
  else
    warn "no CA certificates found. Install your distro's ca-certificates package."
  fi
fi

for tool in curl tar sha256sum; do
  command -v "$tool" >/dev/null || die "$tool is missing. Install it (e.g. apt install $tool) and try again."
done

case "$(uname -m)" in
  x86_64 | amd64) ARCH=x86_64 ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) die "unsupported CPU $(uname -m). Swarmy ships for x86-64 and ARM64; try the Docker image." ;;
esac

# Release builds need glibc 2.36 or newer (Debian 12, Ubuntu 22.04, Fedora 37 and later).
GLIBC="$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}')"
if [ -z "$GLIBC" ]; then
  die "this system doesn't use glibc (Alpine?). Use the Docker image instead."
elif [ "$(printf '%s\n2.36\n' "$GLIBC" | sort -V | head -n1)" != "2.36" ]; then
  die "glibc $GLIBC is too old; Swarmy needs 2.36 or newer (Debian 12, Ubuntu 22.04 or later). Use the Docker image instead."
fi

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
install -d -o swarmy -g swarmy "$DATA_DIR"
mkdir -p "$DOWNLOADS"
# A folder mounted from elsewhere (an NFS share, a Proxmox bind mount) may not
# let us change its owner; Swarmy only needs to be able to write to it.
if [ "$(stat -c %U "$DOWNLOADS")" != "swarmy" ] && ! chown swarmy:swarmy "$DOWNLOADS" 2>/dev/null; then
  warn "couldn't make the swarmy user the owner of $DOWNLOADS. Make sure it can write there."
fi
install -d "$BIN_DIR" /etc/swarmy /usr/share/doc/swarmy
install -m 0755 "$TMP/swarmy" "$BIN_DIR/swarmy"
for doc in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md CHANGELOG.md; do
  [ -f "$TMP/$doc" ] && install -m 0644 "$TMP/$doc" /usr/share/doc/swarmy/
done
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
Description=Swarmy BitTorrent and Usenet client
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
# One-click updates in Settings > About; see swarmy-update.path.
Environment=SWARMY_UPDATE_REQUEST=$UPDATE_REQUEST
Environment=SWARMY_UPDATE_LOG=$UPDATE_LOG

[Install]
WantedBy=multi-user.target
EOF

# The updater. Swarmy runs unprivileged and can only ask for an update; this
# decides what to install (the newest release, checksum verified by
# install.sh), so Swarmy can't get anything else installed as root. Written to
# a temporary file and renamed, because this may be the updater running now.
cat > "$UPDATER.new" <<EOF
#!/usr/bin/env bash
# Installs the newest Swarmy release. swarmy-update.service runs this as root
# when Swarmy asks for an update (Settings > About) by creating $UPDATE_REQUEST.
set -euo pipefail
main() {
  install -m 0644 /dev/null "$UPDATE_LOG"
  exec >>"$UPDATE_LOG" 2>&1
  # Swarmy reads this line to know the update has finished, and how.
  trap 'echo "Updater finished: exit status \$?"' EXIT
  rm -f "$UPDATE_REQUEST"
  echo "\$(date '+%Y-%m-%d %H:%M:%S') Looking for the newest Swarmy release"
  local tag
  tag="\$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)"
  if ! printf '%s\n' "\$tag" | grep -Eq '^v[0-9]{4}\.[0-9]{2}\.[0-9]{2}\.[0-9]{1,4}\$'; then
    echo "Error: couldn't find the newest release on GitHub."
    exit 1
  fi
  echo "Updating to Swarmy \$tag"
  curl -fsSL "https://raw.githubusercontent.com/$REPO/\$tag/install/install.sh" \\
    | SWARMY_VERSION="\$tag" SWARMY_REPO="$REPO" bash
}
main "\$@"
EOF
chmod 0755 "$UPDATER.new"
mv -f "$UPDATER.new" "$UPDATER"

# `sudo swarmy-reset-password`: a new web UI password when nobody can sign in.
# It stops Swarmy, has the binary change the password as Swarmy's own user (so
# the settings stay Swarmy's), and starts Swarmy again.
RESET_PW="$BIN_DIR/reset-password"
cat > "$RESET_PW.new" <<EOF
#!/usr/bin/env bash
# Set a new password for Swarmy's web UI and sign everyone out.
#   sudo swarmy-reset-password                 asks for it (Enter for a random one)
#   sudo swarmy-reset-password --username me   changes the username too
#   echo 'new password' | sudo swarmy-reset-password
set -euo pipefail
[ "\$(id -u)" -eq 0 ] || { echo "Run it as root: sudo \$0" >&2; exit 1; }
pw=""
if [ -t 0 ]; then
  read -rsp "New password for Swarmy (at least 8 characters, or Enter for a random one): " pw; echo
  if [ -n "\$pw" ]; then
    read -rsp "The same again: " again; echo
    [ "\$pw" = "\$again" ] || { echo "Those didn't match. Nothing was changed." >&2; exit 1; }
  fi
else
  IFS= read -r pw || true
fi
if grep -q '^SWARMY_PASSWORD=' "$ENV_FILE" 2>/dev/null; then
  echo "Note: $ENV_FILE sets SWARMY_PASSWORD, which replaces the password every time Swarmy starts." >&2
  echo "Remove that line, or this change won't last: sed -i '/^SWARMY_PASSWORD=/d' $ENV_FILE" >&2
fi
was_running=0
if systemctl is-active --quiet swarmy; then
  was_running=1
  systemctl stop swarmy
fi
restart() { if [ "\$was_running" = 1 ]; then systemctl start swarmy; fi; }
trap restart EXIT
(
  set -a; . "$ENV_FILE"; set +a
  printf '%s\n' "\$pw" | runuser -u swarmy -- env \
    SWARMY_DATA_DIR="\${SWARMY_DATA_DIR:-$DATA_DIR}" SWARMY_WEB_ADDR="\${SWARMY_WEB_ADDR:-0.0.0.0:8080}" \
    "$BIN_DIR/swarmy" reset-password "\$@"
)
EOF
chmod 0755 "$RESET_PW.new"
mv -f "$RESET_PW.new" "$RESET_PW"
ln -sf "$RESET_PW" /usr/local/bin/swarmy-reset-password

cat > "$UPDATE_UNIT" <<EOF
[Unit]
Description=Install the newest Swarmy release (asked for in Swarmy's settings)
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=$UPDATER
EOF

cat > "$UPDATE_PATH" <<EOF
[Unit]
Description=Let Swarmy ask for an update

[Path]
PathExists=$UPDATE_REQUEST

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable --now swarmy-update.path >/dev/null 2>&1 || warn "couldn't turn on one-click updates (swarmy-update.path)."

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
  # The first start creates the account and logs its password once.
  PW=""
  for _ in $(seq 1 15); do
    PW="$(journalctl -u swarmy --no-pager -o cat 2>/dev/null | sed -n 's/^ *password: *//p' | tail -n1)"
    [ -n "$PW" ] && break
    sleep 1
  done
  echo "  Username:  admin"
  echo "  Password:  ${PW:-run: journalctl -u swarmy | grep password}"
  echo
  echo "  Change the password in Settings > Web UI and security."
fi
echo "  Forgot the password? sudo swarmy-reset-password"
echo "  Downloads: $(sed -n 's/^SWARMY_DOWNLOAD_DIR=//p' "$ENV_FILE")"
echo "  Logs:      journalctl -u swarmy -f"
echo "  Settings:  $ENV_FILE (service), and Settings in the web UI"
