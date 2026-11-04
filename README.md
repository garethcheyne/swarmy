# Swarmy

A BitTorrent daemon for Linux with a modern web UI, designed as a drop-in
replacement for qBittorrent in Sonarr / Radarr / Prowlarr setups.

- **Backend:** Rust (axum + [librqbit](https://github.com/ikatson/rqbit)), single binary with the UI embedded
- **Frontend:** React + Vite + shadcn/ui (`web/`)

## Layout

```
crates/engine   Torrent engine abstraction (`Engine` trait) + librqbit implementation
crates/server   The `swarmy` daemon: auth, JSON API, WebSocket, metadata store, embedded UI
web/            Web UI (Vite SPA); `pnpm build` → web/dist, embedded by the server
```

## Running

```sh
cd web && pnpm install && pnpm build && cd ..
cargo run --release -- --download-dir ~/Downloads/torrents
```

Open http://localhost:8080. On first start a password for `admin` is generated
and printed in the log; set your own with `--password` (or `SWARMY_PASSWORD`).

| Flag / env | Default | |
|---|---|---|
| `--data-dir` / `SWARMY_DATA_DIR` | `~/.local/share/swarmy` | settings, metadata, resume data |
| `--download-dir` / `SWARMY_DOWNLOAD_DIR` | `~/Downloads` | default save path |
| `--web-addr` / `SWARMY_WEB_ADDR` | `0.0.0.0:8080` | web UI + API |
| `--bt-port` / `SWARMY_BT_PORT` | `6881` | incoming peers (TCP + uTP) |
| `--bind-interface` / `SWARMY_BIND_INTERFACE` | – | e.g. `wg0`; torrent traffic can't leak if the VPN drops |
| `--upnp` / `SWARMY_UPNP` | off | UPnP port forwarding |
| `--demo` / `SWARMY_DEMO` | off | simulated torrents + 30 days of sample stats, in `<data dir>/demo` |

Everything else lives on the **Settings** page (saved in `<data dir>/swarmy.json`): downloads, connection
(DHT, LSD, peer limit, SOCKS5 proxy, blocklist), speed limits with an alternative-speed schedule, queueing,
seeding limits, categories and tags, account, session length, login lockout and trusted reverse proxies.
Connection flags above are optional; when given they override and lock the matching setting.

## Demo mode

```sh
cargo run -- --demo --password demo
```

Runs a simulated engine (24 torrents that download, stall, finish and seed) and
backfills 30 days of statistics, so every screen has data. Nothing touches the
network, and it uses its own data directory, so real stats are never mixed in.

## Statistics

Every second the daemon samples throughput, peers, active torrents and its own
CPU/memory; the last 10 minutes stay in memory for live charts and per-minute
aggregates go to `stats.db` (SQLite) forever. Torrent adds, completions and
removals are logged as events. API: `/api/stats/{live,history,summary,events}`.

## Sonarr, Radarr and Prowlarr

Swarmy serves the qBittorrent Web API at `/api/v2` and reports itself as qBittorrent 5.1 (Web API 2.11.4).
In Sonarr or Radarr add a **qBittorrent** download client pointing at Swarmy's host and port, with your
Swarmy username and password, or an API key from **Settings → Sonarr and Radarr**. The layer was written
against Sonarr's own qBittorrent client code: qBittorrent 5 state strings, `save_path` / `content_path` /
`root_path`, per-torrent share limits (`ratioLimit`, `seedingTimeLimit`, `inactiveSeedingTimeLimit`),
categories, tags, queue priority and force start. Magnet adds return immediately while metadata resolves.

Not yet implemented: `sync/maindata` (used by third-party web UIs), trackers/peers detail, `setLocation`,
file priorities.

## Development

Run the daemon and the Vite dev server side by side; Vite proxies `/api`
(including the WebSocket) to the daemon:

```sh
cargo run -- --password dev          # :8080
cd web && pnpm dev                   # :5173 with hot reload
```

## Roadmap

1. ✅ Daemon, engine adapter, persistence, auth, JSON API, live WebSocket updates, web UI
2. ✅ qBittorrent Web API v2 compatibility layer (`/api/v2/*`) so Sonarr/Radarr/Prowlarr can use Swarmy as a qBittorrent client
3. UI: settings page, speed limits, category management
   (✅ done early: Hive overview, statistics, demo mode)
4. RSS rules, scheduler, Docker image

## Licence

Swarmy is free and open source, under your choice of the [MIT](LICENSE-MIT) or
[Apache 2.0](LICENSE-APACHE) licence. It has no ads and no tracking, and never will.

RAR unpacking uses RARLAB's UnRAR source code, which comes with its own condition: it may
be used to extract RAR archives but not to create them. That condition and the other
third-party credits are in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

The licences cover the code, not the name "Swarmy" or its logo. If you ship a fork,
please give it its own name.
