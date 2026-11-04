//! A small NNTP client: connect (optionally over TLS), sign in, fetch
//! article bodies by message-id. That's all a downloader needs (RFC 3977,
//! RFC 4643 for AUTHINFO).

use std::{sync::Arc, time::Duration};

use rustls::pki_types::ServerName;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub tls: bool,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// Connections to open at once; providers allow 10 to 100.
    pub connections: u16,
}

#[derive(Debug, Error)]
pub enum NntpError {
    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
    #[error("the server didn't answer in time")]
    Timeout,
    #[error("couldn't set up an encrypted connection: {0}")]
    Tls(String),
    #[error("the server rejected the username or password ({0})")]
    AuthFailed(String),
    #[error("the server wants a username and password")]
    AuthRequired,
    /// 430: this server doesn't have the article (expired or taken down).
    #[error("article not found")]
    NoSuchArticle,
    /// Too many connections, or temporarily unavailable.
    #[error("the server is busy or refusing connections: {0}")]
    Unavailable(String),
    #[error("unexpected reply from the server: {0}")]
    Protocol(String),
}

impl NntpError {
    /// Worth trying again later on the same server.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Io(_) | Self::Timeout | Self::Unavailable(_))
    }
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

pub struct Connection {
    stream: BufReader<Box<dyn Io>>,
    line: Vec<u8>,
    read_timeout: Duration,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

fn tls_config() -> Result<Arc<rustls::ClientConfig>, NntpError> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| NntpError::Tls(e.to_string()))?
        .with_platform_verifier()
        .map_err(|e| NntpError::Tls(e.to_string()))?
        .with_no_client_auth();
    Ok(Arc::new(config))
}

impl Connection {
    /// Connect, read the greeting and sign in if the server config has a
    /// username.
    pub async fn open(cfg: &ServerConfig) -> Result<Self, NntpError> {
        let tcp = timeout(CONNECT_TIMEOUT, TcpStream::connect((cfg.host.as_str(), cfg.port)))
            .await
            .map_err(|_| NntpError::Timeout)??;
        tcp.set_nodelay(true)?;
        let io: Box<dyn Io> = if cfg.tls {
            let name = ServerName::try_from(cfg.host.clone()).map_err(|e| NntpError::Tls(e.to_string()))?;
            let tls = tokio_rustls::TlsConnector::from(tls_config()?);
            let stream = timeout(CONNECT_TIMEOUT, tls.connect(name, tcp))
                .await
                .map_err(|_| NntpError::Timeout)?
                .map_err(|e| NntpError::Tls(e.to_string()))?;
            Box::new(stream)
        } else {
            Box::new(tcp)
        };
        let mut conn = Self { stream: BufReader::with_capacity(256 * 1024, io), line: Vec::with_capacity(4096), read_timeout: READ_TIMEOUT };
        let (code, text) = conn.status().await?;
        match code {
            200 | 201 => {}
            400 | 502 => return Err(NntpError::Unavailable(text)),
            _ => return Err(NntpError::Protocol(text)),
        }
        if !cfg.username.is_empty() {
            conn.authenticate(&cfg.username, &cfg.password).await?;
        }
        Ok(conn)
    }

    async fn authenticate(&mut self, user: &str, pass: &str) -> Result<(), NntpError> {
        let (code, text) = self.command(&format!("AUTHINFO USER {user}")).await?;
        let (code, text) = match code {
            281 => return Ok(()),
            381 => self.command(&format!("AUTHINFO PASS {pass}")).await?,
            _ => (code, text),
        };
        match code {
            281 => Ok(()),
            // 481/482 mean bad credentials, but providers also use 481/502
            // for "too many connections"; the text tells them apart.
            481 | 482 | 502 if text.to_ascii_lowercase().contains("connection") => Err(NntpError::Unavailable(text)),
            481 | 482 | 502 => Err(NntpError::AuthFailed(text)),
            _ => Err(NntpError::Protocol(text)),
        }
    }

