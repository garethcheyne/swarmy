# Swarmy

A BitTorrent and Usenet client for your server, with a web UI you'd actually want to look at.
It's a drop-in replacement for qBittorrent and SABnzbd in Sonarr, Radarr and Prowlarr setups:
add it as either, and they won't know the difference. No ads, no tracking, ever.

- One small binary with the web UI built in. Runs on Linux (x86-64 and ARM64), in Docker, or in a Proxmox container.
- Torrents: DHT, magnets, queueing, seeding limits, speed schedules, categories that keep their own folders,
  and moving downloads between folders without downloading them again.
- Usenet: NZBs with backup servers, PAR2 repair, and RAR and 7z unpacking.
- A live swarm map, statistics over time, notifications on your phone, and twelve colour themes.

## Install

Pick one. Each prints the address to open and the first sign-in password when it's done.

### Proxmox VE

On the Proxmox host's shell (not inside a container), as root:

```sh
bash -c "$(curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/proxmox.sh)"
```

This creates a small Debian container (2 cores, 1 GB RAM, 8 GB disk) that starts with the host, and
installs Swarmy in it. Set options in front of the command, for example to keep downloads in a folder on the
host that Sonarr and Radarr can also see:

```sh
DOWNLOADS_HOST=/tank/downloads DOWNLOADS_CHOWN=1 bash -c "$(curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/proxmox.sh)"
```

| Option | Default | |
|---|---|---|
| `CTID` | next free ID | container ID |
| `CT_HOSTNAME` | `swarmy` | container name |
| `CORES`, `RAM`, `DISK` | `2`, `1024` (MB), `8` (GB) | container size |
| `STORAGE` | first storage that holds containers | e.g. `local-lvm`, `local-zfs` |
| `BRIDGE` | `vmbr0` | network bridge (the container gets its address by DHCP) |
| `PORT` | `8080` | web UI port |
| `DOWNLOADS_HOST` | none | a host folder to use as the download folder |
| `DOWNLOADS_CHOWN` | off | `1` lets the container write to `DOWNLOADS_HOST` |

### Linux (Debian, Ubuntu, Fedora and other systemd distros)

```sh
curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/install.sh | sudo bash
```

Installs to `/opt/swarmy` as a systemd service called `swarmy`, running as its own `swarmy` user.
Settings and data go in `/var/lib/swarmy`, and downloads in `/var/lib/swarmy/downloads`. Options go after `sudo`:

```sh
curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/install.sh | sudo SWARMY_PORT=9090 SWARMY_DOWNLOADS=/srv/downloads bash
```

Needs glibc 2.36 or newer (Debian 12, Ubuntu 22.04, Fedora 37 or later); use Docker on anything older.

### Docker

```sh
mkdir swarmy && cd swarmy
curl -fsSLO https://raw.githubusercontent.com/garethcheyne/swarmy/main/deploy/docker-compose.yml
docker compose up -d
docker compose logs swarmy | grep -A2 password
```

Or on its own:

```sh
docker run -d --name swarmy --restart unless-stopped -p 8080:8080 -p 6881:6881 -p 6881:6881/udp \
  -v ./config:/config -v ./downloads:/downloads ghcr.io/garethcheyne/swarmy:latest
```

The [compose file](deploy/docker-compose.yml) also shows how to run Swarmy behind a VPN container such as gluetun.

### Upgrade and remove

| | Linux and Proxmox | Docker |
|---|---|---|
| Upgrade | run the Linux install command again (on Proxmox: `pct exec <CTID> -- bash -c "curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/install.sh \| bash"`) | `docker compose pull && docker compose up -d` |
| Remove | `curl -fsSL https://raw.githubusercontent.com/garethcheyne/swarmy/main/install/install.sh \| sudo SWARMY_UNINSTALL=1 bash` (keeps your data and downloads) | `docker compose down` |

## First steps

1. Open the address the installer printed (port 8080) and sign in as `admin` with the password it showed.
   Lost it? `journalctl -u swarmy | grep password`, or `docker logs swarmy | grep password`.
2. Change the password in **Settings → Web UI and security**.
3. Set your download folder, and optionally a separate folder for unfinished downloads, in **Settings → Downloads**.
   Every setting has a ⓘ explaining what it does.

## Sonarr, Radarr and Prowlarr

Create an API key in **Settings → Sonarr and Radarr**; the page walks through each step.

- **Torrents:** add a **qBittorrent** download client with Swarmy's host and port, and your Swarmy username and
  password (or the API key). Swarmy reports itself as qBittorrent 5.1 (Web API 2.11.4).
- **Usenet:** add a **SABnzbd** download client with Swarmy's host and port, and the API key.

