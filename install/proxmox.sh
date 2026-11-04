#!/usr/bin/env bash
# Creates an LXC container for Swarmy and installs Swarmy inside it; nothing is
# installed on the Proxmox host itself. Run it on the host, as root:
#
#   bash -c "$(curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/proxmox.sh)"
#
# Options (environment variables), with defaults:
#   CTID=<next free id>    CT_HOSTNAME=swarmy    CORES=2    RAM=1024 (MB)    DISK=8 (GB)
#   STORAGE=<first storage that holds containers, e.g. local-lvm or local-zfs>
#   TEMPLATE_STORAGE=<first storage that holds templates, usually local>
#   BRIDGE=vmbr0           PORT=8080 (web UI)
#   DOWNLOADS_HOST=/path   bind-mount a folder on the host as Swarmy's download folder
#   DOWNLOADS_CHOWN=1      also make that folder writable by the container's swarmy user
#
# For example:
#   CTID=120 DOWNLOADS_HOST=/tank/downloads bash -c "$(curl -fsSL .../proxmox.sh)"
set -euo pipefail

REPO="${SWARMY_REPO:-garethcheyne/swarmy}"
RAW="https://raw.githubusercontent.com/$REPO/main/install"
say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }

command -v pct >/dev/null || die "run this on the Proxmox VE host (pct not found)."
[ "$(id -u)" -eq 0 ] || die "run this as root."

# Not HOSTNAME: bash always sets that to this host's own name.
NAME="${CT_HOSTNAME:-swarmy}"
CTID="${CTID:-$(pvesh get /cluster/nextid)}"
CORES="${CORES:-2}"
RAM="${RAM:-1024}"
DISK="${DISK:-8}"
BRIDGE="${BRIDGE:-vmbr0}"
PORT="${PORT:-8080}"
CT_DOWNLOADS=/var/lib/swarmy/downloads

pct status "$CTID" >/dev/null 2>&1 && die "container $CTID already exists. Pick another with CTID=..."

first_storage() { # first active storage that can hold $1 (rootdir or vztmpl)
  pvesm status --content "$1" 2>/dev/null | awk 'NR > 1 && $3 == "active" { print $1; exit }'
}
STORAGE="${STORAGE:-$(first_storage rootdir)}"
TEMPLATE_STORAGE="${TEMPLATE_STORAGE:-$(first_storage vztmpl)}"
[ -n "$STORAGE" ] || die "no storage can hold containers. Set STORAGE=..."
[ -n "$TEMPLATE_STORAGE" ] || die "no storage can hold templates. Set TEMPLATE_STORAGE=..."
if [ -n "${DOWNLOADS_HOST:-}" ]; then
  [ -d "$DOWNLOADS_HOST" ] || die "DOWNLOADS_HOST $DOWNLOADS_HOST doesn't exist."
fi

say "Finding the latest Debian template"
pveam update >/dev/null || true
TEMPLATE=""
for release in 13 12; do
  TEMPLATE="$(pveam available --section system | awk -v r="debian-$release-standard" '$2 ~ r { print $2 }' | sort -V | tail -n1)"
  [ -n "$TEMPLATE" ] && break
done
[ -n "$TEMPLATE" ] || die "no Debian 12 or 13 template is available from pveam."
if ! pveam list "$TEMPLATE_STORAGE" | grep -q "$TEMPLATE"; then
  say "Downloading $TEMPLATE to $TEMPLATE_STORAGE"
  pveam download "$TEMPLATE_STORAGE" "$TEMPLATE" >/dev/null
fi

say "Creating container $CTID ($NAME) on $STORAGE: $CORES cores, $RAM MB, $DISK GB"
pct create "$CTID" "$TEMPLATE_STORAGE:vztmpl/$TEMPLATE" \
  --hostname "$NAME" \
  --cores "$CORES" --memory "$RAM" --swap 512 \
  --rootfs "$STORAGE:$DISK" \
  --net0 "name=eth0,bridge=$BRIDGE,ip=dhcp" \
  --unprivileged 1 --features nesting=1 \
  --onboot 1 >/dev/null

if [ -n "${DOWNLOADS_HOST:-}" ]; then
  pct set "$CTID" -mp0 "$DOWNLOADS_HOST,mp=$CT_DOWNLOADS"
fi

pct start "$CTID"

say "Waiting for the container's network"
for _ in $(seq 1 30); do
  if pct exec "$CTID" -- getent hosts github.com >/dev/null 2>&1; then break; fi
  sleep 2
done
pct exec "$CTID" -- getent hosts github.com >/dev/null 2>&1 \
  || die "container $CTID has no network or DNS. Check BRIDGE=$BRIDGE has DHCP, then run: pct destroy $CTID"

say "Installing Swarmy inside the container"
pct exec "$CTID" -- env DEBIAN_FRONTEND=noninteractive bash -c "apt-get update -qq && apt-get install -y -qq curl ca-certificates >/dev/null"
pct exec "$CTID" -- env SWARMY_PORT="$PORT" SWARMY_DOWNLOADS="$CT_DOWNLOADS" \
  bash -c "curl -fsSL $RAW/install.sh | bash"

if [ -n "${DOWNLOADS_HOST:-}" ]; then
  # An unprivileged container's users are shifted by 100000 on the host.
  UID_IN="$(pct exec "$CTID" -- id -u swarmy)"
  GID_IN="$(pct exec "$CTID" -- id -g swarmy)"
  HOST_UID=$((100000 + UID_IN))
  HOST_GID=$((100000 + GID_IN))
  if [ "${DOWNLOADS_CHOWN:-0}" = "1" ]; then
    say "Giving the container's swarmy user ($HOST_UID:$HOST_GID on the host) $DOWNLOADS_HOST"
    chown "$HOST_UID:$HOST_GID" "$DOWNLOADS_HOST"
  fi
fi

echo
say "Swarmy is running in container $CTID ($NAME)"
echo "  Upgrade:   pct exec $CTID -- bash -c \"curl -fsSL $RAW/install.sh | bash\""
echo "  Shell:     pct enter $CTID"
if [ -n "${DOWNLOADS_HOST:-}" ] && [ "${DOWNLOADS_CHOWN:-0}" != "1" ]; then
  echo
  echo "  Downloads go to $DOWNLOADS_HOST on this host. For Swarmy to write there, run:"
  echo "    chown $HOST_UID:$HOST_GID $DOWNLOADS_HOST"
  echo "  (or re-run with DOWNLOADS_CHOWN=1). Sonarr and Radarr need to see the same files."
fi
