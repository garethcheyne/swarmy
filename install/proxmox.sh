#!/usr/bin/env bash
# Creates an LXC container for Swarmy and installs Swarmy inside it; nothing is
# installed on the Proxmox host itself. Run it in the host's shell, as root:
#
#   bash -c "$(curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/proxmox.sh)"
#
# It asks where to put the container, which of the host's shared storage (NAS
# shares and so on) to map into it, and where downloads go, then shows a
# summary before creating anything. If setup fails, the half-made container
# is removed.
#
# Every answer can be given up front instead (and NONINTERACTIVE=1 asks nothing):
#   CTID=<next free id>   CT_HOSTNAME=swarmy   CORES=2   RAM=1024 (MB)   DISK=8 (GB)
#   STORAGE=local-lvm     where the container's disk goes (default: the best local storage)
#   TEMPLATE_STORAGE=local
#   BRIDGE=vmbr0          PORT=8080 (web UI)
#   SHARES="NAS-Public NAS-Media"   storages to map, at /mnt/<name> in the container
#   DOWNLOADS_SHARE=NAS-Public      download to this mapped storage...
#   DOWNLOADS_SUBDIR=downloads      ...in this folder on it
#   DOWNLOADS_HOST=/some/host/path  or download to any folder on the host
#   DOWNLOADS_CHOWN=1     let the container write to a download folder that already existed
#   CT_PASSWORD=...       root password for the container's console (default: the console
#                         signs in as root by itself; only Proxmox users can open it)
#   AUTO_UPDATE=0         don't install system updates by themselves (default: on, daily)
#   SMB_FIX=1             if the download share is SMB and the container can't write to it,
#                         remount it so containers can (see the note printed if this happens)
#   KEEP_ON_FAILURE=1     keep the container if setup fails, to look into it
set -euo pipefail

REPO="${SWARMY_REPO:-garethcheyne/swarmy}"
RAW="https://raw.githubusercontent.com/$REPO/main/install"
STORAGE_CFG="${PVE_STORAGE_CFG:-/etc/pve/storage.cfg}"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mNote:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }

command -v pct >/dev/null || die "run this on the Proxmox VE host (pct not found)."
[ "$(id -u)" -eq 0 ] || die "run this as root."

INTERACTIVE=0
if [ -t 0 ] && [ -t 1 ] && [ "${NONINTERACTIVE:-0}" != "1" ]; then INTERACTIVE=1; fi
WT=0
command -v whiptail >/dev/null && WT=1

# ---------------------------------------------------------------- questions

