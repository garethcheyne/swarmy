//! Check the NNTP client against a real server:
//!
//!   cargo run -p swarmy-usenet --example probe -- nntp.lore.kernel.org 563 tls org.kernel.vger.linux-kernel
//!
//! Opens several connections at once and fetches the newest articles.

use std::time::Instant;

use swarmy_usenet::nntp::{Connection, ServerConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let cfg = ServerConfig {
        host: a.get(1).cloned().unwrap_or("nntp.lore.kernel.org".into()),
        port: a.get(2).and_then(|p| p.parse().ok()).unwrap_or(563),
        tls: a.get(3).map(|t| t == "tls").unwrap_or(true),
        username: std::env::var("NNTP_USER").unwrap_or_default(),
        password: std::env::var("NNTP_PASS").unwrap_or_default(),
        connections: 4,
    };
    let group = a.get(4).cloned().unwrap_or("org.kernel.vger.linux-kernel".into());

    let t = Instant::now();
    let mut c = Connection::open(&cfg).await?;
    println!("connected to {}:{} (tls={}) in {:?}", cfg.host, cfg.port, cfg.tls, t.elapsed());
    let (count, low, high) = c.group(&group).await?;
    println!("{group}: ~{count} articles, {low}..{high}");
    let ids = c.message_ids(high.saturating_sub(39).max(low), high).await?;
    println!("{} message-ids from OVER", ids.len());
    c.quit().await;

    // Fetch them over 4 connections at once.
    let t = Instant::now();
    let chunks: Vec<Vec<String>> = ids.chunks(ids.len().div_ceil(4).max(1)).map(|c| c.to_vec()).collect();
    let mut tasks = Vec::new();
    for chunk in chunks {
        let cfg = cfg.clone();
        tasks.push(tokio::spawn(async move {
            let mut conn = Connection::open(&cfg).await?;
            let mut bytes = 0usize;
            let mut missing = 0usize;
            for id in &chunk {
                match conn.body(id).await {
                    Ok(b) => bytes += b.len(),
                    Err(swarmy_usenet::nntp::NntpError::NoSuchArticle) => missing += 1,
                    Err(e) => return Err(anyhow::anyhow!(e)),
                }
            }
            conn.quit().await;
            anyhow::Ok((chunk.len(), bytes, missing))
        }));
    }
    let (mut n, mut bytes, mut missing) = (0, 0, 0);
    for t in tasks {
        let (a, b, m) = t.await??;
        n += a;
        bytes += b;
        missing += m;
    }
    let secs = t.elapsed().as_secs_f64();
    println!("fetched {n} bodies ({missing} missing), {bytes} bytes in {secs:.2}s over 4 connections ({:.0} KiB/s)", bytes as f64 / 1024.0 / secs);
    // A missing article must be reported as such, not as an error.
    let mut c = Connection::open(&cfg).await?;
    match c.body("definitely-not-a-real-article@swarmy.invalid").await {
        Err(swarmy_usenet::nntp::NntpError::NoSuchArticle) => println!("missing article correctly reported as 430"),
        other => println!("missing article gave: {other:?}"),
    }
    c.quit().await;
    Ok(())
}
