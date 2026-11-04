//! End to end: a mock NNTP server serves yEnc articles; the downloader
//! fetches, decodes, assembles, post-processes and moves them.

use std::{collections::HashMap, net::SocketAddr, path::Path, sync::Arc, time::Duration};

use swarmy_usenet::{Downloader, Event, JobStatus, Priority, Server, ServerConfig, Settings, yenc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

type Articles = Arc<HashMap<String, Vec<u8>>>;

/// A tiny NNTP server: optional AUTHINFO, BODY by message-id, QUIT.
async fn mock_server(articles: Articles, creds: Option<(&'static str, &'static str)>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else { return };
            let articles = articles.clone();
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                w.write_all(b"200 mock ready\r\n").await.unwrap();
                let mut authed = creds.is_none();
                let mut user = String::new();
                let mut line = String::new();
                loop {
                    line.clear();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    let l = line.trim_end();
                    let reply: Vec<u8> = if let Some(u) = l.strip_prefix("AUTHINFO USER ") {
                        user = u.to_string();
                        b"381 password please\r\n".to_vec()
                    } else if let Some(p) = l.strip_prefix("AUTHINFO PASS ") {
                        if creds.is_some_and(|(cu, cp)| cu == user && cp == p) {
                            authed = true;
                            b"281 welcome\r\n".to_vec()
                        } else {
                            b"481 authentication rejected\r\n".to_vec()
                        }
                    } else if let Some(id) = l.strip_prefix("BODY <").and_then(|s| s.strip_suffix('>')) {
                        if !authed {
                            b"480 authentication required\r\n".to_vec()
                        } else if let Some(body) = articles.get(id) {
                            let mut out = format!("222 0 <{id}>\r\n").into_bytes();
                            for line in body.split_inclusive(|&b| b == b'\n') {
                                if line.starts_with(b".") {
                                    out.push(b'.');
                                }
                                out.extend_from_slice(line);
                            }
                            out.extend_from_slice(b".\r\n");
                            out
                        } else {
                            b"430 no such article\r\n".to_vec()
                        }
                    } else if l == "QUIT" {
                        let _ = w.write_all(b"205 bye\r\n").await;
                        return;
                    } else {
                        b"500 what?\r\n".to_vec()
                    };
                    if w.write_all(&reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    addr
}

/// Post `data` as `name` in parts of `part_size`; returns (articles, <file> xml).
fn post(data: &[u8], name: &str, part_size: usize, tag: &str) -> (Vec<(String, Vec<u8>)>, String) {
    let total = data.len().div_ceil(part_size) as u32;
    let mut arts = Vec::new();
    let mut segs = String::new();
    for p in 0..total {
        let begin = p as usize * part_size;
        let end = (begin + part_size).min(data.len());
        let body = yenc::encode(data, name, p + 1, total, begin, end, 128);
        let id = format!("{tag}.{name}.{p}@swarmy.test");
        segs.push_str(&format!("<segment bytes=\"{}\" number=\"{}\">{id}</segment>", body.len(), p + 1));
        arts.push((id, body));
    }
    let xml = format!(
        "<file poster=\"test\" date=\"1700000000\" subject=\"&quot;{name}&quot; yEnc (1/{total})\"><groups><group>alt.binaries.test</group></groups><segments>{segs}</segments></file>"
    );
    (arts, xml)
}

fn nzb(files: &[String]) -> Vec<u8> {
    format!("<?xml version=\"1.0\"?><nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">{}</nzb>", files.concat()).into_bytes()
}

fn data(n: usize, seed: u8) -> Vec<u8> {
    let mut x = seed as u32 | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn server(id: &str, addr: SocketAddr, priority: u8, user: &str, pass: &str) -> Server {
    Server {
        id: id.into(),
        name: id.into(),
        enabled: true,
        priority,
        config: ServerConfig { host: addr.ip().to_string(), port: addr.port(), tls: false, username: user.into(), password: pass.into(), connections: 4 },
    }
}

async fn wait(d: &Downloader, rx: &mut tokio::sync::broadcast::Receiver<Event>) -> Event {
    let _ = d;
    tokio::time::timeout(Duration::from_secs(30), rx.recv()).await.expect("job finished in time").unwrap()
}

fn dirs(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    (root.join("incomplete"), root.join("complete"))
}

#[tokio::test]
async fn downloads_and_assembles_multiple_files() {
    let a = data(3_000_000, 1);
    let b = data(1_234_567, 2);
    let (mut arts, fa) = post(&a, "movie.mkv", 700_000, "t1");
    let (arts_b, fb) = post(&b, "movie.nfo", 500_000, "t1");
    arts.extend(arts_b);
    let addr = mock_server(Arc::new(arts.into_iter().collect()), None).await;
    let tmp = tempfile::tempdir().unwrap();
    let (inc, comp) = dirs(tmp.path());
    let d = Downloader::new(Settings { servers: vec![server("main", addr, 0, "", "")], incomplete_dir: inc, complete_dir: comp.clone(), speed_limit: 0 });
    let mut rx = d.subscribe();
    let id = d.add(&nzb(&[fa, fb]), Some("My.Movie.2024"), "movies", Priority::Normal).unwrap();
    assert!(id.starts_with("SABnzbd_nzo_"));
    let Event::Completed(job) = wait(&d, &mut rx).await else { panic!("job failed") };
    let out = comp.join("movies").join("My.Movie.2024");
    assert_eq!(job.storage.as_deref(), Some(out.as_path()));
    assert_eq!(std::fs::read(out.join("movie.mkv")).unwrap(), a);
    assert_eq!(std::fs::read(out.join("movie.nfo")).unwrap(), b);
    assert_eq!(d.history()[0].status, JobStatus::Completed);
    assert!(d.queue().is_empty());
}

#[tokio::test]
async fn backup_server_fills_in_missing_articles() {
    let a = data(2_100_000, 3);
    let (arts, fa) = post(&a, "file.bin", 700_000, "t2");
    // The main server lacks the middle article; the backup has everything.
    let main: HashMap<_, _> = arts.iter().cloned().enumerate().filter(|(i, _)| *i != 1).map(|(_, a)| a).collect();
    let backup: HashMap<_, _> = arts.into_iter().collect();
    let main_addr = mock_server(Arc::new(main), None).await;
    let backup_addr = mock_server(Arc::new(backup), None).await;
    let tmp = tempfile::tempdir().unwrap();
    let (inc, comp) = dirs(tmp.path());
    let d = Downloader::new(Settings {
        servers: vec![server("main", main_addr, 0, "", ""), server("backup", backup_addr, 1, "", "")],
        incomplete_dir: inc,
        complete_dir: comp.clone(),
        speed_limit: 0,
    });
    let mut rx = d.subscribe();
    d.add(&nzb(&[fa]), Some("Backup.Test"), "", Priority::Normal).unwrap();
    let Event::Completed(job) = wait(&d, &mut rx).await else { panic!("job failed") };
    assert_eq!(std::fs::read(job.storage.unwrap().join("file.bin")).unwrap(), a);
    let servers = d.servers();
    assert!(servers.iter().find(|s| s.id == "backup").unwrap().downloaded > 0, "backup was used");
}

#[tokio::test]
async fn missing_everywhere_without_repair_data_fails_clearly() {
    let a = data(1_400_000, 4);
    let (arts, fa) = post(&a, "file.bin", 700_000, "t3");
    let only_first: HashMap<_, _> = arts.into_iter().take(1).collect();
    let addr = mock_server(Arc::new(only_first), None).await;
    let tmp = tempfile::tempdir().unwrap();
    let (inc, comp) = dirs(tmp.path());
    let d = Downloader::new(Settings { servers: vec![server("main", addr, 0, "", "")], incomplete_dir: inc, complete_dir: comp, speed_limit: 0 });
    let mut rx = d.subscribe();
    d.add(&nzb(&[fa]), Some("Broken"), "", Priority::Normal).unwrap();
    let Event::Failed(job) = wait(&d, &mut rx).await else { panic!("should fail") };
    let msg = job.fail_message.unwrap();
    assert!(msg.contains("no repair"), "{msg}");
    assert!(job.missing > 0);
}

#[tokio::test]
async fn signs_in_and_reports_bad_passwords() {
    let a = data(800_000, 5);
    let (arts, fa) = post(&a, "f.bin", 400_000, "t4");
    let addr = mock_server(Arc::new(arts.into_iter().collect()), Some(("alice", "s3cret"))).await;
    let cfg = |pass: &str| ServerConfig { host: "127.0.0.1".into(), port: addr.port(), tls: false, username: "alice".into(), password: pass.into(), connections: 2 };
    assert!(swarmy_usenet::downloader::test_server(&cfg("s3cret")).await.is_ok());
    let err = swarmy_usenet::downloader::test_server(&cfg("wrong")).await.unwrap_err();
    assert!(err.to_string().contains("rejected"), "{err}");

    let tmp = tempfile::tempdir().unwrap();
    let (inc, comp) = dirs(tmp.path());
    let d = Downloader::new(Settings { servers: vec![server("main", addr, 0, "alice", "s3cret")], incomplete_dir: inc, complete_dir: comp, speed_limit: 0 });
    let mut rx = d.subscribe();
    d.add(&nzb(&[fa]), Some("Auth"), "", Priority::Normal).unwrap();
    let Event::Completed(job) = wait(&d, &mut rx).await else { panic!("job failed") };
    assert_eq!(std::fs::read(job.storage.unwrap().join("f.bin")).unwrap(), a);
}

#[tokio::test]
async fn speed_limit_is_respected() {
    let a = data(1_000_000, 6);
    let (arts, fa) = post(&a, "slow.bin", 100_000, "t5");
    let addr = mock_server(Arc::new(arts.into_iter().collect()), None).await;
    let tmp = tempfile::tempdir().unwrap();
    let (inc, comp) = dirs(tmp.path());
    // ~1 MB at 400 KB/s should take at least ~1.5 s (one second of burst).
    let d = Downloader::new(Settings { servers: vec![server("main", addr, 0, "", "")], incomplete_dir: inc, complete_dir: comp, speed_limit: 400_000 });
    let mut rx = d.subscribe();
    let t = std::time::Instant::now();
    d.add(&nzb(&[fa]), Some("Slow"), "", Priority::Normal).unwrap();
    let Event::Completed(_) = wait(&d, &mut rx).await else { panic!("job failed") };
    assert!(t.elapsed() >= Duration::from_millis(1300), "took {:?}", t.elapsed());
}

// ---------------------------------------------------------------- repair and unpack
//
// Fixtures from tests/fixtures/make.sh (real rar, par2 and 7z tools). The
// payload inside every archive is data(600_000, 1).

fn fixture_dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// Post every file in a fixture folder; returns (articles, nzb).
fn post_dir(dir: &str, tag: &str) -> (Vec<(String, Vec<u8>)>, Vec<String>) {
    let mut arts = Vec::new();
    let mut files = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(fixture_dir(dir)).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        let bytes = std::fs::read(&p).unwrap();
        let (a, xml) = post(&bytes, &p.file_name().unwrap().to_string_lossy(), 64_000, tag);
        arts.extend(a);
        files.push(xml);
    }
    (arts, files)
}

fn nzb_with_password(files: &[String], password: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?><nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\"><head><meta type=\"password\">{password}</meta></head>{}</nzb>",
        files.concat()
    )
    .into_bytes()
}

async fn run_job(arts: HashMap<String, Vec<u8>>, nzb_bytes: Vec<u8>, name: &str) -> (Event, Downloader, tempfile::TempDir, u64) {
    let served = Arc::new(arts);
    let addr = mock_server(served, None).await;
    let tmp = tempfile::tempdir().unwrap();
    let (inc, comp) = dirs(tmp.path());
    let d = Downloader::new(Settings { servers: vec![server("main", addr, 0, "", "")], incomplete_dir: inc, complete_dir: comp, speed_limit: 0 });
    let mut rx = d.subscribe();
    d.add(&nzb_bytes, Some(name), "tv", Priority::Normal).unwrap();
    let ev = wait(&d, &mut rx).await;
    let fetched = d.servers()[0].downloaded;
    (ev, d, tmp, fetched)
}

#[tokio::test]
async fn intact_rar_set_unpacks_without_fetching_repair_data() {
    let (arts, files) = post_dir("rarset", "r1");
    let recovery_bytes: u64 = arts.iter().filter(|(id, _)| id.contains(".vol")).map(|(_, b)| b.len() as u64).sum();
    let all_bytes: u64 = arts.iter().map(|(_, b)| b.len() as u64).sum();
    let (ev, _d, _tmp, fetched) = run_job(arts.into_iter().collect(), nzb(&files), "Show.S01E01").await;
    let Event::Completed(job) = ev else { panic!("failed: {ev:?}") };
    let out = job.storage.unwrap();
    assert_eq!(std::fs::read(out.join("payload.bin")).unwrap(), data(600_000, 1));
    // No archives or PAR2 files are left behind.
    let names: Vec<String> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["payload.bin"]);
    assert!(fetched <= all_bytes - recovery_bytes + 1024, "recovery volumes weren't downloaded ({fetched} of {all_bytes})");
}

#[tokio::test]
async fn damaged_rar_set_is_repaired_with_par2() {
    let (arts, files) = post_dir("rarset", "r2");
    // Lose an article (about 10% of the data) from the middle volume, on
    // every server. The set has 20% recovery data.
    let damaged: HashMap<_, _> = arts.into_iter().filter(|(id, _)| !id.contains("show.part2.rar.1@")).collect();
    let (ev, _d, _tmp, _) = run_job(damaged, nzb(&files), "Show.S01E02").await;
    let Event::Completed(job) = ev else { panic!("failed: {ev:?}") };
    assert!(job.message.as_deref().unwrap_or("").starts_with("Repaired"), "{:?}", job.message);
    assert_eq!(std::fs::read(job.storage.unwrap().join("payload.bin")).unwrap(), data(600_000, 1));
}

#[tokio::test]
async fn unpacks_7z() {
    let (arts, files) = post_dir("sevenz", "z1");
    let (ev, _d, _tmp, _) = run_job(arts.into_iter().collect(), nzb(&files), "Movie.2024").await;
    let Event::Completed(job) = ev else { panic!("failed: {ev:?}") };
    assert_eq!(std::fs::read(job.storage.unwrap().join("payload.bin")).unwrap(), data(600_000, 1));
}

#[tokio::test]
async fn password_protected_rar() {
    let (arts, files) = post_dir("password", "p1");
    let arts: HashMap<_, _> = arts.into_iter().collect();
    let (ev, _d, _tmp, _) = run_job(arts.clone(), nzb_with_password(&files, "swarmy"), "Locked.With.Password").await;
    let Event::Completed(job) = ev else { panic!("failed: {ev:?}") };
    assert_eq!(std::fs::read(job.storage.unwrap().join("payload.bin")).unwrap(), data(600_000, 1));

    let (ev, _d, _tmp, _) = run_job(arts, nzb(&files), "Locked.Without.Password").await;
    let Event::Failed(job) = ev else { panic!("should fail without the password") };
    assert!(job.fail_message.as_deref().unwrap_or("").contains("password"), "{:?}", job.fail_message);
}

#[tokio::test]
async fn too_much_damage_fails_with_a_clear_reason() {
    let (arts, files) = post_dir("rarset", "r3");
    // About 30% gone: more than the 20% recovery data can fix.
    let damaged: HashMap<_, _> = arts
        .into_iter()
        .filter(|(id, _)| !(id.contains("show.part2.rar.0@") || id.contains("show.part2.rar.1@") || id.contains("show.part2.rar.2@")))
        .collect();
    let (ev, _d, _tmp, _) = run_job(damaged, nzb(&files), "Show.S01E03").await;
    let Event::Failed(job) = ev else { panic!("should fail") };
    assert!(job.fail_message.as_deref().unwrap_or("").contains("Too much is missing"), "{:?}", job.fail_message);
}