Sonarr, Radarr and Swarmy need to see downloads at the same path. If they don't (different containers or machines),
add a remote path mapping in Sonarr and Radarr.

<details>
<summary>What the qBittorrent and SABnzbd compatibility covers</summary>

The qBittorrent Web API lives at `/api/v2`, written against Sonarr's own qBittorrent client code: qBittorrent 5
state strings, `save_path` / `content_path` / `root_path`, per-torrent share limits (`ratioLimit`,
`seedingTimeLimit`, `inactiveSeedingTimeLimit`), categories, tags, queue priority and force start. Magnet adds
return immediately while metadata resolves. Moves work the qBittorrent way: `setLocation`, `setAutoManagement`
(torrents follow their category's folder, so Sonarr's post-import category moves them), an incomplete folder
(`temp_path`), and the `moving` state while files move, which Sonarr treats as still downloading. Not yet
implemented: `sync/maindata` (used by third-party web UIs), trackers and peers detail, file priorities.

The SABnzbd API lives at `/api` and `/sabnzbd/api`: version, config, queue, history, add by file or URL,
pause and resume, and categories. See [design/sabnzbd-compat.md](design/sabnzbd-compat.md).

</details>

## Configuration

Almost everything is on the **Settings** page, saved in `<data dir>/swarmy.json`. A few things can be set when
Swarmy starts, as flags or environment variables (the Linux installer keeps them in `/etc/swarmy/swarmy.env`).
Connection options given this way override and lock the matching setting in the UI.

| Flag / env | Default | |
|---|---|---|
| `--data-dir` / `SWARMY_DATA_DIR` | `~/.local/share/swarmy` | settings, statistics, resume data |
| `--download-dir` / `SWARMY_DOWNLOAD_DIR` | `~/Downloads` | default save folder |
| `--web-addr` / `SWARMY_WEB_ADDR` | `0.0.0.0:8080` | web UI and API |
| `--bt-port` / `SWARMY_BT_PORT` | `6881` | incoming peers (TCP and uTP) |
| `--bind-interface` / `SWARMY_BIND_INTERFACE` | none | e.g. `wg0`: torrent traffic stops instead of leaking if the VPN drops |
| `--upnp` / `SWARMY_UPNP` | off | UPnP port forwarding |
| `--password` / `SWARMY_PASSWORD` | generated | set or reset the web UI password |
| `--demo` / `SWARMY_DEMO` | off | simulated torrents and 30 days of sample statistics |

## Building from source

You need Rust (stable), Node.js 24 and pnpm.

```sh
cd web && pnpm install && pnpm build && cd ..
cargo build --release
./target/release/swarmy --download-dir ~/Downloads/torrents
```

`docker build -t swarmy .` builds the image from source instead.

For development, run the daemon and the Vite dev server side by side; Vite proxies `/api` (including the
WebSocket) to the daemon:

```sh
cargo run -- --password dev          # :8080
cd web && pnpm dev                   # :5173 with hot reload
```

`cargo run -- --demo --password demo` runs a simulated engine (24 torrents that download, stall, finish and seed)
with 30 days of backfilled statistics, so every screen has data. It touches no network and keeps its own data
folder, so real statistics are never mixed in.

```text
crates/engine   torrent engine: the Engine trait, librqbit, the demo simulator, moving files
crates/usenet   Usenet: NNTP, NZB, yEnc, PAR2 repair, unpacking
crates/server   the swarmy daemon: auth, JSON API, WebSocket, qBittorrent and SABnzbd APIs, embedded UI
web/            the web UI (React, Vite, shadcn/ui); pnpm build → web/dist, embedded by the server
install/        the Linux and Proxmox installers
```

Versions are `yyyy.mm.dd.build`: the release date, then which release of that day it is (`2026.11.05.1`,
`2026.11.05.2`…). To release, add the version's notes at the top of [CHANGELOG.md](CHANGELOG.md) and push a
matching tag (`v2026.11.05.1`). [The release workflow](.github/workflows/release.yml) builds the Linux binaries,
publishes the GitHub release the installers download, and pushes the Docker image. A tag that doesn't match the
changelog fails the build before anything is published.

## Licence

Swarmy is free and open source, under your choice of the [MIT](LICENSE-MIT) or
[Apache 2.0](LICENSE-APACHE) licence. It has no ads and no tracking, and never will.

RAR unpacking uses RARLAB's UnRAR source code, which comes with its own condition: it may
be used to extract RAR archives but not to create them. That condition and the other
third-party credits are in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

The licences cover the code, not the name "Swarmy" or its logo. If you ship a fork,
please give it its own name.
