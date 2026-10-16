//! `m3u-to-strm` — turn an IPTV M3U playlist into an Emby-ready STRM library.
//!
//! Default behaviour matches the original container: convert once, then repeat every
//! `INTERVAL_SECONDS` (or exit after one pass when it is `0`).

use std::process::ExitCode;
use std::time::Duration;

use m3u_to_strm::config::Config;
use m3u_to_strm::emby::Emby;
use m3u_to_strm::fsx::Fs;
use m3u_to_strm::{log, runner};

const USAGE: &str = "\
m3u-to-strm — convert an IPTV M3U playlist into STRM files for Emby

Usage:
  m3u-to-strm              Convert, then repeat every INTERVAL_SECONDS (0 = run once)
  m3u-to-strm --once       Convert exactly once; exit 1 if the pass failed
  m3u-to-strm --explain    Print the resolved configuration and exit (no network)
  m3u-to-strm --help       Show this help
  m3u-to-strm --version    Show the version

Environment:
  M3U_URL                  Playlist URL                     (required to convert)
  EMBY_SERVER_URL          Emby base URL                    (optional)
  EMBY_API_KEY             Emby API key                     (required with the URL)
  EMBY_MOVIES_PATH         Mount of the movie library
  EMBY_SERIES_PATH         Mount of the series library
  EMBY_LIVETV_PATH         Mount of the live TV library
  REMOVE_FILES             true = delete folders left empty (default false)
  REMOVE_ORPHANED          true = delete STRM files the playlist dropped (default false)
  INTERVAL_SECONDS         Seconds between passes           (default 0)
  MAX_ITEMS_PER_RUN        New items per pass, 0 = no limit (default 0)
  M3U_CACHE_HOURS          Playlist cache lifetime          (default 8)
  LOG_LEVEL                DEBUG|INFO|WARNING|ERROR         (default INFO)
";

fn main() -> ExitCode {
    log::init_from_env();

    let mut once = false;
    let mut explain = false;
    let args = std::env::args().skip(1);
    for arg in args {
        match arg.as_str() {
            "--once" => once = true,
            "--explain" => explain = true,
            "--help" | "-h" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--version" | "-V" => {
                println!("m3u-to-strm {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            other => {
                log::error(format!("❌ Unknown argument: {other}"));
                eprint!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            log::error(format!("❌ Invalid configuration: {error}"));
            return ExitCode::from(2);
        }
    };

    if explain {
        print_configuration(&config);
        return ExitCode::SUCCESS;
    }

    if config.m3u_url.is_none() {
        log::error("❌ Invalid configuration: M3U_URL is not set");
        return ExitCode::from(2);
    }

    let emby = Emby::new(&config);

    if once {
        let mut fs = Fs::new();
        return match runner::run_cycle(&config, &emby, &mut fs) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                log::error(format!("❌ Error: {error}"));
                ExitCode::FAILURE
            }
        };
    }

    log::info("Starting M3U to STRM converter");
    loop {
        // A fresh filesystem cache per pass: a long-lived container must notice changes made
        // outside of it between passes.
        let mut fs = Fs::new();
        if let Err(error) = runner::run_cycle(&config, &emby, &mut fs) {
            log::error(format!("❌ Error: {error}"));
        }

        if config.interval_seconds <= 0 {
            break;
        }
        log::info(format!(
            "Waiting {} seconds until next update...",
            config.interval_seconds
        ));
        std::thread::sleep(Duration::from_secs(config.interval_seconds as u64));
    }

    ExitCode::SUCCESS
}

fn shown(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("(not set)")
}

fn print_configuration(config: &Config) {
    println!("Resolved configuration:");
    println!("  M3U_URL              = {}", shown(&config.m3u_url));
    println!(
        "  EMBY_SERVER_URL      = {}",
        shown(&config.emby_server_url)
    );
    println!(
        "  EMBY_API_KEY         = {}",
        if config.emby_api_key.is_some() {
            "(set)"
        } else {
            "(not set)"
        }
    );
    println!("  Emby integration     = {}", config.emby_enabled());
    println!("  movies_dir           = {}", config.movies_dir.display());
    println!("  series_dir           = {}", config.series_dir.display());
    println!("  livetv_dir           = {}", config.livetv_dir.display());
    println!("  tmp_playlist         = {}", config.tmp_playlist.display());
    println!("  REMOVE_FILES         = {}", config.remove_files);
    println!("  REMOVE_ORPHANED      = {}", config.remove_orphaned);
    println!("  INTERVAL_SECONDS     = {}", config.interval_seconds);
    println!("  MAX_ITEMS_PER_RUN    = {}", config.max_items_per_run);
    println!("  M3U_CACHE_HOURS      = {}", config.m3u_cache_hours);
    println!(
        "  EMBY_MOVIES_PATH     = {}",
        shown(&config.emby_movies_path)
    );
    println!(
        "  EMBY_SERIES_PATH     = {}",
        shown(&config.emby_series_path)
    );
    println!(
        "  EMBY_LIVETV_PATH     = {}",
        shown(&config.emby_livetv_path)
    );
    println!(
        "  country filter       = {} {}",
        config.country_rule().label(),
        match config.country_rule().codes().is_empty() {
            true => String::new(),
            false => format!("({})", config.country_rule().codes().join(",")),
        }
    );
}
