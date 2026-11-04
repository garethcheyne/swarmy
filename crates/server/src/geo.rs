//! Where peers are, for the swarm map.
//!
//! Locations come from DB-IP's free "IP to City Lite" database (CC BY 4.0),
//! kept in `<data>/geo` and refreshed when DB-IP publishes a new month. It is
//! downloaded on first start and looked up on this machine: peer addresses
//! are never sent to anyone. Lookups are cached in memory until the database
//! changes.
//!
//! Protections, since the file comes from the internet:
//! - HTTPS only, from a fixed URL; compressed and uncompressed sizes are capped,
//!   so a bad or malicious download can't fill the disk.
//! - A download is only used after it opens as a City database and answers a
//!   known lookup; until then the previous file stays in service.
//! - Files are written under a new name and never modified once in use.

use std::{
    collections::HashMap,
    io::{Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, bail};
use chrono::{Datelike, Months, Utc};
use maxminddb::{Mmap, Reader, geoip2};
use serde::Serialize;

const URL: &str = "https://download.db-ip.com/free";
const PREFIX: &str = "dbip-city-lite-";
/// The compressed file is ~60 MB and the database ~130 MB; anything far
/// beyond that is not what we asked for.
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;
const MAX_DATABASE: u64 = 600 * 1024 * 1024;
const CACHE_LIMIT: usize = 50_000;
/// How often to look for a new month's database.
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

pub const ATTRIBUTION: &str = "IP geolocation by DB-IP (db-ip.com), CC BY 4.0";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Place {
    pub lat: f64,
    pub lon: f64,
    /// ISO 3166-1 alpha-2.
    pub country: String,
    pub city: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GeoState {
    /// Not downloaded yet.
    #[default]
    Missing,
    Downloading,
    Ready,
    Error,
    /// Switched off in Settings.
    Off,
    /// Demo mode: made-up locations, no database.
    Demo,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GeoStatus {
    pub state: GeoState,
    /// The database edition in use, e.g. "2026-11".
    pub edition: Option<String>,
    /// 0..1 while downloading.
    pub progress: Option<f32>,
    pub error: Option<String>,
    pub attribution: &'static str,
}

struct Db {
    edition: String,
    reader: Reader<Mmap>,
}

pub struct Geo {
    dir: PathBuf,
    demo: bool,
    db: RwLock<Option<Arc<Db>>>,
    cache: Mutex<HashMap<IpAddr, Option<Place>>>,
    status: Mutex<GeoStatus>,
    busy: AtomicBool,
    http: reqwest::Client,
}

impl Geo {
    pub fn new(data_dir: &Path, demo: bool) -> anyhow::Result<Arc<Self>> {
        let dir = data_dir.join("geo");
        let http = reqwest::Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(60))
            .user_agent(concat!("Swarmy/", env!("SWARMY_VERSION")))
            .build()
            .context("building geo HTTP client")?;
        let geo = Arc::new(Self {
            dir,
            demo,
            db: RwLock::new(None),
            cache: Mutex::new(HashMap::new()),
            status: Mutex::new(GeoStatus { attribution: ATTRIBUTION, ..Default::default() }),
            busy: AtomicBool::new(false),
            http,
        });
        if demo {
            geo.set_status(|s| s.state = GeoState::Demo);
        } else if let Some((edition, path)) = geo.newest_on_disk() {
            match open(&path) {
                Ok(reader) => geo.install(edition, reader),
                Err(e) => tracing::warn!("ignoring unreadable location database {}: {e:#}", path.display()),
            }
        }
        Ok(geo)
    }

    pub fn status(&self) -> GeoStatus {
        self.status.lock().unwrap().clone()
    }

    fn set_status(&self, f: impl FnOnce(&mut GeoStatus)) {
        f(&mut self.status.lock().unwrap());
    }

    /// Where an address is, if we can tell. Private and local addresses
    /// never have a location.
    pub fn lookup(&self, ip: IpAddr) -> Option<Place> {
        let ip = canonical(ip);
        if !is_public(ip) {
            return None;
        }
        if self.demo {
            return Some(demo_place(ip));
        }
        if let Some(hit) = self.cache.lock().unwrap().get(&ip) {
            return hit.clone();
        }
        let db = self.db.read().unwrap().clone()?;
        let place = locate(&db.reader, ip);
        let mut cache = self.cache.lock().unwrap();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(ip, place.clone());
        place
    }

    fn install(&self, edition: String, reader: Reader<Mmap>) {
        *self.db.write().unwrap() = Some(Arc::new(Db { edition: edition.clone(), reader }));
        self.cache.lock().unwrap().clear();
        self.set_status(|s| {
            s.state = GeoState::Ready;
            s.edition = Some(edition.clone());
            s.progress = None;
            s.error = None;
        });
        self.remove_old(&edition);
    }

    /// Turned off in Settings: release the database (it stays on disk).
    pub fn disable(&self) {
        if self.demo {
            return;
        }
        *self.db.write().unwrap() = None;
        self.cache.lock().unwrap().clear();
        self.set_status(|s| {
            s.state = GeoState::Off;
            s.progress = None;
        });
    }

    fn newest_on_disk(&self) -> Option<(String, PathBuf)> {
        let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(&self.dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let edition = name.strip_prefix(PREFIX)?.strip_suffix(".mmdb")?.to_string();
                valid_edition(&edition).then(|| (edition, e.path()))
            })
            .collect();
        found.sort();
        found.pop()
    }

    fn remove_old(&self, keep: &str) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let current = name == format!("{PREFIX}{keep}.mmdb");
            if name.starts_with(PREFIX) && !current {
                // On Windows a file still mapped by a reader can't be removed
                // yet; it goes on the next pass.
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Make sure we have this month's database, or at least the latest one
    /// DB-IP has published. Returns quickly if there's nothing to do or a
    /// download is already running.
    pub async fn refresh(self: &Arc<Self>) {
        if self.demo || self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let result = self.refresh_inner().await;
        self.busy.store(false, Ordering::SeqCst);
        if let Err(e) = result {
            tracing::warn!("location database update failed: {e:#}");
            let have = self.db.read().unwrap().is_some();
            self.set_status(|s| {
                // An old database keeps working; only say "error" if we have none.
                s.state = if have { GeoState::Ready } else { GeoState::Error };
                s.progress = None;
                s.error = Some(format!("{e:#}"));
            });
        }
    }

    async fn refresh_inner(self: &Arc<Self>) -> anyhow::Result<()> {
        // Switched back on: use what's already on disk before downloading.
        if self.db.read().unwrap().is_none()
            && let Some((edition, path)) = self.newest_on_disk()
            && let Ok(reader) = open(&path)
        {
            self.install(edition, reader);
        }
        let now = Utc::now().date_naive();
        let this_month = format!("{:04}-{:02}", now.year(), now.month());
        let last = now.checked_sub_months(Months::new(1)).context("date")?;
        let last_month = format!("{:04}-{:02}", last.year(), last.month());
        let current = self.db.read().unwrap().as_ref().map(|d| d.edition.clone());
        if current.as_deref() == Some(this_month.as_str()) {
            return Ok(());
        }
        // DB-IP publishes early in the month; until then last month's is newest.
        let mut wanted = vec![this_month];
        if current.as_deref() != Some(last_month.as_str()) {
            wanted.push(last_month);
        }
        for edition in wanted {
            match self.download(&edition).await? {
                true => return Ok(()),
                false => continue,
            }
        }
        if current.is_none() {
            bail!("DB-IP has no recent database to download");
        }
        Ok(())
    }

    /// Download and install one edition. `Ok(false)` if it isn't published.
    async fn download(self: &Arc<Self>, edition: &str) -> anyhow::Result<bool> {
        debug_assert!(valid_edition(edition));
        let url = format!("{URL}/{PREFIX}{edition}.mmdb.gz");
        let mut resp = self.http.get(&url).send().await.context("contacting db-ip.com")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        if !resp.status().is_success() {
            bail!("db-ip.com answered {}", resp.status());
        }
        let total = resp.content_length();
        if total.is_some_and(|n| n > MAX_DOWNLOAD) {
            bail!("the download is larger than expected; not using it");
        }
        tracing::info!("downloading location database {edition}");
        std::fs::create_dir_all(&self.dir)?;
        let gz = self.dir.join(format!("{PREFIX}{edition}.mmdb.gz.part"));
        let raw = self.dir.join(format!("{PREFIX}{edition}.mmdb.part"));
        let done = self.dir.join(format!("{PREFIX}{edition}.mmdb"));
        self.set_status(|s| {
            s.state = if s.edition.is_some() { s.state.clone() } else { GeoState::Downloading };
            s.progress = Some(0.0);
            s.error = None;
        });

        let mut file = std::fs::File::create(&gz)?;
        let mut got: u64 = 0;
        while let Some(chunk) = resp.chunk().await.context("downloading")? {
            got += chunk.len() as u64;
            if got > MAX_DOWNLOAD {
                drop(file);
                let _ = std::fs::remove_file(&gz);
                bail!("the download is larger than expected; not using it");
            }
            file.write_all(&chunk)?;
            if let Some(total) = total {
                let p = (got as f32 / total as f32).min(1.0);
                self.set_status(|s| s.progress = Some(p * 0.9));
            }
        }
        file.sync_all()?;
        drop(file);

        // Decompress and check it on a blocking thread; it's ~130 MB.
        let (gz2, raw2, done2) = (gz.clone(), raw.clone(), done.clone());
        let reader = tokio::task::spawn_blocking(move || -> anyhow::Result<Reader<Mmap>> {
            let result = (|| {
                let mut input = flate2::read::GzDecoder::new(std::fs::File::open(&gz2)?).take(MAX_DATABASE + 1);
                let mut out = std::fs::File::create(&raw2)?;
                let n = std::io::copy(&mut input, &mut out).context("decompressing")?;
                if n > MAX_DATABASE {
                    bail!("the database is larger than expected; not using it");
                }
                out.sync_all()?;
                drop(out);
                // Check it before it replaces anything.
                validate(&raw2)?;
                std::fs::rename(&raw2, &done2)?;
                open(&done2)
            })();
            let _ = std::fs::remove_file(&gz2);
            if result.is_err() {
                let _ = std::fs::remove_file(&raw2);
            }
            result
        })
        .await??;
        self.install(edition.to_string(), reader);
        tracing::info!("location database {edition} ready");
        Ok(true)
    }
}

/// Keep the database current while Swarmy runs. `enabled` is read each pass,
/// so switching the setting on or off takes effect without a restart.
pub async fn run_updater(geo: Arc<Geo>, enabled: impl Fn() -> bool + Send + 'static) {
    if geo.demo {
        return;
    }
    loop {
        if enabled() {
            geo.refresh().await;
        } else {
            geo.disable();
        }
        tokio::time::sleep(CHECK_EVERY).await;
    }
}

fn open(path: &Path) -> anyhow::Result<Reader<Mmap>> {
    // SAFETY: memory-mapping is only unsound if the file changes underneath
    // us. Swarmy never writes to a database file once it has a final name:
    // updates go to a new file (new edition) and old ones are removed only
    // after they've been swapped out.
    unsafe { Reader::open_mmap(path) }.with_context(|| format!("opening {}", path.display()))
}

/// A download must be a City database that can place a well-known address.
fn validate(path: &Path) -> anyhow::Result<()> {
    // Mapped rather than read, to avoid a 130 MB spike; the reader is dropped
    // before the file is renamed.
    let reader = open(path).context("not a location database")?;
    let kind = reader.metadata().database_type.to_ascii_lowercase();
    if !kind.contains("city") {
        bail!("expected a city database, got {kind:?}");
    }
    let probe: IpAddr = [1, 1, 1, 1].into();
    if locate(&reader, probe).is_none() {
        bail!("the database can't place a known address");
    }
    Ok(())
}

fn locate<S: AsRef<[u8]>>(reader: &Reader<S>, ip: IpAddr) -> Option<Place> {
    let city: geoip2::City = reader.lookup(ip).ok()?.decode().ok()??;
    Some(Place {
        lat: city.location.latitude?,
        lon: city.location.longitude?,
        country: city.country.iso_code?.to_string(),
        city: city.city.names.english.map(str::to_string),
    })
}

fn valid_edition(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7 && b[4] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || c.is_ascii_digit())
}

/// IPv4-mapped IPv6 (`::ffff:1.2.3.4`) is really IPv4.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

/// Addresses that could be somewhere on the public internet.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || a == 0
                || (a == 100 && (64..128).contains(&b)) // carrier-grade NAT
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local
                || (first & 0xffc0) == 0xfe80) // link local
        }
    }
}

