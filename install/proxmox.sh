#!/usr/bin/env bash
# Create a Debian LXC container on Proxmox VE and install Swarmy in it.
# Run on the Proxmox host as root:
#
#   bash -c "$(curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/proxmox.sh)"
#
# Options (environment variables), with defaults:
#   CTID=<next free id>   HOSTNAME=swarmy   CORES=2   RAM=1024   DISK=8 (GB)
#   STORAGE=local-lvm     TEMPLATE_STORAGE=local     BRIDGE=vmbr0
#   DOWNLOADS_HOST=/path/on/host   bind-mount a host folder as the download folder
set -euo pipefail

REPO="${SWARMY_REPO:-garethcheyne/swarmy}"
say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }

command -v pct >/dev/null || die "run this on the Proxmox VE host (pct not found)."
[ "$(id -u)" -eq 0 ] || die "run this as root."

CTID="${CTID:-$(pvesh get /cluster/nextid)}"
HOSTNAME="${HOSTNAME:-swarmy}"
CORES="${CORES:-2}"
RAM="${RAM:-1024}"
DISK="${DISK:-8}"
STORAGE="${STORAGE:-local-lvm}"
TEMPLATE_STORAGE="${TEMPLATE_STORAGE:-local}"
BRIDGE="${BRIDGE:-vmbr0}"

say "Finding the latest Debian 12 template"
pveam update >/dev/null
TEMPLATE="$(pveam available --section system | awk '/debian-12-standard/ {print $2}' | sort -V | tail -n1)"
[ -n "$TEMPLATE" ] || die "no Debian 12 template is available from pveam."
if ! pveam list "$TEMPLATE_STORAGE" | grep -q "$TEMPLATE"; then
  say "Downloading $TEMPLATE"
  pveam download "$TEMPLATE_STORAGE" "$TEMPLATE"
fi

say "Creating container $CTID ($HOSTNAME)"
pct create "$CTID" "$TEMPLATE_STORAGE:vztmpl/$TEMPLATE" \
  --hostname "$HOSTNAME" \
  --cores "$CORES" --memory "$RAM" --swap 512 \
  --rootfs "$STORAGE:$DISK" \
  --net0 "name=eth0,bridge=$BRIDGE,ip=dhcp" \
  --unprivileged 1 --features nesting=1 \
  --onboot 1

if [ -n "${DOWNLOADS_HOST:-}" ]; then
  [ -d "$DOWNLOADS_HOST" ] || die "DOWNLOADS_HOST $DOWNLOADS_HOST doesn't exist."
  pct set "$CTID" -mp0 "$DOWNLOADS_HOST,mp=/var/lib/swarmy/downloads"
fi

pct start "$CTID"

say "Waiting for the network"
for _ in $(seq 1 30); do
  if pct exec "$CTID" -- getent hosts github.com >/dev/null 2>&1; then break; fi
  sleep 2
done
pct exec "$CTID" -- getent hosts github.com >/dev/null 2>&1 || die "container $CTID has no network or DNS."

say "Installing Swarmy inside the container"
pct exec "$CTID" -- bash -c "apt-get update -qq && apt-get install -y -qq curl ca-certificates >/dev/null"
pct exec "$CTID" -- bash -c "curl -fsSL https://raw.githubusercontent.com/$REPO/main/install/install.sh | bash"

IP="$(pct exec "$CTID" -- hostname -I | awk '{print $1}')"
echo
say "Swarmy is running in container $CTID"
echo "  Open:     http://$IP:8080"
echo "  Upgrade:  pct exec $CTID -- bash -c \"curl -fsSL https://raw.githubusercontent.com/$REPO/main/install/install.sh | bash\""
if [ -n "${DOWNLOADS_HOST:-}" ]; then
  echo "  Downloads: $DOWNLOADS_HOST on the host. In an unprivileged container, make it"
  echo "             writable by container uid 100000+ (for example: chown -R 100000:100000 $DOWNLOADS_HOST)"
fi
