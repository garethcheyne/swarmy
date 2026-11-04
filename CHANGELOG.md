# Changelog

Every Swarmy release, newest first. This file is built into Swarmy itself:
it is what you see under **Settings → About** and in the "What's new" window
after an upgrade, so write entries for the people using Swarmy, not for its
developers.

Each release is a `## [version] - date` heading (or `- Unreleased`), an
optional one-line summary, then `### New`, `### Improved` and `### Fixed`
lists.

## [0.1.0] - Unreleased

The first Swarmy: a BitTorrent client with a web UI you'd actually want to look at, and no ads, ever.

### New
- A fast BitTorrent engine (librqbit) with DHT, local peer discovery, UPnP port forwarding, magnet links and `.torrent` files.
- Works with Sonarr, Radarr and Prowlarr: add Swarmy as a qBittorrent download client and they won't know the difference.
- A dense, keyboard-friendly torrent table, with an inspector showing each torrent's files, peers, category, tags and a live piece map.
- Statistics: throughput, daily volume, busiest hours, ratio and disk space, recorded over time.
- Speed limits, an alternative "turtle" speed and a schedule for it.
- Queueing, force start, and seeding limits by ratio, seeding time or inactivity, globally or per torrent.
- Categories and tags with their own save folders.
- Twelve colour themes, each with light and dark modes.
- Install it as an app (PWA) on your phone or desktop, with notifications when downloads finish or fail.
- A mini view that floats on top of your other windows.
- A command palette: press Ctrl+K anywhere.
- Secure sign-in with lockout after failed attempts, session management and API keys, ready to sit behind a reverse proxy.
- Docker image, a Linux installer and a Proxmox LXC installer.
- An animated sign-in screen where the swarm assembles the Swarmy mark.
- This changelog, built into the app.