/// Demo mode: real cities, weighted roughly like a busy public swarm.
const DEMO_CITIES: &[(&str, &str, f64, f64, u32)] = &[
    ("Amsterdam", "NL", 52.37, 4.90, 9),
    ("Frankfurt am Main", "DE", 50.11, 8.68, 9),
    ("Berlin", "DE", 52.52, 13.40, 5),
    ("Paris", "FR", 48.86, 2.35, 7),
    ("London", "GB", 51.51, -0.13, 8),
    ("Manchester", "GB", 53.48, -2.24, 3),
    ("Stockholm", "SE", 59.33, 18.07, 4),
    ("Oslo", "NO", 59.91, 10.75, 2),
    ("Helsinki", "FI", 60.17, 24.94, 2),
    ("Warsaw", "PL", 52.23, 21.01, 4),
    ("Prague", "CZ", 50.08, 14.44, 2),
    ("Madrid", "ES", 40.42, -3.70, 4),
    ("Milan", "IT", 45.46, 9.19, 3),
    ("Bucharest", "RO", 44.43, 26.10, 3),
    ("Kyiv", "UA", 50.45, 30.52, 3),
    ("Istanbul", "TR", 41.01, 28.98, 2),
    ("New York", "US", 40.71, -74.01, 8),
    ("Ashburn", "US", 39.04, -77.49, 5),
    ("Chicago", "US", 41.88, -87.63, 4),
    ("Dallas", "US", 32.78, -96.80, 3),
    ("Los Angeles", "US", 34.05, -118.24, 6),
    ("Seattle", "US", 47.61, -122.33, 3),
    ("Toronto", "CA", 43.65, -79.38, 4),
    ("Montreal", "CA", 45.50, -73.57, 2),
    ("Vancouver", "CA", 49.28, -123.12, 2),
    ("Mexico City", "MX", 19.43, -99.13, 2),
    ("São Paulo", "BR", -23.55, -46.63, 5),
    ("Buenos Aires", "AR", -34.60, -58.38, 2),
    ("Santiago", "CL", -33.45, -70.67, 1),
    ("Johannesburg", "ZA", -26.20, 28.05, 2),
    ("Lagos", "NG", 6.52, 3.38, 1),
    ("Cairo", "EG", 30.04, 31.24, 1),
    ("Dubai", "AE", 25.20, 55.27, 2),
    ("Mumbai", "IN", 19.08, 72.88, 4),
    ("Bengaluru", "IN", 12.97, 77.59, 3),
    ("Singapore", "SG", 1.35, 103.82, 5),
    ("Hong Kong", "HK", 22.32, 114.17, 3),
    ("Tokyo", "JP", 35.68, 139.69, 5),
    ("Seoul", "KR", 37.57, 126.98, 3),
    ("Taipei", "TW", 25.03, 121.57, 2),
    ("Manila", "PH", 14.60, 120.98, 2),
    ("Jakarta", "ID", -6.21, 106.85, 2),
    ("Bangkok", "TH", 13.76, 100.50, 2),
    ("Sydney", "AU", -33.87, 151.21, 4),
    ("Melbourne", "AU", -37.81, 144.96, 3),
    ("Brisbane", "AU", -27.47, 153.03, 1),
    ("Perth", "AU", -31.95, 115.86, 1),
    ("Auckland", "NZ", -36.85, 174.76, 2),
    ("Wellington", "NZ", -41.29, 174.78, 1),
    ("Christchurch", "NZ", -43.53, 172.64, 1),
];

