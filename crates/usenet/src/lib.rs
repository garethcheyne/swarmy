//! Swarmy's Usenet downloader.
//!
//! - [`nzb`]: reading NZB files.
//! - [`nntp`]: talking to Usenet servers.
//! - [`yenc`]: decoding (and, for tests, encoding) article bodies.
//! - [`downloader`]: the queue and connection pool.
//! - [`post`]: repair, unpack and move finished downloads.

pub mod downloader;
pub mod nntp;
pub mod nzb;
pub mod par2;
pub mod post;
pub mod yenc;

pub use downloader::{Downloader, Event, JobStatus, JobView, Priority, Server, ServerStatus, Settings};
pub use nntp::ServerConfig;
