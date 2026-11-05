//! Commands run from a shell instead of starting the server.
//!
//! `swarmy reset-password` sets a new web UI password when nobody can sign in.
//! Anyone who can run it already controls Swarmy's data folder, so it needs no
//! other proof. It refuses to run while Swarmy is running, because the server
//! keeps the account in memory and would write over the change.

use std::{
    io::{BufRead, IsTerminal},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
    path::Path,
    time::Duration,
};

use anyhow::{Context, bail};
use clap::Subcommand;

use crate::{auth, config::Config, store::Store};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Set a new password for the web UI and sign everyone out.
    ///
    /// Reads the new password from standard input (one line). Leave it empty
    /// for a random one, which is printed. Stop Swarmy first; on Linux installs
    /// `sudo swarmy-reset-password` does that for you.
    ResetPassword {
        /// Change the username as well.
        #[arg(long)]
        username: Option<String>,
    },
}

pub fn run(cfg: &Config, cmd: &Command) -> anyhow::Result<()> {
    match cmd {
        Command::ResetPassword { username } => reset_password(cfg, username.as_deref()),
    }
}

fn reset_password(cfg: &Config, new_username: Option<&str>) -> anyhow::Result<()> {
    // The web port answering means Swarmy (or something) is running there.
    let probe = match cfg.web_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => SocketAddr::new(Ipv4Addr::LOCALHOST.into(), cfg.web_addr.port()),
        IpAddr::V6(ip) if ip.is_unspecified() => SocketAddr::new(Ipv6Addr::LOCALHOST.into(), cfg.web_addr.port()),
        _ => cfg.web_addr,
    };
    if TcpStream::connect_timeout(&probe, Duration::from_secs(1)).is_ok() {
        bail!(
            "Swarmy seems to be running: something answers on {probe}. Stop it first \
             (systemctl stop swarmy, or docker compose stop), or use swarmy-reset-password, which does that for you."
        );
    }

    let data_dir = cfg.data_dir();
    let store_path = data_dir.join("swarmy.json");
    anyhow::ensure!(
        store_path.exists(),
        "There's no Swarmy data in {}. Point SWARMY_DATA_DIR (or --data-dir) at the folder Swarmy uses.",
        data_dir.display()
    );
    refuse_wrong_owner(&store_path)?;

    if std::io::stdin().is_terminal() {
        eprintln!("Type the new password and press Enter (it will show as you type). Leave it empty for a random one:");
    }
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).context("reading the new password")?;
    let typed = line.trim_end_matches(['\r', '\n']).to_owned();
    let (password, generated) = if typed.is_empty() {
        (auth::random_password(), true)
    } else {
        (typed, false)
    };
    anyhow::ensure!(password.chars().count() >= 8, "Use at least 8 characters. Nothing was changed.");
    if let Some(u) = new_username {
        anyhow::ensure!(!u.trim().is_empty(), "The username can't be empty. Nothing was changed.");
    }

    let store = Store::open(store_path)?;
    let username = auth::reset_account(&store, new_username.map(str::trim), &cfg.username, &password)?;
    println!("Swarmy's password has been changed. Everyone who was signed in has been signed out.");
    println!("  Username: {username}");
    if generated {
        println!("  Password: {password}");
    }
    Ok(())
}

/// Running as root over files that belong to Swarmy's user would leave the
/// settings owned by root, and Swarmy unable to save them.
fn refuse_wrong_owner(file: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let me = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(u32::MAX);
        let owner = std::fs::metadata(file)?.uid();
        if me != owner {
            bail!(
                "{} belongs to user {owner}, but this is running as user {me}. Run it as Swarmy's user \
                 (sudo -u swarmy swarmy reset-password), or use sudo swarmy-reset-password.",
                file.display()
            );
        }
    }
    let _ = file;
    Ok(())
}