fn demo_place(ip: IpAddr) -> Place {
    let mut x: u64 = match ip {
        IpAddr::V4(v4) => u64::from(u32::from(v4)),
        IpAddr::V6(v6) => (u128::from(v6) as u64) ^ ((u128::from(v6) >> 64) as u64),
    };
    x ^= x >> 31;
    x = x.wrapping_mul(0x9e3779b97f4a7c15);
    x ^= x >> 29;
    let total: u32 = DEMO_CITIES.iter().map(|c| c.4).sum();
    let mut pick = (x % u64::from(total)) as u32;
    let (city, country, lat, lon, _) = *DEMO_CITIES
        .iter()
        .find(|c| {
            if pick < c.4 {
                true
            } else {
                pick -= c.4;
                false
            }
        })
        .unwrap_or(&DEMO_CITIES[0]);
    // Spread peers around the city a little.
    let jx = ((x >> 20) % 1000) as f64 / 1000.0 - 0.5;
    let jy = ((x >> 40) % 1000) as f64 / 1000.0 - 0.5;
    Place { lat: lat + jy * 0.12, lon: lon + jx * 0.16, country: country.to_string(), city: Some(city.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_addresses() {
        for ip in ["10.1.2.3", "192.168.1.1", "172.16.0.9", "127.0.0.1", "100.64.1.1", "169.254.0.1", "fd00::1", "fe80::1", "::1"] {
            assert!(!is_public(canonical(ip.parse().unwrap())), "{ip} should not be public");
        }
        for ip in ["1.1.1.1", "8.8.8.8", "2001:4860::8888", "::ffff:8.8.8.8"] {
            assert!(is_public(canonical(ip.parse().unwrap())), "{ip} should be public");
        }
    }

    #[test]
    fn editions() {
        assert!(valid_edition("2026-11"));
        assert!(!valid_edition("2026-1"));
        assert!(!valid_edition("../etc/x"));
    }

    #[test]
    fn demo_places_are_stable() {
        let ip: IpAddr = "81.2.69.160".parse().unwrap();
        assert_eq!(demo_place(ip), demo_place(ip));
    }
}
