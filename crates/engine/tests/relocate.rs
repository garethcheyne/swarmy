//! Moving torrents with the real engine: a seeding torrent moves, keeps
//! seeding from its new folder without downloading anything, and is still
//! there after a restart.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use swarmy_engine::{AddOptions, AddSource, ContentLayout, Engine, RqbitConfig, RqbitEngine, TorrentState};

fn config(root: &Path) -> RqbitConfig {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    RqbitConfig {
        default_download_dir: root.join("downloads"),
        state_dir: root.join("state"),
        listen_port: port,
        enable_upnp: false,
        bind_interface: None,
        ipv4_only: true,
        dht: false,
        lsd: false,
        peer_limit: None,
        socks_proxy: None,
        blocklist_url: None,
        client_name: "Swarmy test".into(),
    }
}

fn write(path: &Path, len: usize, seed: u8) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect::<Vec<_>>()).unwrap();
}

/// A two-file torrent called "Pack", with its data in `<root>/old/Pack`.
async fn make_torrent(root: &Path) -> Vec<u8> {
    let content = root.join("old/Pack");
    write(&content.join("a.bin"), 300_000, 1);
    write(&content.join("sub/b.bin"), 120_000, 2);
    let spawner = librqbit::spawn_utils::BlockingSpawner::new(2);
    let opts = librqbit::CreateTorrentOptions { piece_length: Some(32 * 1024), ..Default::default() };
    librqbit::create_torrent(&content, opts, &spawner).await.unwrap().as_bytes().unwrap().to_vec()
}

async fn wait_for(engine: &RqbitEngine, hash: &str, what: &str, ok: impl Fn(&swarmy_engine::TorrentStatus) -> bool) {
    for _ in 0..200 {
        if engine.status(hash).is_some_and(|s| ok(&s)) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}: {:?}", engine.status(hash));
}

async fn add(engine: &RqbitEngine, torrent: Vec<u8>, save: PathBuf, paused: bool) -> String {
    let opts = AddOptions { save_path: Some(save), paused, layout: ContentLayout::Original };
    engine.add(AddSource::TorrentFile(torrent), opts).await.unwrap().info_hash
}

#[tokio::test(flavor = "multi_thread")]
async fn a_seeding_torrent_moves_and_keeps_seeding() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let torrent = make_torrent(root).await;
    let cfg = config(root);

    let engine = RqbitEngine::start(cfg.clone()).await.unwrap();
    let hash = add(&engine, torrent, root.join("old"), false).await;
    wait_for(&engine, &hash, "the existing data to check out", |s| s.finished && s.state == TorrentState::Seeding).await;
    assert_eq!(engine.status(&hash).unwrap().output_folder, root.join("old/Pack"));

    let to = root.join("new/Pack");
    engine.relocate(&hash, to.clone()).await.unwrap();

    assert!(to.join("a.bin").exists() && to.join("sub/b.bin").exists());
    assert!(!root.join("old/Pack").exists(), "the emptied torrent folder is removed");
    assert!(root.join("old").exists(), "the folder above it is left alone");
    wait_for(&engine, &hash, "seeding from the new folder", |s| s.finished && s.state == TorrentState::Seeding).await;
    let s = engine.status(&hash).unwrap();
    assert_eq!(s.output_folder, to);
    assert_eq!(s.downloaded_bytes, s.total_bytes);
    assert!(fs::read_dir(cfg.state_dir.join("moves")).map(|d| d.count()).unwrap_or(0) == 0, "journal cleared");

    // It's remembered in the new folder after a restart.
    engine.shutdown().await;
    drop(engine);
    let engine = RqbitEngine::start(cfg).await.unwrap();
    wait_for(&engine, &hash, "the restored torrent", |s| s.finished).await;
    assert_eq!(engine.status(&hash).unwrap().output_folder, to);
    engine.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_torrent_stays_paused_and_nothing_is_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let torrent = make_torrent(root).await;
    let engine = RqbitEngine::start(config(root)).await.unwrap();
    let hash = add(&engine, torrent, root.join("old"), true).await;
    wait_for(&engine, &hash, "the torrent to be added", |s| s.state == TorrentState::Paused).await;

    // Something is already where a.bin would go: the move is refused, untouched.
    write(&root.join("taken/Pack/a.bin"), 10, 9);
    let err = engine.relocate(&hash, root.join("taken/Pack")).await.unwrap_err();
    assert!(err.to_string().contains("already exists"), "{err:#}");
    assert!(root.join("old/Pack/a.bin").exists());
    assert_eq!(fs::metadata(root.join("taken/Pack/a.bin")).unwrap().len(), 10);

    engine.relocate(&hash, root.join("elsewhere/Pack")).await.unwrap();
    // It checks its pieces at the new folder first (Initializing), then settles paused.
    wait_for(&engine, &hash, "the moved torrent to settle, paused", |s| {
        s.output_folder == root.join("elsewhere/Pack") && s.state == TorrentState::Paused
    })
    .await;
    engine.shutdown().await;
}