    async fn read_line(&mut self) -> Result<(), NntpError> {
        self.line.clear();
        let n = timeout(self.read_timeout, self.stream.read_until(b'\n', &mut self.line))
            .await
            .map_err(|_| NntpError::Timeout)??;
        if n == 0 {
            return Err(NntpError::Io(std::io::ErrorKind::UnexpectedEof.into()));
        }
        Ok(())
    }

    /// A single-line status reply: (code, text).
    async fn status(&mut self) -> Result<(u16, String), NntpError> {
        self.read_line().await?;
        let s = String::from_utf8_lossy(&self.line).trim_end().to_string();
        let code = s.get(..3).and_then(|c| c.parse().ok()).ok_or_else(|| NntpError::Protocol(s.clone()))?;
        Ok((code, s))
    }

    pub async fn command(&mut self, cmd: &str) -> Result<(u16, String), NntpError> {
        debug_assert!(!cmd.contains('\n'));
        let w = self.stream.get_mut();
        w.write_all(cmd.as_bytes()).await?;
        w.write_all(b"\r\n").await?;
        w.flush().await?;
        self.status().await
    }

    /// The rest of a multi-line reply, dot-unstuffed, CRLFs kept.
    async fn read_block(&mut self, out: &mut Vec<u8>) -> Result<(), NntpError> {
        loop {
            self.read_line().await?;
            let l = &self.line;
            if l == b".\r\n" || l == b".\n" {
                return Ok(());
            }
            if l.starts_with(b"..") {
                out.extend_from_slice(&l[1..]);
            } else {
                out.extend_from_slice(l);
            }
        }
    }

    /// The body of the article with this message-id (no angle brackets).
    pub async fn body(&mut self, message_id: &str) -> Result<Vec<u8>, NntpError> {
        if message_id.contains(['\r', '\n', ' ', '<', '>']) {
            return Err(NntpError::Protocol(format!("bad message-id {message_id:?}")));
        }
        let (code, text) = self.command(&format!("BODY <{message_id}>")).await?;
        match code {
            222 => {
                let mut out = Vec::with_capacity(800 * 1024);
                self.read_block(&mut out).await?;
                Ok(out)
            }
            430 | 423 => Err(NntpError::NoSuchArticle),
            480 => Err(NntpError::AuthRequired),
            400 | 502 => Err(NntpError::Unavailable(text)),
            _ => Err(NntpError::Protocol(text)),
        }
    }

    /// Select a group: (estimated count, low, high).
    pub async fn group(&mut self, name: &str) -> Result<(u64, u64, u64), NntpError> {
        let (code, text) = self.command(&format!("GROUP {name}")).await?;
        if code != 211 {
            return Err(NntpError::Protocol(text));
        }
        let n: Vec<u64> = text.split_whitespace().skip(1).take(3).filter_map(|x| x.parse().ok()).collect();
        match n.as_slice() {
            [c, l, h] => Ok((*c, *l, *h)),
            _ => Err(NntpError::Protocol(text)),
        }
    }

    /// Message-ids of articles `from..=to` in the selected group (via OVER).
    pub async fn message_ids(&mut self, from: u64, to: u64) -> Result<Vec<String>, NntpError> {
        let (code, text) = self.command(&format!("OVER {from}-{to}")).await?;
        if code != 224 {
            return Err(NntpError::Protocol(text));
        }
        let mut block = Vec::new();
        self.read_block(&mut block).await?;
        Ok(String::from_utf8_lossy(&block)
            .lines()
            .filter_map(|l| l.split('\t').nth(4))
            .map(|id| id.trim().trim_start_matches('<').trim_end_matches('>').to_string())
            .filter(|id| !id.is_empty())
            .collect())
    }

    pub async fn quit(mut self) {
        let _ = timeout(Duration::from_secs(2), self.command("QUIT")).await;
    }
}
