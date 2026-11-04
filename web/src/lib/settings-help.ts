/**
 * Plain-language explanations behind the ⓘ next to each setting: what it
 * does, what changing it does to your downloads, and anything worth knowing
 * before you touch it. Written for people who aren't BitTorrent experts.
 */
export interface HelpTopic {
  title: string
  /** What the setting does. */
  what: string
  /** What changes when you change it. */
  effect: string
  /** Advice, a sensible value, or a pitfall. */
  tip?: string
}

export const SETTINGS_HELP = {
  save_path: {
    title: "Default save folder",
    what: "The folder new downloads are saved in, on the machine running Swarmy (not the device you're browsing from).",
    effect: "Torrents with a category go into a subfolder named after it, unless the category has its own folder. Changing this only affects new torrents; existing ones stay where they are.",
    tip: "In Docker, this is a path inside the container (usually /downloads). Sonarr and Radarr must see the same files at the same path, or need a remote path mapping.",
  },
  add_paused: {
    title: "Add torrents paused",
    what: "New torrents are added stopped, waiting for you to start them.",
    effect: "Nothing downloads until you press Resume. This includes torrents sent by Sonarr and Radarr, which will sit waiting too.",
    tip: "Leave this off unless you want to check every download before it starts.",
  },

  queue: {
    title: "Queue torrents",
    what: "Caps how many torrents run at the same time. The rest wait in line and start, oldest first, when a slot frees up.",
    effect: "Fewer torrents at once means each one finishes sooner, your disk does less jumping around, and your router handles fewer connections. Queued torrents don't connect to anyone, so they don't download or seed while they wait.",
    tip: "Useful on slower connections or disks. Force start skips the queue for a single torrent. Sonarr and Radarr can reorder the queue.",
  },
  queue_downloads: {
    title: "Active downloads",
    what: "How many torrents may download at the same time.",
    effect: "Raising it splits your bandwidth across more torrents, so each takes longer. Lowering it gets individual downloads done sooner.",
    tip: "3 to 5 suits most home connections.",
  },
  queue_uploads: {
    title: "Active uploads",
    what: "How many finished torrents may seed at the same time.",
    effect: "More seeding slots help more people and raise your ratio overall, but each torrent gets a smaller share of your upload speed.",
    tip: "5 to 10 is typical.",
  },
  queue_total: {
    title: "Active in total",
    what: "An overall cap on downloading and seeding torrents together.",
    effect: "Whichever limit is reached first applies. With 4 downloads and 6 uploads but 8 in total, you'll never have more than 8 running.",
  },
  ratio_limit: {
    title: "Share ratio limit",
    what: "Ratio is how much you've uploaded divided by the torrent's size. A ratio of 2 means you've shared it twice over.",
    effect: "When a torrent reaches this ratio it stops seeding, and the 'Then' action below happens. Sonarr and Radarr read this limit to decide when a finished download can be cleaned up.",
    tip: "Private trackers often require a minimum ratio or seeding time. Check their rules: stopping too early can count as a 'hit and run'. Sonarr, Radarr and other qBittorrent-API tools can set a different limit for a single torrent.",
  },
  seeding_time: {
    title: "Seeding time limit",
    what: "How long a torrent keeps seeding after it finishes downloading, in minutes.",
    effect: "After this long it stops seeding and the 'Then' action happens, whatever its ratio.",
    tip: "1440 minutes is a day; 4320 is three days, a common private tracker minimum. If both a ratio and a time are set, whichever comes first applies.",
  },
  inactive_time: {
    title: "Inactivity limit",
    what: "Stops seeding a torrent that hasn't uploaded anything for this long, in minutes.",
    effect: "Torrents nobody is asking for stop taking up a seeding slot, so ones people want get it instead.",
    tip: "Handy alongside queueing. 720 minutes is 12 hours.",
  },
  seed_action: {
    title: "What happens when a limit is reached",
    what: "What Swarmy does to a torrent once it hits a ratio, seeding time or inactivity limit.",
    effect: "Pause keeps the torrent and its files; it just stops seeding. Remove takes it out of Swarmy but leaves the files. Remove with files deletes the downloaded data too, and can't be undone.",
    tip: "Pause is the safe choice, and works best with Sonarr and Radarr: they remove the torrent themselves once they've imported it. Files Sonarr or Radarr hardlinked into your library survive either way.",
  },

  download_limit: {
    title: "Download limit",
    what: "The most Swarmy will download per second, across all torrents together. 0 means no limit.",
    effect: "Leaves bandwidth for everything else on your network. All running torrents share this limit.",
    tip: "1024 KiB/s is 1 MiB/s, about 8.4 Mbit/s. To leave room for streaming or calls, use about 80% of your line's download speed.",
  },
  upload_limit: {
    title: "Upload limit",
    what: "The most Swarmy will upload per second, across all torrents. 0 means no limit.",
    effect: "Peers send more to people who send back, so a very low upload limit also slows your downloads. An unlimited upload can fill your connection and make everything else on it laggy.",
    tip: "Set it to about 80% of your line's upload speed: plenty for peers, with headroom for everything else.",
  },
  alt_limits: {
    title: "Alternative limits",
    what: "A second, usually lower, pair of limits you can switch to: by hand with the turtle in the top bar, or on a schedule.",
    effect: "While they're on, they replace the normal limits. Switching back restores the normal ones straight away.",
    tip: "Set these for when you need the connection, like evenings or work hours, and let the schedule switch them for you.",
  },
  schedule: {
    title: "Alternative limits schedule",
    what: "Turns the alternative limits on between two times on the days you pick.",
    effect: "It runs on the clock of the machine running Swarmy. A start later than the end (22:00 to 07:00) runs overnight.",
  },

  listen_port: {
    title: "Listening port",
    what: "The port other peers use to connect to you, over TCP and uTP.",
    effect: "If peers can reach this port you're 'connectable': you can trade with far more of the swarm, and seed to people who are themselves behind a router. If they can't, you only reach peers that accept incoming connections.",
    tip: "Forward this port on your router, or use UPnP. On a VPN with port forwarding, put the port your VPN gives you here. Takes effect after a restart.",
  },
  upnp: {
    title: "UPnP port forwarding",
    what: "Asks your router to forward the listening port to Swarmy automatically.",
    effect: "Saves you forwarding the port by hand, if your router supports UPnP and has it switched on.",
    tip: "It does nothing through a VPN. Some people keep UPnP off on their router for security and forward the port by hand instead.",
  },
  bind_interface: {
    title: "Network interface",
    what: "Ties all torrent traffic to one network interface, such as your VPN's (wg0 or tun0).",
    effect: "Torrent traffic can only leave through that interface. If the VPN drops, torrents stop instead of carrying on over your normal connection. The web UI isn't affected.",
    tip: "This is the most reliable kill switch. Find the name with 'ip a' on Linux. Takes effect after a restart.",
  },
  ipv4_only: {
    title: "IPv4 only",
    what: "Connect to peers over IPv4 only, never IPv6.",
    effect: "You may see slightly fewer peers. It stops IPv6 traffic going around a VPN that only tunnels IPv4, a common leak.",
    tip: "Turn it on if your VPN doesn't support IPv6, or if your IPv6 connection is unreliable.",
  },
  dht: {
    title: "DHT",
    what: "A shared directory, run by all BitTorrent users together, that finds peers without needing a tracker.",
    effect: "Needed for magnet links that have no tracker, and finds extra peers for everything else. Turning it off can leave magnet links stuck waiting for metadata.",
    tip: "Torrents from private trackers never use DHT, even with this on, as their rules require.",
  },
  lsd: {
    title: "Local peer discovery",
    what: "Looks for other BitTorrent clients on your own network sharing the same torrents.",
    effect: "If another computer at home has the same torrent, you can swap pieces at full local network speed.",
    tip: "Harmless to leave on. Private torrents don't use it.",
  },
  peer_limit: {
    title: "Maximum peers per torrent",
    what: "How many peers each torrent may be connected to at once. 0 uses the engine's default.",
    effect: "More connections can mean faster downloads on busy torrents, but use more memory and CPU. Some cheap home routers struggle with thousands of connections.",
    tip: "Leave it at 0 unless your router struggles, then try 50 to 100.",
  },
  socks_proxy: {
    title: "SOCKS5 proxy",
    what: "Sends outgoing peer connections through a SOCKS5 proxy, such as one your VPN provider offers.",
    effect: "Peers see the proxy's address instead of yours. Peers can't connect to you through it, so you'll be less connectable and may see fewer peers.",
    tip: "Binding to a VPN interface is usually better: it covers everything, and fails closed if the VPN drops.",
  },
  blocklist: {
    title: "IP blocklist",
    what: "A downloadable list of address ranges Swarmy refuses to connect to, in P2P or DAT format, gzipped or plain.",
    effect: "Peers on the list are refused. It's loaded when Swarmy starts.",
    tip: "A blocklist doesn't make you anonymous: anyone not on it can still see your address. A VPN does that job.",
  },

  qbit_api: {
    title: "qBittorrent-compatible API",
    what: "Answers the same web API as qBittorrent at /api/v2, so Sonarr, Radarr, Prowlarr and similar tools can use Swarmy as a qBittorrent download client.",
    effect: "With it off, those tools can't add or manage torrents. The Swarmy web UI keeps working.",
  },
  session_days: {
    title: "Stay signed in for",
    what: "How long a sign-in lasts before you have to sign in again on that device.",
    effect: "Longer is more convenient; shorter limits the damage if a device is lost. It applies to new sign-ins, not ones that already exist.",
  },
  lockout: {
    title: "Sign-in lockout",
    what: "After this many wrong passwords from one address, that address can't try again for a while.",
    effect: "It stops anyone guessing passwords. You can't lock yourself out of other devices: the lockout is per address.",
    tip: "Behind a reverse proxy, add the proxy under 'Trusted reverse proxies', or every visitor will share the proxy's address and lockout.",
  },
  trusted_proxies: {
    title: "Trusted reverse proxies",
    what: "Reverse proxies (like Nginx, Caddy or Traefik) allowed to tell Swarmy who the real visitor is, and whether they used HTTPS.",
    effect: "Lockouts then apply to the real visitor rather than the proxy, and sign-in cookies are marked secure when the proxy serves HTTPS. Requests from anywhere else can't fake these headers.",
    tip: "Use the proxy's address as Swarmy sees it, such as 127.0.0.1 on the same machine or 172.16.0.0/12 for Docker networks.",
  },
  peer_locations: {
    title: "Peer locations",
    what: "Places your peers on the Swarm map using DB-IP's free city database, which Swarmy downloads (about 60 MB) and keeps up to date.",
    effect: "Lookups happen on your server and peer addresses are never sent anywhere. Turning it off releases the database and stops the monthly download.",
    tip: "Locations are approximate, typically to the city. Peers behind a VPN show the VPN's location.",
  },
} satisfies Record<string, HelpTopic>

export type HelpId = keyof typeof SETTINGS_HELP
