//! The swarm map: every connected peer, placed and grouped by location, with
//! how fast data is moving each way.
//!
//! The browser only ever gets places (city, country, coordinates) with peer
//! counts and rates, never peer addresses.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::Serialize;
use swarmy_engine::{PeerEntry, TorrentState};

use crate::{geo::GeoStatus, state::AppState, store::Home};

/// More places than this aren't readable on a map; the rest are summed into
/// the busiest ones' neighbours by the client's country table anyway.
const MAX_PLACES: usize = 600;
const RATE_TRACK_LIMIT: usize = 20_000;

#[derive(Debug, Serialize)]
pub struct PeerMap {
    pub geo: GeoStatus,
    pub home: Option<Home>,
    /// Connected peers in total, and those we couldn't place (private
    /// addresses, or no database).
    pub total: usize,
    pub unlocated: usize,
    pub places: Vec<PlaceView>,
}

#[derive(Debug, Serialize)]
pub struct PlaceView {
    pub lat: f64,
    pub lon: f64,
    pub country: String,
    pub city: Option<String>,
    pub peers: u32,
    /// Bytes per second from these peers to us, and from us to them.
    pub dl: u64,
    pub ul: u64,
}

struct Sample {
    at: Instant,
    dl: u64,
    ul: u64,
    rate: (u64, u64),
}

/// Turns per-peer byte counters into rates between polls.
#[derive(Default)]
pub struct PeerRates {
    samples: Mutex<HashMap<(String, SocketAddr), Sample>>,
}

impl PeerRates {
    fn rate(&self, hash: &str, p: &PeerEntry, now: Instant) -> (u64, u64) {
        if let (Some(dl), Some(ul)) = (p.download_speed, p.upload_speed) {
            return (dl, ul);
        }
        let mut samples = self.samples.lock().unwrap();
        let key = (hash.to_string(), p.addr);
        match samples.get_mut(&key) {
            Some(s) => {
                let dt = now.duration_since(s.at).as_secs_f64();
                // Several open tabs poll at once; keep the last rate rather
                // than measuring over a sliver of time.
                if dt >= 0.8 {
                    let dl = p.downloaded_bytes.saturating_sub(s.dl) as f64 / dt;
                    let ul = p.uploaded_bytes.saturating_sub(s.ul) as f64 / dt;
                    *s = Sample { at: now, dl: p.downloaded_bytes, ul: p.uploaded_bytes, rate: (dl as u64, ul as u64) };
                }
                s.rate
            }
            None => {
                if samples.len() >= RATE_TRACK_LIMIT {
                    samples.clear();
                }
                samples.insert(key, Sample { at: now, dl: p.downloaded_bytes, ul: p.uploaded_bytes, rate: (0, 0) });
                (0, 0)
            }
        }
    }

    fn forget_older_than(&self, age: Duration, now: Instant) {
        self.samples.lock().unwrap().retain(|_, s| now.duration_since(s.at) < age);
    }
}

/// Build the map for one torrent, or for everything that's running.
pub fn build(state: &AppState, hash: Option<&str>) -> PeerMap {
    let now = Instant::now();
    let hashes: Vec<String> = match hash {
        Some(h) => vec![h.to_string()],
        None => state
            .engine
            .list()
            .into_iter()
            .filter(|t| matches!(t.state, TorrentState::Downloading | TorrentState::Seeding))
            .map(|t| t.info_hash)
            .collect(),
    };

    type Key = (String, Option<String>, i32, i32);
    let mut places: HashMap<Key, PlaceView> = HashMap::new();
    let (mut total, mut unlocated) = (0, 0);
    for h in &hashes {
        let Ok(peers) = state.engine.peers(h) else { continue };
        for p in &peers {
            total += 1;
            let (dl, ul) = state.peer_rates.rate(h, p, now);
            let Some(place) = state.geo.lookup(p.addr.ip()) else {
                unlocated += 1;
                continue;
            };
            // Group to roughly 10 km so a city's peers share one point.
            let key = (place.country.clone(), place.city.clone(), (place.lat * 10.0) as i32, (place.lon * 10.0) as i32);
            let v = places.entry(key).or_insert_with(|| PlaceView {
                lat: place.lat,
                lon: place.lon,
                country: place.country,
                city: place.city,
                peers: 0,
                dl: 0,
                ul: 0,
            });
            v.peers += 1;
            v.dl += dl;
            v.ul += ul;
        }
    }
    state.peer_rates.forget_older_than(Duration::from_secs(120), now);

    let mut places: Vec<PlaceView> = places.into_values().collect();
    places.sort_by(|a, b| (b.dl + b.ul, b.peers).cmp(&(a.dl + a.ul, a.peers)));
    places.truncate(MAX_PLACES);
    PeerMap {
        geo: state.geo.status(),
        home: state.store.read(|d| d.settings.peer_map.home),
        total,
        unlocated,
        places,
    }
}
