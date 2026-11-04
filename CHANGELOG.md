# Changelog

Every Swarmy release, newest first. This file is built into Swarmy itself:
it is what you see under **Settings → About** and in the "What's new" window
after an upgrade, so write entries for the people using Swarmy, not for its
developers.

Each release is a `## [version] - date` heading (or `- Unreleased`), an
optional one-line summary, then `### New`, `### Improved` and `### Fixed`
lists. Versions are `yyyy.mm.dd.build`: the release date, then 1 for the
first release that day, 2 for the next, and so on. The top entry is the
version Swarmy reports, and a release tag must match it.

## [2026.11.05.7] - 2026-11-05

### Improved
- Swarmy now checks for a new release every hour instead of every six, so you hear about one sooner without opening Settings.
- An "Update available" button in the top bar when there's a new release, on phones too. It opens Settings → About.

## [2026.11.05.6] - 2026-11-05

### Fixed
- The notes for an available update could show its release date a day off, depending on your time zone.

## [2026.11.05.5] - 2026-11-05

Know when there's a new Swarmy, and install it from Settings.

### New
- Swarmy checks for new releases every few hours. When there is one, the status bar says so, and Settings → About shows what's new in every version since yours.
- Install updates with one click on Linux and Proxmox: Swarmy downloads the new version, checks it, installs it and restarts, and the page reloads by itself. If anything goes wrong, Settings → About shows what the updater said.
- Docker users get the command to pull the new image instead.
- Turn the checks off in Settings → About if you'd rather not.

### Improved
- Installed with the Linux or Proxmox installer before this version? Run the install command once more to turn on one-click updates; after that, updates come from Settings.

## [2026.11.05.4] - 2026-11-05

Give each app its own API key.

### New
- As many API keys as you like, each with a name saying what it's for (Sonarr, Radarr, a script…). Revoke one and the others keep working.
- Keys can expire: in 30 or 90 days, a year, or on a date you pick. Expired keys stop working and are marked, so you can see which app needs a new one.
- Each key shows when it was last used, and its last four characters to match it with the app it's in.
- Rename a key at any time.

### Improved
- Your existing API key keeps working. It's now listed as "API key"; rename it after the app that uses it.

## [2026.11.05.3] - 2026-11-05

### New
- Proxmox installer: asks whether to keep the container's system packages up to date by themselves (daily security fixes and stable updates, using Debian's unattended-upgrades). On by default; `AUTO_UPDATE=0` turns it off.

### Fixed
- Proxmox installer: could stop without showing the address and password after remounting an SMB share that had no mount options. If anything stops it now, it says where, and how to sign in.

## [2026.11.05.2] - 2026-11-05

Pick folders by browsing the server, and know straight away if Swarmy can write there.

### New
- A folder browser on every folder setting (download, unfinished, category and Usenet folders, and Move). Browse the server's folders and mounted shares, make a new folder, and see whether Swarmy can write to it before you pick it.
- Swarmy checks it can write to a folder before saving it, and says why not (with the user it runs as) if it can't.
- A warning in the status bar and on Settings → Downloads if the download folder stops being writable, for example a share that came unmounted.
- Proxmox installer: asks for a root password for the container's console, or signs the console in by itself if you leave it empty.
- Proxmox installer: if the download share is an SMB share the container can't write to, it offers to remount it so containers can (and warns if that share also holds backups).

### Fixed
- The download folder set by the Linux installer or Docker could no longer be changed in Settings. It's now only the default.
- Proxmox installer: no more locale warnings from inside the new container.

## [2026.11.05.1] - 2026-11-05

The first Swarmy: a BitTorrent client with a web UI you'd actually want to look at, and no ads, ever. Free and open source under MIT or Apache-2.0.

### New
- A fast BitTorrent engine (librqbit) with DHT, local peer discovery, UPnP port forwarding, magnet links and `.torrent` files.
- Works with Sonarr, Radarr and Prowlarr: add Swarmy as a qBittorrent download client and they won't know the difference.
- A dense, keyboard-friendly torrent table, with an inspector showing each torrent's files, peers, category, tags and a live piece map.
- Usenet: download NZBs from your news server, with backup servers for missing articles, automatic PAR2 repair, RAR and 7z unpacking, and a queue that survives restarts.
- Works with Sonarr and Radarr for Usenet too: add Swarmy as a SABnzbd download client.
- Statistics: throughput, daily volume, busiest hours, ratio and disk space, recorded over time.
- Speed limits, an alternative "turtle" speed and a schedule for it.
- Queueing, force start, and seeding limits by ratio, seeding time or inactivity, globally or per torrent.
- Categories and tags with their own save folders.
- Move torrents to another folder (one, or a whole selection) and they carry on seeding from there without downloading anything again. Moves survive a crash or power cut halfway through, and never overwrite anything.
- An optional folder for unfinished downloads: torrents move to their save folder when complete, so Sonarr, Radarr and your library only see finished files.
- Torrents follow their category: change the category, or the category's folder, and the files move to match. This works with Sonarr and Radarr's post-import category.
- Twelve colour themes, each with light and dark modes.
- Install it as an app (PWA) on your phone or desktop, with notifications when downloads finish or fail.
- A mini view that floats on top of your other windows.
- A command palette: press Ctrl+K anywhere.
- Secure sign-in with lockout after failed attempts, session management and API keys, ready to sit behind a reverse proxy.
- Docker image, a Linux installer and a Proxmox LXC installer.
- A live swarm map: where your peers are, with pieces flowing to and from you. Peer locations are looked up on your own server, from a free database Swarmy downloads and keeps up to date, so peer addresses are never sent anywhere.
- Charts that draw themselves in, numbers that count up, and a piece map that lights up as pieces finish.
- An animated sign-in screen where the swarm assembles the Swarmy mark.
- Every setting is explained: a plain description, and an ⓘ with what it does, what changing it affects, and a sensible value.
- This changelog, built into the app.