TITLE="Swarmy installer"
ask_yesno() { # text [question for the plain prompt]
  if [ "$WT" = 1 ]; then
    whiptail --backtitle "$TITLE" --title "Swarmy" --yesno "$1" 22 78
  else
    printf '%s\n' "$1"
    read -rp "${2:-Continue?} [Y/n] " a
    [[ ! "$a" =~ ^[Nn] ]]
  fi
}
ask_menu() { # text default tag item tag item...
  local text="$1" def="$2"
  shift 2
  if [ "$WT" = 1 ]; then
    whiptail --backtitle "$TITLE" --title "Swarmy" --default-item "$def" --menu "$text" 20 78 10 "$@" 3>&1 1>&2 2>&3
  else
    local tags=() i=1 n
    printf '%s\n' "$text" >&2
    while [ $# -gt 0 ]; do
      printf '  %d) %s  %s\n' "$i" "$1" "$2" >&2
      tags+=("$1")
      shift 2
      i=$((i + 1))
    done
    read -rp "Number [default: $def]: " n
    if [ -z "$n" ]; then echo "$def"; else echo "${tags[$((n - 1))]}"; fi
  fi
}
ask_checklist() { # text tag item tag item... (all start unticked)
  local text="$1"
  shift
  if [ "$WT" = 1 ]; then
    local args=()
    while [ $# -gt 0 ]; do
      args+=("$1" "$2" OFF)
      shift 2
    done
    whiptail --backtitle "$TITLE" --title "Swarmy" --separate-output --checklist "$text" 20 78 10 "${args[@]}" 3>&1 1>&2 2>&3
  else
    local tags=() i=1 nums n
    printf '%s\n' "$text" >&2
    while [ $# -gt 0 ]; do
      printf '  %d) %s  %s\n' "$i" "$1" "$2" >&2
      tags+=("$1")
      shift 2
      i=$((i + 1))
    done
    read -rp "Numbers separated by spaces, or Enter for none: " nums
    for n in $nums; do echo "${tags[$((n - 1))]}"; done
  fi
}
ask_input() { # text default
  if [ "$WT" = 1 ]; then
    whiptail --backtitle "$TITLE" --title "Swarmy" --inputbox "$1" 12 78 "$2" 3>&1 1>&2 2>&3
  else
    local v
    read -rp "$1 [$2]: " v
    echo "${v:-$2}"
  fi
}

# ---------------------------------------------------------------- what's here

# Storages that can hold $1 (rootdir or vztmpl), best first: local block
# storage, then local folders, then network storage last. "name type GiB-free".
storages_for() {
  pvesm status --content "$1" 2>/dev/null | awk 'NR > 1 && $3 == "active" {
    t = $2; s = 5
    if (t == "zfspool" || t == "lvmthin") s = 1
    else if (t == "lvm" || t == "btrfs" || t == "rbd") s = 2
    else if (t == "dir") s = 3
    if ($1 !~ /^local/) s += 0.5
    printf "%s %s %s %d\n", s, $1, t, $6 / 1048576
  }' | sort -n | cut -d" " -f2-
}

# The host's shared storage that's mounted as a folder: "name type path".
shared_storages() {
  [ -r "$STORAGE_CFG" ] || return 0
  awk '
    function flush() {
      if (name == "" || name == "local" || off) return
      if (type !~ /^(dir|nfs|cifs|cephfs|glusterfs)$/) return
      p = path; if (p == "" && type != "dir") p = "/mnt/pve/" name
      if (p != "") print name, type, p
    }
    /^[a-z]+:[ \t]/ { flush(); type = $1; sub(/:$/, "", type); name = $2; path = ""; off = 0; next }
    /^[ \t]+path[ \t]/ { path = $2 }
    /^[ \t]+disable/ { off = 1 }
    END { flush() }
  ' "$STORAGE_CFG" | while read -r n t p; do
    # A network share that isn't mounted right now can't be mapped.
    if [ "$t" != "dir" ] && ! mountpoint -q "$p" 2>/dev/null; then continue; fi
    [ -d "$p" ] && echo "$n $t $p"
  done
}

ARCH="$(dpkg --print-architecture 2>/dev/null || echo amd64)"
NAME="${CT_HOSTNAME:-swarmy}" # not HOSTNAME: bash sets that to this host's name
CTID="${CTID:-$(pvesh get /cluster/nextid)}"
CORES="${CORES:-2}"
RAM="${RAM:-1024}"
DISK="${DISK:-8}"
BRIDGE="${BRIDGE:-vmbr0}"
PORT="${PORT:-8080}"
SUBDIR="${DOWNLOADS_SUBDIR:-downloads}"

pct status "$CTID" >/dev/null 2>&1 && die "container $CTID already exists. Pick another with CTID=..."

mapfile -t ROOT_OPTIONS < <(storages_for rootdir)
[ "${#ROOT_OPTIONS[@]}" -gt 0 ] || die "no active storage can hold containers."
mapfile -t SHARED < <(shared_storages)

TEMPLATE_STORAGE="${TEMPLATE_STORAGE:-}"
if [ -z "$TEMPLATE_STORAGE" ]; then
  mapfile -t TPL < <(storages_for vztmpl)
  for s in "${TPL[@]}"; do
    if [ "${s%% *}" = "local" ]; then TEMPLATE_STORAGE=local; fi
  done
  [ -n "$TEMPLATE_STORAGE" ] || TEMPLATE_STORAGE="${TPL[0]%% *}"
fi
[ -n "$TEMPLATE_STORAGE" ] || die "no storage can hold container templates. Set TEMPLATE_STORAGE=..."

share_field() { # name field-number (2 = type, 3 = path)
  local s n t p
  for s in "${SHARED[@]}"; do
    read -r n t p <<<"$s"
    if [ "$n" = "$1" ]; then
      if [ "$2" = 2 ]; then echo "$t"; else echo "$p"; fi
      return
    fi
  done
}
share_type() { share_field "$1" 2; }
share_path() { share_field "$1" 3; }

# A setting of a storage in storage.cfg, e.g. cfg_value backup content.
cfg_value() {
  awk -v want="$1" -v key="$2" '
    /^[a-z]+:[ \t]/ { name = $2; next }
    name == want && $1 == key { $1 = ""; sub(/^ /, ""); print; exit }
  ' "$STORAGE_CFG"
}

# Run in the container. The host's locale (often en_US.UTF-8) isn't installed
# in a fresh container; C.UTF-8 always is.
ct() { pct exec "$CTID" -- env LANG=C.UTF-8 LC_ALL=C.UTF-8 "$@"; }

# ---------------------------------------------------------------- ask

STORAGE="${STORAGE:-}"
SHARES="${SHARES:-}"
DL_SHARE="${DOWNLOADS_SHARE:-}"

if [ "$INTERACTIVE" = 1 ] && [ -z "$STORAGE" ]; then
  items=()
  for s in "${ROOT_OPTIONS[@]}"; do
    read -r n t free <<<"$s"
    items+=("$n" "$t, $free GiB free")
  done
  STORAGE="$(ask_menu "Where should the container's disk ($DISK GB) go?

Local storage is best. Containers on NFS or SMB storage often won't start." "${ROOT_OPTIONS[0]%% *}" "${items[@]}")" \
    || die "cancelled."
fi
STORAGE="${STORAGE:-${ROOT_OPTIONS[0]%% *}}"

if [ "$INTERACTIVE" = 1 ] && [ -z "$SHARES" ] && [ -z "${DOWNLOADS_HOST:-}" ] && [ "${#SHARED[@]}" -gt 0 ]; then
  items=()
  for s in "${SHARED[@]}"; do
    read -r n t p <<<"$s"
    items+=("$n" "$t  $p  ->  /mnt/$n")
  done
  SHARES="$(ask_checklist "Map any of this host's storage into the container?

Swarmy can download to a mapped share, and move finished downloads onto
any of them. Space ticks, Enter carries on; tick none to keep everything
inside the container." "${items[@]}" | tr '\n' ' ')" || die "cancelled."
fi
read -ra SHARE_LIST <<<"${SHARES//,/ }"
for s in "${SHARE_LIST[@]}"; do
  [ -n "$(share_path "$s")" ] || die "$s isn't a mounted shared storage on this host."
done

if [ "$INTERACTIVE" = 1 ] && [ -z "$DL_SHARE" ] && [ -z "${DOWNLOADS_HOST:-}" ] && [ "${#SHARE_LIST[@]}" -gt 0 ]; then
  items=()
  for s in "${SHARE_LIST[@]}"; do items+=("$s" "a folder on $s"); done
  items+=("container" "inside the container")
  DL_SHARE="$(ask_menu "Where should Swarmy download to?" "${SHARE_LIST[0]}" "${items[@]}")" || die "cancelled."
  [ "$DL_SHARE" = "container" ] && DL_SHARE=""
  if [ -n "$DL_SHARE" ]; then
    SUBDIR="$(ask_input "Folder on $DL_SHARE for downloads (created if it isn't there):" "$SUBDIR")" || die "cancelled."
  fi
fi
SUBDIR="${SUBDIR#/}"
SUBDIR="${SUBDIR%/}"

# Where downloads live, on the host and in the container.
HOST_DL=""
CT_DL=/var/lib/swarmy/downloads
DL_TYPE=dir
if [ -n "$DL_SHARE" ]; then
  [[ " ${SHARE_LIST[*]} " == *" $DL_SHARE "* ]] || SHARE_LIST+=("$DL_SHARE")
  [ -n "$(share_path "$DL_SHARE")" ] || die "$DL_SHARE isn't a mounted shared storage on this host."
  HOST_DL="$(share_path "$DL_SHARE")${SUBDIR:+/$SUBDIR}"
  CT_DL="/mnt/$DL_SHARE${SUBDIR:+/$SUBDIR}"
  DL_TYPE="$(share_type "$DL_SHARE")"
elif [ -n "${DOWNLOADS_HOST:-}" ]; then
  [ -d "$DOWNLOADS_HOST" ] || die "DOWNLOADS_HOST $DOWNLOADS_HOST doesn't exist on this host."
  HOST_DL="$DOWNLOADS_HOST"
fi

ROOT_PW="${CT_PASSWORD:-}"
if [ "$INTERACTIVE" = 1 ] && [ -z "$ROOT_PW" ]; then
  while true; do
    if [ "$WT" = 1 ]; then
      ROOT_PW="$(whiptail --backtitle "$TITLE" --title "Swarmy" --passwordbox "Root password for the container's console.

Leave it empty and the console signs in as root by itself (only people who
can use this Proxmox can open it). You can set one later: pct exec $CTID -- passwd" 14 78 3>&1 1>&2 2>&3)" || die "cancelled."
    else
      read -rsp "Root password for the container's console (Enter for none, the console signs in by itself): " ROOT_PW
      echo
    fi
    [ -z "$ROOT_PW" ] && break
    if [ "$WT" = 1 ]; then
      again="$(whiptail --backtitle "$TITLE" --title "Swarmy" --passwordbox "The same password again:" 10 78 3>&1 1>&2 2>&3)" || die "cancelled."
    else
      read -rsp "The same password again: " again
      echo
    fi
    [ "$ROOT_PW" = "$again" ] && break
    if [ "$WT" = 1 ]; then
      whiptail --backtitle "$TITLE" --title "Swarmy" --msgbox "Those didn't match. Try again." 8 50
    else
      echo "Those didn't match. Try again."
    fi
  done
fi
CONSOLE="signs in as root by itself"
[ -n "$ROOT_PW" ] && CONSOLE="root, with the password you chose"

AUTO_UPDATE="${AUTO_UPDATE:-}"
if [ "$INTERACTIVE" = 1 ] && [ -z "$AUTO_UPDATE" ]; then
  if ask_yesno "Keep the container's system packages up to date by themselves?

Once a day, Debian's unattended-upgrades installs security fixes and
stable updates for the container's packages. The container uses this
host's kernel, so it never needs a reboot for them.

Swarmy itself isn't touched: upgrade it with the command shown at the end." "Turn on automatic updates?"; then
    AUTO_UPDATE=1
  else
    AUTO_UPDATE=0
  fi
fi
[ "$AUTO_UPDATE" = 0 ] || AUTO_UPDATE=1
UPDATES="off"
[ "$AUTO_UPDATE" = 1 ] && UPDATES="system packages update daily, by themselves"

if [ "$INTERACTIVE" = 1 ]; then
  mapped="none"
  [ "${#SHARE_LIST[@]}" -gt 0 ] && mapped="$(printf '/mnt/%s ' "${SHARE_LIST[@]}")"
  ask_yesno "Ready to create the container:

  Container   $CTID, named $NAME
  Size        $CORES cores, $RAM MB memory, $DISK GB disk on $STORAGE
  Network     $BRIDGE, address by DHCP; web UI on port $PORT
  Shares      $mapped
  Downloads   $CT_DL${HOST_DL:+
              ($HOST_DL on this host)}
  Console     $CONSOLE
  Updates     $UPDATES

Create it and install Swarmy?" || die "cancelled; nothing was changed."
fi

# ---------------------------------------------------------------- build

CREATED=0
FINISHED=0
cleanup() {
  local status=$?
  if [ "$status" -ne 0 ] && [ "$CREATED" = 1 ] && [ "$FINISHED" = 0 ]; then
    if [ "${KEEP_ON_FAILURE:-0}" = "1" ]; then
      warn "setup failed; container $CTID is kept for you to look into (pct destroy $CTID removes it)."
    else
      warn "setup failed, so container $CTID is being removed (KEEP_ON_FAILURE=1 keeps it)."
      pct stop "$CTID" >/dev/null 2>&1 || true
      pct destroy "$CTID" --purge >/dev/null 2>&1 || true
    fi
  elif [ "$status" -ne 0 ] && [ "$FINISHED" = 1 ]; then
    warn "Swarmy is installed in container $CTID, but the installer didn't finish its last steps (pct start $CTID if it's stopped)."
    echo "  Username:   admin" >&2
    echo "  Password:   pct exec $CTID -- journalctl -u swarmy | grep password" >&2
    echo "  Address:    pct exec $CTID -- ip -4 addr show eth0   (web UI on port $PORT)" >&2
  fi
}
trap cleanup EXIT
# Never stop without saying where: set -e alone exits silently.
set -E
trap 'warn "the installer stopped at line $LINENO, running: $BASH_COMMAND"' ERR

say "Finding the latest Debian template for $ARCH"
pveam update >/dev/null || true
TEMPLATE=""
for release in 13 12; do
  TEMPLATE="$(pveam available --section system \
    | awk -v r="^debian-$release-standard_.*_$ARCH\\\\." '$2 ~ r { print $2 }' | sort -V | tail -n1)"
  [ -n "$TEMPLATE" ] && break
done
[ -n "$TEMPLATE" ] || die "no Debian 12 or 13 template for $ARCH is available from pveam."
if ! pveam list "$TEMPLATE_STORAGE" | grep -q "$TEMPLATE"; then
  say "Downloading $TEMPLATE to $TEMPLATE_STORAGE"
  pveam download "$TEMPLATE_STORAGE" "$TEMPLATE" >/dev/null
fi

# Make the download folder now, as the host's root: the container may not be
# allowed to create folders on a share.
MADE_DL=0
if [ -n "$HOST_DL" ] && [ ! -d "$HOST_DL" ]; then
  mkdir -p "$HOST_DL" || die "couldn't create $HOST_DL on this host."
  MADE_DL=1
fi

say "Creating container $CTID ($NAME) on $STORAGE"
pct create "$CTID" "$TEMPLATE_STORAGE:vztmpl/$TEMPLATE" \
  --hostname "$NAME" \
  --cores "$CORES" --memory "$RAM" --swap 512 \
  --rootfs "$STORAGE:$DISK" \
  --net0 "name=eth0,bridge=$BRIDGE,ip=dhcp" \
  --unprivileged 1 --features nesting=1 \
  --onboot 1 >/dev/null
CREATED=1

i=0
for s in "${SHARE_LIST[@]}"; do
  pct set "$CTID" "-mp$i" "$(share_path "$s"),mp=/mnt/$s" >/dev/null
  i=$((i + 1))
done
if [ -z "$DL_SHARE" ] && [ -n "$HOST_DL" ]; then
  pct set "$CTID" "-mp$i" "$HOST_DL,mp=$CT_DL" >/dev/null
fi

say "Starting the container"
pct start "$CTID" || die "the container wouldn't start (the error is above). If its disk is on network storage, try STORAGE=local-lvm (or your local storage)."

say "Waiting for the container's network"
for _ in $(seq 1 30); do
  if ct getent hosts github.com >/dev/null 2>&1; then break; fi
  sleep 2
done
ct getent hosts github.com >/dev/null 2>&1 \
  || die "container $CTID has no network or DNS. Check that bridge $BRIDGE hands out addresses by DHCP."

say "Setting up the console"
if [ -n "$ROOT_PW" ]; then
  # Through stdin, so the password never appears in a process list.
  printf 'root:%s\n' "$ROOT_PW" | pct exec "$CTID" -- chpasswd
else
  # shellcheck disable=SC2016 # $TERM is for systemd to expand, not us.
  printf '%s\n' '[Service]' 'ExecStart=' \
    'ExecStart=-/sbin/agetty --autologin root --noclear --keep-baud tty%I 115200,38400,9600 $TERM' \
    | ct sh -c 'mkdir -p /etc/systemd/system/container-getty@1.service.d \
        && cat > /etc/systemd/system/container-getty@1.service.d/autologin.conf \
        && systemctl daemon-reload && systemctl restart container-getty@1.service'
fi

say "Installing Swarmy inside the container"
ct env DEBIAN_FRONTEND=noninteractive bash -c "apt-get update -qq && apt-get install -y -qq curl ca-certificates >/dev/null"

if [ "$AUTO_UPDATE" = 1 ]; then
  say "Turning on automatic updates"
  ct env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq unattended-upgrades >/dev/null
  # apt's own daily timers run it. Debian's default takes security fixes and
  # point releases; add stable updates too, and clear out what's no longer needed.
  printf '%s\n' 'APT::Periodic::Update-Package-Lists "1";' 'APT::Periodic::Unattended-Upgrade "1";' \
    'APT::Periodic::AutocleanInterval "7";' | ct sh -c 'cat > /etc/apt/apt.conf.d/20auto-upgrades'
  # shellcheck disable=SC2016 # ${distro_codename} is for unattended-upgrades to expand.
  printf '%s\n' 'Unattended-Upgrade::Origins-Pattern { "origin=Debian,codename=${distro_codename}-updates"; };' \
    'Unattended-Upgrade::Remove-Unused-Dependencies "true";' | ct sh -c 'cat > /etc/apt/apt.conf.d/52swarmy-upgrades'
fi
ct env SWARMY_PORT="$PORT" SWARMY_DOWNLOADS="$CT_DL" \
  bash -c "curl -fsSL $RAW/install.sh | bash" >/dev/null 2>&1 \
  || die "installing Swarmy in the container failed. Run it there to see why: pct exec $CTID -- bash -c \"curl -fsSL $RAW/install.sh | bash\""
FINISHED=1

# ---------------------------------------------------------------- downloads

can_write() {
  ct runuser -u swarmy -- sh -c \
    "touch '$CT_DL/.swarmy-write-test' && rm -f '$CT_DL/.swarmy-write-test'" >/dev/null 2>&1
}
WRITABLE=1
if [ -n "$HOST_DL" ] && ! can_write; then
  # An unprivileged container's users are shifted by 100000 on the host.
  HOST_UID=$((100000 + $(ct id -u swarmy)))
  HOST_GID=$((100000 + $(ct id -g swarmy)))
  give=0
  if [ "$MADE_DL" = 1 ] || [ "${DOWNLOADS_CHOWN:-0}" = "1" ]; then
    give=1
  elif [ "$INTERACTIVE" = 1 ] && ask_yesno "Swarmy can't write to $HOST_DL yet.

Make the container's swarmy user ($HOST_UID on this host) its owner?
Only that folder changes, not what's in it."; then
    give=1
  fi
  if [ "$give" = 1 ]; then
    chown "$HOST_UID:$HOST_GID" "$HOST_DL" 2>/dev/null || true
  fi
  can_write || WRITABLE=0
fi

# SMB shares are mounted for the host's root only. They can be remounted so
# every container can write: Sonarr and Radarr, in their own containers, need
# to move and delete what Swarmy downloads too, so ownership alone won't do.
if [ "$WRITABLE" = 0 ] && [ "$DL_TYPE" = cifs ]; then
  fix=0
  holds_backups=""
  case ",$(cfg_value "$DL_SHARE" content)," in *,backup,*) holds_backups=1 ;; esac
  if [ "${SMB_FIX:-0}" = "1" ]; then
    fix=1
  elif [ "$INTERACTIVE" = 1 ] && ask_yesno "Swarmy can't write to $DL_SHARE: Proxmox mounts SMB shares so only
this host's root can write, and the container isn't root here.

Remount $DL_SHARE so every container can write to it? That adds
file_mode=0666,dir_mode=0777 to its mount options (other mount options
stay), stops this container for a moment, and needs nothing else using
the share right now.${holds_backups:+

Careful: $DL_SHARE also holds Proxmox backups. Any container could then
change them. A separate share for downloads is better.}"; then
    fix=1
  fi
  if [ "$fix" = 1 ]; then
    mount_path="$(share_path "$DL_SHARE")"
    old="$(cfg_value "$DL_SHARE" options)"
    # grep finds nothing when the share has no other options; that's fine.
    kept="$(printf '%s' "$old" | tr ',' '\n' | { grep -vE '^(file_mode|dir_mode)=' || true; } | paste -sd, -)"
    opts="${kept:+$kept,}file_mode=0666,dir_mode=0777"
    say "Remounting $DL_SHARE with $opts"
    pct stop "$CTID" >/dev/null
    pvesm set "$DL_SHARE" --options "$opts"
    if umount "$mount_path" 2>/dev/null; then
      pvesm status --storage "$DL_SHARE" >/dev/null 2>&1 || true
      for _ in $(seq 1 10); do mountpoint -q "$mount_path" && break; sleep 1; done
      mountpoint -q "$mount_path" || warn "$DL_SHARE didn't mount again by itself; Proxmox remounts it within a minute, or run: pvesm status"
    else
      warn "$mount_path is in use (another container maps it?), so it'll pick up the new options at its next mount, e.g. after a reboot."
    fi
    pct start "$CTID"
    for _ in $(seq 1 15); do ct systemctl is-active -q swarmy 2>/dev/null && break; sleep 1; done
    if can_write; then WRITABLE=1; fi
  fi
fi

# ---------------------------------------------------------------- done

# Neither of these is worth failing over, now Swarmy is installed.
IP="$(ct ip -4 -o addr show scope global 2>/dev/null | awk '{sub(/\/.*/, "", $4); print $4; exit}')" || IP=""
PW="$(ct journalctl -u swarmy --no-pager -o cat 2>/dev/null | sed -n 's/^ *password: *//p' | tail -n1)" || PW=""
echo
say "Swarmy is running in container $CTID ($NAME)"
echo
echo "  Open:       http://${IP:-<container address>}:$PORT"
echo "  Username:   admin"
echo "  Password:   ${PW:-run: pct exec $CTID -- journalctl -u swarmy | grep password}"
echo "  Downloads:  $CT_DL${HOST_DL:+  ($HOST_DL on this host)}"
for s in "${SHARE_LIST[@]}"; do echo "  Mapped:     /mnt/$s  ($(share_path "$s"))"; done
echo
echo "  Upgrade:    pct exec $CTID -- bash -c \"curl -fsSL $RAW/install.sh | bash\""
echo "  Shell:      pct enter $CTID  (the console $CONSOLE)"
if [ "$AUTO_UPDATE" = 1 ]; then
  echo "  Updates:    daily, by themselves. What ran: pct exec $CTID -- cat /var/log/unattended-upgrades/unattended-upgrades.log"
fi

if [ "$WRITABLE" = 0 ]; then
  echo
  warn "Swarmy can't write to $HOST_DL, so downloads will fail until that's sorted."
  case "$DL_TYPE" in
    nfs)
      echo "  The NAS doesn't let the container's user ($HOST_UID on this host) write there. On the NAS, give"
      echo "  that folder to uid $HOST_UID, or set the share to map all users to one that can write (all_squash"
      echo "  with anonuid and anongid)." ;;
    cifs)
      echo "  Proxmox mounts SMB shares so only this host's root can write, and the container isn't root here."
      echo "  Re-run with SMB_FIX=1 to remount it so containers can write, or by hand:"
      echo "    pvesm set $DL_SHARE --options file_mode=0666,dir_mode=0777"
      echo "  then, with nothing using it: umount $(share_path "$DL_SHARE") && pvesm status" ;;
    *)
      echo "  Run: chown $HOST_UID:$HOST_GID $HOST_DL" ;;
  esac
  echo "  Or pick another download folder in Swarmy under Settings > Downloads."
fi
