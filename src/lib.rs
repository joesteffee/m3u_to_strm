#![deny(unsafe_code)]
//! `m3u-to-strm` — turn an IPTV M3U playlist into an Emby/Jellyfin-friendly STRM library.
//!
//! A Rust port of the original single-file Python script. Same environment variables, same output
//! layout, no interpreter in the container.

pub mod config;
pub mod emby;
pub mod fsx;
pub mod http;
pub mod log;
pub mod naming;
pub mod paths;
pub mod playlist;
pub mod pytext;
pub mod runner;
pub mod strm;

pub use config::Config;
