use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;

/// Swarmy — a BitTorrent daemon with a modern web UI and a
/// qBittorrent-compatible API for Sonarr/Radarr/Prowlarr.
#[derive(Debug, Parser)]
#[command(version = env!("SWARMY_VERSION"), about)]
pub struct Config {
    /// Directory for Swarmy's own state (settings, metadata, resume data).
    #[arg(long, env = "SWARMY_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Default download location, used until another is chosen in Settings.
    #[arg(long, env = "SWARMY_DOWNLOAD_DIR")]
    pub download_dir: Option<PathBuf>,

    /// Address the web UI and API listen on.
    #[arg(long, env = "SWARMY_WEB_ADDR", default_value = "0.0.0.0:8080")]
    pub web_addr: SocketAddr,

    // Connection flags are optional: when given they override (and lock) the
    // matching setting on the Settings page; otherwise the saved setting is used.

    /// Port for incoming BitTorrent connections (TCP and uTP).
    #[arg(long, env = "SWARMY_BT_PORT")]
    pub bt_port: Option<u16>,

    /// Bind all torrent traffic to this network interface (e.g. wg0, tun0).
    /// If the interface goes down, torrent traffic stops instead of leaking.
    #[arg(long, env = "SWARMY_BIND_INTERFACE")]
    pub bind_interface: Option<String>,

    /// Try to forward the BitTorrent port with UPnP.
    #[arg(long, env = "SWARMY_UPNP", num_args = 0..=1, default_missing_value = "true")]
    pub upnp: Option<bool>,

    /// Only use IPv4.
    #[arg(long, env = "SWARMY_IPV4_ONLY", num_args = 0..=1, default_missing_value = "true")]
    pub ipv4_only: Option<bool>,

    /// Web UI username. Only used when no account exists yet, or with --password.
    #[arg(long, env = "SWARMY_USERNAME", default_value = "admin")]
    pub username: String,

    /// Set (or reset) the web UI password. If no password has ever been set,
    /// a random one is generated and printed to the log.
    #[arg(long, env = "SWARMY_PASSWORD", hide_env_values = true)]
    pub password: Option<String>,

    /// Contact for push services (VAPID "sub"). Apple requires a real mailto: or https: URL.
    #[arg(long, env = "SWARMY_PUSH_CONTACT", default_value = "mailto:admin@localhost")]
    pub push_contact: String,

    /// Run with a simulated engine and sample data instead of real torrents.
    /// Uses its own data directory (`<data dir>/demo`), so real stats are untouched.
    #[arg(long, env = "SWARMY_DEMO", default_value_t = false)]
    pub demo: bool,

    /// Run a command instead of starting Swarmy.
    #[command(subcommand)]
    pub command: Option<crate::cli::Command>,
}

impl Config {
    pub fn data_dir(&self) -> PathBuf {
        let base = self.data_dir.clone().unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("swarmy")
        });
        if self.demo { base.join("demo") } else { base }
    }

    pub fn download_dir(&self) -> PathBuf {
        self.download_dir.clone().unwrap_or_else(|| {
            dirs::download_dir().unwrap_or_else(|| self.data_dir().join("downloads"))
        })
    }
}
