//! Demo mode: a local NNTP server whose articles are generated on the fly,
//! and NZBs for made-up releases. The real downloader fetches, decodes and
//! assembles them, so the demo shows the actual engine at work.
//!
//! A message-id encodes everything needed to rebuild its article:
//! `d<seed>.<part>.<total>.<size>.<partsize>@swarmy.demo`.

use std::net::SocketAddr;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

use crate::yenc;

const PART: u64 = 750_000;

/// Start the demo server on a random local port.
pub async fn start() -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (r, mut w) = sock.into_split();
                let mut r = BufReader::new(r);
                if w.write_all(b"200 Swarmy demo news server ready\r\n").await.is_err() {
                    return;
                }
                let mut line = String::new();
                loop {
                    line.clear();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    let l = line.trim_end();
                    let reply = if let Some(id) = l.strip_prefix("BODY <").and_then(|s| s.strip_suffix('>')) {
                        match article(id) {
                            Some(body) => {
                                let mut out = format!("222 0 <{id}>\r\n").into_bytes();
                                for line in body.split_inclusive(|&b| b == b'\n') {
                                    if line.starts_with(b".") {
                                        out.push(b'.');
                                    }
                                    out.extend_from_slice(line);
                                }
                                out.extend_from_slice(b".\r\n");
                                out
                            }
                            None => b"430 no such article\r\n".to_vec(),
                        }
                    } else if l.starts_with("AUTHINFO") {
                        b"281 ok\r\n".to_vec()
                    } else if l == "QUIT" {
                        let _ = w.write_all(b"205 bye\r\n").await;
                        return;
                    } else {
                        b"500 unknown command\r\n".to_vec()
                    };
                    if w.write_all(&reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    Ok(addr)
}

fn article(id: &str) -> Option<Vec<u8>> {
    let spec = id.strip_suffix("@swarmy.demo")?.strip_prefix('d')?;
    let n: Vec<u64> = spec.split('.').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let [seed, part, total, size, part_size] = n.as_slice().try_into().ok()?;
    if part == 0 || part > total || part_size == 0 || size > 1 << 40 {
        return None;
    }
    let begin = (part - 1) * part_size;
    let end = (begin + part_size).min(size);
    // Only this part's bytes are generated; the encoder is told the offset.
    let mut chunk = vec![0u8; (end - begin) as usize];
    let mut x = (seed ^ part.wrapping_mul(0x9e37_79b9_7f4a_7c15)) | 1;
    for b in &mut chunk {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = (x >> 24) as u8;
    }
    Some(encode_at(&chunk, &format!("demo-{seed}"), part as u32, total as u32, begin, size))
}

/// yEnc-encode `chunk`, which sits at `begin` in a file of `size` bytes.
fn encode_at(chunk: &[u8], name: &str, part: u32, total: u32, begin: u64, size: u64) -> Vec<u8> {
    // Encode as a standalone part, then patch the header to the real
    // position and file size.
    let body = yenc::encode(chunk, name, part, total.max(2), 0, chunk.len(), 128);
    let text_end = body.iter().position(|&b| b == b'\n').unwrap() + 1;
    let ypart_end = text_end + body[text_end..].iter().position(|&b| b == b'\n').unwrap() + 1;
    let mut out = format!("=ybegin part={part} total={total} line=128 size={size} name={name}\r\n").into_bytes();
    out.extend_from_slice(format!("=ypart begin={} end={}\r\n", begin + 1, begin + chunk.len() as u64).as_bytes());
    out.extend_from_slice(&body[ypart_end..]);
    out
}

/// A made-up release: name, category, and files (name, bytes).
pub struct Release {
    pub name: &'static str,
    pub category: &'static str,
    pub files: &'static [(&'static str, u64)],
}

pub const RELEASES: &[Release] = &[
    Release { name: "Sintel.2010.2160p.UHD.BluRay.x265", category: "movies", files: &[("Sintel.2010.2160p.mkv", 184_000_000), ("Sintel.2010.2160p.nfo", 4_200)] },
    Release { name: "Elephants.Dream.2006.1080p.WEB-DL", category: "movies", files: &[("Elephants.Dream.2006.1080p.mkv", 96_000_000), ("Elephants.Dream.2006.1080p.srt", 38_000)] },
    Release { name: "Cosmos.Laundromat.S01E01.1080p.WEB-DL", category: "tv", files: &[("Cosmos.Laundromat.S01E01.1080p.mkv", 128_000_000)] },
    Release { name: "Spring.2019.S01E02.720p.WEB", category: "tv", files: &[("Spring.2019.S01E02.720p.mkv", 64_000_000)] },
    Release { name: "Agent.327.Operation.Barbershop.S01E03.1080p", category: "tv", files: &[("Agent.327.S01E03.1080p.mkv", 72_000_000)] },
];

/// The NZB for `release`; `round` varies the seed so repeats differ.
pub fn nzb(release: &Release, round: u64) -> Vec<u8> {
    let mut files = String::new();
    for (i, (name, size)) in release.files.iter().enumerate() {
        let seed = (round << 16) ^ (i as u64 + 1) ^ release.name.len() as u64 * 7919;
        let total = size.div_ceil(PART);
        let mut segs = String::new();
        for p in 1..=total {
            segs.push_str(&format!("<segment bytes=\"{}\" number=\"{p}\">d{seed}.{p}.{total}.{size}.{PART}@swarmy.demo</segment>", PART * 102 / 100));
        }
        files.push_str(&format!(
            "<file poster=\"demo@swarmy\" date=\"1700000000\" subject=\"[{}/{}] - &quot;{name}&quot; yEnc (1/{total})\"><groups><group>alt.binaries.swarmy.demo</group></groups><segments>{segs}</segments></file>",
            i + 1,
            release.files.len()
        ));
    }
    format!("<?xml version=\"1.0\"?><nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">{files}</nzb>").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn articles_decode_to_their_place() {
        let id = "d42.3.10.7000000.750000@swarmy.demo";
        let body = article(id).unwrap();
        let p = yenc::decode(&body).unwrap();
        assert!(p.intact);
        assert_eq!(p.offset, 1_500_000);
        assert_eq!(p.file_size, 7_000_000);
        assert_eq!(p.data.len(), 750_000);
        assert!(article("d42.11.10.7000000.750000@swarmy.demo").is_none());
    }
}
