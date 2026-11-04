#![recursion_limit = "256"]

mod api;
mod auth;
mod changelog;
mod config;
mod geo;
mod mover;
mod policy;
mod push;
mod peermap;
mod qbit;
mod sab;
mod state;
mod stats;
mod store;
mod web;
mod usenet;
mod ws;

use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use clap::Parser;
use swarmy_engine::{Engine, RqbitConfig, RqbitEngine, SimEngine};
use tokio::sync::watch;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

use crate::{
    auth::Auth,
    config::Config,
    state::{AppState, Snapshot},
    stats::Stats,
    store::{Store, TorrentMeta, now_unix},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Colours only on a terminal: journalctl and `docker logs` would show the escape codes.
    tracing_subscriber::fmt()
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,librqbit=warn,librqbit_dht=warn")),
        )
        .init();

    let cfg = Config::parse();
    let data_dir = cfg.data_dir();
    let download_dir = cfg.download_dir();
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("creating {}", data_dir.display()))?;

    let store = Store::open(data_dir.join("swarmy.json"))?;
    auth::ensure_account(&store, &cfg.username, cfg.password.as_deref())?;

    // Saved settings, with command-line flags / env vars taking precedence.
    let mut locked: Vec<&'static str> = Vec::new();
    let mut conn = store.read(|d| d.settings.connection.clone());
    if let Some(p) = cfg.bt_port {
        conn.listen_port = p;
        locked.push("connection.listen_port");
    }
    if let Some(u) = cfg.upnp {
        conn.upnp = u;
        locked.push("connection.upnp");
    }
    if let Some(i) = &cfg.bind_interface {
        conn.bind_interface = i.clone();
        locked.push("connection.bind_interface");
    }
    if let Some(v) = cfg.ipv4_only {
        conn.ipv4_only = v;
        locked.push("connection.ipv4_only");
    }
    if cfg.download_dir.is_some() {
        locked.push("downloads.default_save_path");
    }
    let saved_dir = store.read(|d| d.settings.downloads.default_save_path.clone());
    let download_dir = if cfg.download_dir.is_none() && !saved_dir.trim().is_empty() {
        std::path::PathBuf::from(saved_dir.trim())
    } else {
        download_dir
    };
    let opt = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_owned());

    let stats = Arc::new(Stats::open(&data_dir.join("stats.db"), download_dir.clone())?);
    let push = Arc::new(push::Push::load_or_create(&store, cfg.push_contact.clone())?);

    let engine: Arc<dyn Engine> = if cfg.demo {
        let (engine, seeds) = SimEngine::start(download_dir.clone());
        seed_demo(&store, &stats, &engine, &seeds)?;
        tracing::warn!("DEMO MODE: simulated torrents, nothing is downloaded");
        Arc::new(engine)
    } else {
        Arc::new(
            RqbitEngine::start(RqbitConfig {
                client_name: format!("Swarmy {}", env!("SWARMY_VERSION")),
                default_download_dir: download_dir.clone(),
                state_dir: data_dir.join("session"),
                listen_port: conn.listen_port,
                enable_upnp: conn.upnp,
                bind_interface: opt(&conn.bind_interface),
                ipv4_only: conn.ipv4_only,
                dht: conn.dht,
                lsd: conn.lsd,
                peer_limit: (conn.peer_limit > 0).then_some(conn.peer_limit as usize),
                socks_proxy: opt(&conn.socks_proxy),
                blocklist_url: opt(&conn.blocklist_url),
            })
            .await?,
        )
    };

    let (snap_tx, snap_rx) = watch::channel(Arc::new(Snapshot::default()));
    let geo = geo::Geo::new(&data_dir, cfg.demo)?;
    // Usenet: dirs and servers are applied from settings in usenet::start.
    let usenet = swarmy_usenet::Downloader::new(swarmy_usenet::Settings {
        servers: Vec::new(),
        incomplete_dir: data_dir.join("usenet-incomplete"),
        complete_dir: download_dir.join("usenet"),
        speed_limit: 0,
    });
    let state = Arc::new(AppState {
        engine: engine.clone(),
        store,
        auth: Auth::default(),
        fallback_download_dir: download_dir.clone(),
        data_dir: data_dir.clone(),
        locked,
        startup_connection: conn,
        policy: Default::default(),
        stats: stats.clone(),
        push,
        errored: Default::default(),
        activity: Default::default(),
        demo: cfg.demo,
        geo,
        peer_rates: Default::default(),
        usenet,
        demo_usenet: Default::default(),
        mover: Default::default(),
        snapshot: snap_rx,
    });
    // The peer-location database: fetched on first start, then kept current.
    {
        let s = state.clone();
        tokio::spawn(geo::run_updater(state.geo.clone(), move || {
            s.store.read(|d| d.settings.peer_map.locations)
        }));
    }
    tokio::spawn(state::run_ticker(state.clone(), snap_tx));
    usenet::start(&state);
    tokio::spawn(mover::run(state.clone()));
    tokio::spawn(stats::run_system_sampler(stats));

    let app = Router::new()
        // qBittorrent-compatible API for Sonarr, Radarr, Prowlarr and friends.
        .nest("/api/v2", qbit::router(state.clone()))
        // SABnzbd-compatible API (Sonarr, Radarr). `/api` itself is in api::router.
        .route("/sabnzbd/api", axum::routing::get(sab::api).post(sab::api))
        .nest("/api", api::router(state.clone()))
        .fallback(web::serve)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(cfg.web_addr)
        .await
        .with_context(|| format!("binding {}", cfg.web_addr))?;
    tracing::info!(
        "Swarmy listening on http://{} (downloads: {}, data: {})",
        cfg.web_addr,
        download_dir.display(),
        data_dir.display()
    );

    // Connect info gives login lockout and the trusted-proxy check the peer address.
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("shutting down torrent engine");
    engine.shutdown().await;
    Ok(())
}

/// Attach categories/tags to the demo catalogue and backfill 30 days of stats.
fn seed_demo(
    store: &Store,
    stats: &Stats,
    engine: &SimEngine,
    seeds: &[swarmy_engine::DemoSeed],
) -> anyhow::Result<()> {
    let now = now_unix();
    let finished: std::collections::HashSet<String> = engine
        .list()
        .into_iter()
        .filter(|t| t.finished)
        .map(|t| t.info_hash)
        .collect();
    store.update(|d| {
        for s in seeds {
            d.categories.entry(s.category.to_owned()).or_default();
            d.tags.extend(s.tags.iter().map(|t| t.to_string()));
            d.torrents
                .entry(s.info_hash.clone())
                .or_insert_with(|| TorrentMeta {
                    category: s.category.to_owned(),
                    tags: s.tags.iter().map(|t| t.to_string()).collect(),
                    added_on: now - s.added_ago,
                    // Already-finished demo torrents completed a while after being added.
                    completed_on: finished
                        .contains(&s.info_hash)
                        .then(|| now - s.added_ago + (s.added_ago / 3).min(6 * 3600)),
                    ..Default::default()
                });
        }
    })?;
    let names: Vec<(String, String, u64)> = seeds
        .iter()
        .filter_map(|s| {
            let st = engine.status(&s.info_hash)?;
            Some((
                s.info_hash.clone(),
                format!("{}|{}", st.name.unwrap_or_default(), s.category),
                st.total_bytes,
            ))
        })
        .collect();
    stats.backfill_demo(&names)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
}
