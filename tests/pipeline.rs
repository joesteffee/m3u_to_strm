//! End-to-end tests for the whole pass: download → classify → write STRMs → clean up → notify Emby.
//!
//! These drive the same entry point the container's `main` uses, against a local HTTP server, so
//! what an unRAID user runs is what is under test.
//!
//! One `Harness` per test: it holds the log capture, which serialises these tests by design.

mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use m3u_to_strm::config::Config;
use m3u_to_strm::emby::Emby;
use m3u_to_strm::fsx::Fs;
use m3u_to_strm::log::Capture;
use m3u_to_strm::runner;
use support::{Request, Server};

const PLAYLIST: &str = concat!(
    "#EXTM3U\n",
    "#EXTINF:-1 tvg-name=\"The Matrix (1999)\" group-title=\"Movies\",The Matrix\n",
    "http://example.org/movie/111/the-matrix.mkv\n",
    "#EXTINF:-1 tvg-name=\"Severance S01E01\" group-title=\"Series\",Severance\n",
    "http://example.org/series/222/s01e01.mkv\n",
    "#EXTINF:-1 tvg-name=\"US| CNN\" group-title=\"News\",CNN\n",
    "http://example.org/live/333/cnn.ts\n",
);

const MOVIE_STRM: &str = "movies/The Matrix (1999)/The Matrix (1999).strm";
const SERIES_STRM: &str = "series/Severance/Season 1/S01E01.strm";
const LIVE_M3U: &str = "livetv/livetv.m3u";
const MOVIE_URL: &str = "http://example.org/movie/111/the-matrix.mkv";

/// How the mock server answers Emby calls.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EmbyMode {
    /// Nothing but the playlist is served.
    Off,
    /// Item lookups return one item (id `4242`); every other Emby call answers 204.
    Healthy,
    /// Every Emby call fails with 500.
    Broken,
}

/// A run: temp directories, a controllable playlist server, and a log capture.
struct Harness {
    root: tempfile::TempDir,
    config: Config,
    server: Server,
    capture: Capture,
    body: Arc<Mutex<String>>,
    downloads_broken: Arc<AtomicBool>,
}

impl Harness {
    fn new(playlist: &str, mode: EmbyMode) -> Harness {
        Harness::serving(Arc::new(Mutex::new(playlist.to_string())), mode)
    }

    fn serving(body: Arc<Mutex<String>>, mode: EmbyMode) -> Harness {
        // Starting the capture holds a global lock, so exactly one harness exists per test.
        let capture = Capture::start();
        let root = tempfile::tempdir().expect("temp dir");
        let broken = Arc::new(AtomicBool::new(false));

        let handler_body = Arc::clone(&body);
        let handler_broken = Arc::clone(&broken);
        let server = Server::start(move |request: &Request| -> (u16, String) {
            if request.path().ends_with(".m3u") {
                if handler_broken.load(Ordering::Relaxed) {
                    return (500, "provider unavailable".to_string());
                }
                return (200, handler_body.lock().unwrap().clone());
            }
            match mode {
                EmbyMode::Off => (404, String::new()),
                EmbyMode::Healthy => {
                    if request.path() == "/emby/Items" {
                        (200, r#"{"Items":[{"Id":"4242"}]}"#.to_string())
                    } else {
                        (204, String::new())
                    }
                }
                EmbyMode::Broken => (500, "emby unavailable".to_string()),
            }
        });

        let connected = mode != EmbyMode::Off;
        let config = Config {
            movies_dir: root.path().join("movies"),
            series_dir: root.path().join("series"),
            livetv_dir: root.path().join("livetv"),
            tmp_playlist: root.path().join("playlist.m3u"),
            m3u_url: Some(format!("{}/playlist.m3u", server.url)),
            emby_server_url: connected.then(|| server.url.clone()),
            emby_api_key: connected.then(|| "test-key".to_string()),
            emby_movies_path: connected.then(|| "/mnt/media/movies".to_string()),
            emby_series_path: connected.then(|| "/mnt/media/series".to_string()),
            emby_livetv_path: connected.then(|| "/mnt/media/livetv".to_string()),
            ..Config::default()
        };

        Harness {
            root,
            config,
            server,
            capture,
            body,
            downloads_broken: broken,
        }
    }

    fn run(&self) -> Result<(), String> {
        let emby = Emby::new(&self.config);
        let mut fs = Fs::new();
        runner::run_cycle(&self.config, &emby, &mut fs)
    }

    /// Replaces the playlist the server serves on the next download.
    fn set_playlist(&self, playlist: &str) {
        *self.body.lock().unwrap() = playlist.to_string();
    }

    fn break_downloads(&self) {
        self.downloads_broken.store(true, Ordering::Relaxed);
    }

    /// Makes the cached playlist `hours` old, so the next pass re-downloads it.
    fn age_playlist(&self, hours: u64) {
        let file = fs::OpenOptions::new()
            .append(true)
            .open(&self.config.tmp_playlist)
            .expect("open cached playlist");
        let age = SystemTime::now() - Duration::from_secs(hours * 3600);
        file.set_modified(age).expect("backdate cached playlist");
    }

    fn lines(&self) -> Vec<String> {
        self.capture.lines()
    }

    /// Only keep log lines written from now on.
    fn clear_log(&mut self) {
        self.capture.clear();
    }

    fn logged(&self, needle: &str) -> bool {
        self.lines().iter().any(|line| line.contains(needle))
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative))
            .unwrap_or_else(|error| panic!("no file at {relative}: {error}"))
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn exists(&self, relative: &str) -> bool {
        self.path(relative).exists()
    }

    fn downloads(&self) -> usize {
        self.server
            .count_matching(|hit| hit.path().ends_with(".m3u"))
    }

    fn requests(&self, method: &str, path: &str) -> Vec<Request> {
        self.server
            .hits()
            .into_iter()
            .filter(|hit| hit.method == method && hit.path() == path)
            .collect()
    }

    /// Every library file with its contents: the tree a media server would see.
    fn library(&self) -> BTreeSet<String> {
        [
            &self.config.movies_dir,
            &self.config.series_dir,
            &self.config.livetv_dir,
        ]
        .into_iter()
        .flat_map(|dir| manifest(dir.as_path()))
        .collect()
    }
}

/// Every file below `root`, as `relative/path|contents`.
fn manifest(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if !root.exists() {
        return out;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let body = fs::read_to_string(&path).unwrap_or_default();
                out.insert(format!("{name}|{body}"));
            }
        }
    }
    out
}

/// Decodes the `%XX` and `+` escapes the Emby client emits in query strings.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("00");
            out.push(u8::from_str_radix(hex, 16).unwrap_or(0));
            index += 3;
        } else if bytes[index] == b'+' {
            out.push(b' ');
            index += 1;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).expect("decoded utf-8")
}

#[test]
fn a_first_pass_writes_movies_series_and_live_tv() {
    let harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.run().expect("first pass");

    assert_eq!(harness.read(MOVIE_STRM), MOVIE_URL);
    assert_eq!(
        harness.read(SERIES_STRM),
        "http://example.org/series/222/s01e01.mkv"
    );
    assert_eq!(
        harness.read(LIVE_M3U),
        "#EXTINF:-1 tvg-name=\"US| CNN\" group-title=\"News\",CNN\nhttp://example.org/live/333/cnn.ts"
    );
    assert!(harness.logged("Classified: 1 movies, 1 series, 1 live TV"));
    assert!(harness.logged("✅ Processing complete."));
}

#[test]
fn a_second_pass_changes_nothing() {
    let mut harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.run().expect("first pass");
    let after_first = harness.library();

    harness.clear_log();
    harness.run().expect("second pass");

    assert_eq!(
        after_first,
        harness.library(),
        "a repeat pass must be a no-op"
    );
    assert_eq!(
        harness.downloads(),
        1,
        "the cached playlist should be reused"
    );
    assert!(!harness.logged("Created new STRM file"));
    assert!(!harness.logged("Updated STRM file"));
}

#[test]
fn a_changed_url_updates_the_file_in_place() {
    let harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.run().expect("first pass");

    harness.set_playlist(&PLAYLIST.replace("movie/111/the-matrix.mkv", "movie/999/new.mkv"));
    harness.age_playlist(9);
    harness.run().expect("second pass");

    assert_eq!(
        harness.read(MOVIE_STRM),
        "http://example.org/movie/999/new.mkv"
    );
    assert!(harness.logged("Updated STRM file"));
    assert_eq!(harness.downloads(), 2);
}

#[test]
fn a_fresh_cache_is_reused_and_a_stale_one_is_not() {
    let harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.run().expect("first pass");
    assert_eq!(harness.downloads(), 1);

    harness.run().expect("cached pass");
    assert_eq!(harness.downloads(), 1);
    assert!(harness.logged("ℹ️ Using cached playlist"));

    harness.age_playlist(9);
    harness.run().expect("stale pass");
    assert_eq!(harness.downloads(), 2);
    assert!(harness.logged("file is 9.0 hours old (cache: 8 hours)"));
}

#[test]
fn the_playlist_cache_reports_why_it_redownloads() {
    let mut harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.run().expect("first pass");
    assert!(harness.logged("file does not exist"));

    // An empty cached file is never trusted.
    fs::write(&harness.config.tmp_playlist, "").expect("empty the cache");
    harness.run().expect("pass after an empty cache");
    assert!(harness.logged("file is empty"));
    assert_eq!(harness.downloads(), 2);

    // M3U_CACHE_HOURS decides whether an old file counts as fresh.
    harness.config.m3u_cache_hours = 24;
    harness.age_playlist(9);
    harness.run().expect("pass with a 24 hour cache");
    assert!(harness.logged("Using cached playlist (age: 9.0 hours, cache: 24 hours)"));
    assert_eq!(harness.downloads(), 2);

    harness.config.m3u_cache_hours = 1;
    harness.run().expect("pass with a 1 hour cache");
    assert!(harness.logged("file is 9.0 hours old (cache: 1 hours)"));
    assert_eq!(harness.downloads(), 3);
}

#[test]
fn an_empty_playlist_never_deletes_existing_files() {
    let mut harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.config.remove_orphaned = true;
    harness.config.remove_files = true;
    harness.run().expect("first pass");
    let before = harness.library();
    assert_eq!(before.len(), 3, "library: {before:#?}");

    // A playlist that parses to zero usable items, e.g. a provider returning an error page.
    harness.set_playlist(
        "#EXTM3U\n#EXTINF:-1 tvg-id=\"x\" group-title=\"Movies\", junk\nhttp://x/y\n",
    );
    harness.age_playlist(9);
    harness.run().expect("empty pass");

    assert!(harness.logged("⚠️ Playlist contains no valid items"));
    assert!(harness.logged("⚠️ Skipping orphan cleanup to prevent accidental mass deletion"));
    assert_eq!(
        before,
        harness.library(),
        "an empty playlist must delete nothing"
    );
}

#[test]
fn orphaned_files_are_removed_only_when_asked() {
    let mut harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.write(
        "movies/Old Movie/Old Movie.strm",
        "http://example.org/movie/1/old.mkv",
    );

    harness.run().expect("pass without REMOVE_ORPHANED");
    assert!(harness.exists("movies/Old Movie/Old Movie.strm"));
    assert!(!harness.logged("orphaned"));

    harness.config.remove_orphaned = true;
    harness.run().expect("pass with REMOVE_ORPHANED");
    assert!(!harness.exists("movies/Old Movie/Old Movie.strm"));
    assert!(harness.logged("Found 1 orphaned movie STRM file(s) to remove"));
    assert!(harness.exists(MOVIE_STRM));
}

#[test]
fn remove_files_deletes_directories_left_empty() {
    let mut harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.write(
        "movies/Gone/Gone.strm",
        "http://example.org/movie/2/gone.mkv",
    );
    harness.config.remove_orphaned = true;
    harness.config.remove_files = true;

    harness.run().expect("pass");

    assert!(
        !harness.exists("movies/Gone"),
        "an emptied directory should go"
    );
    assert!(harness.exists(MOVIE_STRM));
}

#[test]
fn max_items_per_run_limits_new_items_and_resumes_later() {
    let playlist = concat!(
        "#EXTM3U\n",
        "#EXTINF:-1 tvg-name=\"A (2001)\" group-title=\"Movies\",A\nhttp://example.org/movie/1/a.mkv\n",
        "#EXTINF:-1 tvg-name=\"B (2002)\" group-title=\"Movies\",B\nhttp://example.org/movie/2/b.mkv\n",
        "#EXTINF:-1 tvg-name=\"C (2003)\" group-title=\"Movies\",C\nhttp://example.org/movie/3/c.mkv\n",
    );
    let mut harness = Harness::new(playlist, EmbyMode::Off);
    harness.config.max_items_per_run = 2;
    harness.run().expect("limited pass");

    assert!(harness.exists("movies/A (2001)/A (2001).strm"));
    assert!(harness.exists("movies/B (2002)/B (2002).strm"));
    assert!(!harness.exists("movies/C (2003)/C (2003).strm"));
    assert!(harness.logged("Item processing limit: 2 (movies + series, excluding Live TV)"));
    assert!(harness.logged("⚠️ Item processing limit reached (2). Skipping remaining 1 movie(s)"));

    harness.run().expect("second pass");
    assert!(harness.exists("movies/C (2003)/C (2003).strm"));
}

#[test]
fn country_codes_filter_live_tv_only() {
    let playlist = concat!(
        "#EXTM3U\n",
        "#EXTINF:-1 tvg-name=\"US| CNN\" group-title=\"News\",CNN\nhttp://example.org/live/1/cnn.ts\n",
        "#EXTINF:-1 tvg-name=\"FR| France 2\" group-title=\"News\",France 2\nhttp://example.org/live/2/france2.ts\n",
        "#EXTINF:-1 tvg-name=\"The Matrix (1999)\" group-title=\"Movies\",The Matrix\nhttp://example.org/movie/111/the-matrix.mkv\n",
    );
    let mut harness = Harness::new(playlist, EmbyMode::Off);
    harness.config.filter_country_codes = ["US".to_string()].into_iter().collect();
    harness.run().expect("pass");

    let live = harness.read(LIVE_M3U);
    assert!(
        !live.contains("CNN"),
        "US| CNN should be filtered out: {live}"
    );
    assert!(harness.logged("Filtered out 1 live TV channel(s) based on country code rules"));
    assert!(
        harness.exists(MOVIE_STRM),
        "country codes must not touch movies"
    );
}

#[test]
fn included_country_codes_keep_only_matching_channels() {
    let mut harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.config.include_country_codes = ["gb".to_string()].into_iter().collect();
    harness.run().expect("pass");

    // Nothing matches GB, so no live TV playlist is written at all.
    assert!(!harness.exists(LIVE_M3U));
    assert!(harness.logged("Filtered out 1 live TV channel(s) based on country code rules"));
}

#[test]
fn two_versions_of_a_title_get_id_based_filenames() {
    let playlist = concat!(
        "#EXTM3U\n",
        "#EXTINF:-1 tvg-name=\"Big Movie (2020)\" group-title=\"Movies\",Big\nhttp://example.org/movie/a/111.mkv\n",
        "#EXTINF:-1 tvg-name=\"Big Movie (2020)\" group-title=\"Movies\",Big\nhttp://example.org/movie/b/222.mkv\n",
    );
    let harness = Harness::new(playlist, EmbyMode::Off);
    harness.run().expect("pass");

    assert_eq!(
        harness.read("movies/Big Movie (2020)/Big Movie (2020).strm"),
        "http://example.org/movie/a/111.mkv"
    );
    assert_eq!(
        harness.read("movies/Big Movie (2020)/Big Movie (2020) [222].strm"),
        "http://example.org/movie/b/222.mkv"
    );
}

#[test]
fn emby_is_told_about_new_directories() {
    let harness = Harness::new(PLAYLIST, EmbyMode::Healthy);
    harness.run().expect("pass");

    let refreshes: Vec<String> = harness
        .requests("POST", "/emby/Library/Refresh")
        .iter()
        .map(|hit| percent_decode(&hit.query_param("Path").unwrap_or_default()))
        .collect();
    assert_eq!(
        refreshes,
        vec![
            "/mnt/media/movies/The Matrix (1999)".to_string(),
            "/mnt/media/series/Severance/Season 1".to_string(),
        ]
    );
}

#[test]
fn emby_is_asked_to_refresh_an_item_whose_url_changed() {
    let harness = Harness::new(PLAYLIST, EmbyMode::Healthy);
    harness.run().expect("first pass");

    harness.set_playlist(&PLAYLIST.replace("movie/111/the-matrix.mkv", "movie/999/new.mkv"));
    harness.age_playlist(9);
    harness.run().expect("second pass");

    let lookups = harness.requests("GET", "/emby/Items");
    assert_eq!(lookups.len(), 1, "one lookup for the changed item");
    assert_eq!(
        percent_decode(&lookups[0].query_param("Path").unwrap()),
        "/mnt/media/movies/The Matrix (1999)/The Matrix (1999).strm"
    );
    assert_eq!(
        lookups[0].query_param("Recursive").as_deref(),
        Some("false")
    );
    assert_eq!(
        harness.requests("POST", "/emby/Items/4242/Refresh").len(),
        1
    );
}

#[test]
fn orphan_removal_deletes_the_emby_item_too() {
    let mut harness = Harness::new(PLAYLIST, EmbyMode::Healthy);
    harness.write(
        "movies/Gone/Gone.strm",
        "http://example.org/movie/9/gone.mkv",
    );
    harness.config.remove_orphaned = true;

    harness.run().expect("pass");

    assert_eq!(harness.requests("DELETE", "/emby/Items/4242").len(), 1);
    assert!(harness.logged("✅ Deleted 1 orphaned movie item(s) from Emby library"));
    assert!(!harness.exists("movies/Gone/Gone.strm"));
}

#[test]
fn emby_errors_do_not_stop_the_pass() {
    let harness = Harness::new(PLAYLIST, EmbyMode::Broken);
    harness
        .run()
        .expect("a failing Emby must not fail the pass");
    assert!(harness.exists(MOVIE_STRM));
    assert!(harness.exists(SERIES_STRM));
    assert!(harness.exists(LIVE_M3U));
}

#[test]
fn a_failed_download_reports_an_error_and_keeps_the_tree() {
    let harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.run().expect("first pass");
    let before = harness.library();

    harness.break_downloads();
    harness.age_playlist(9);
    let error = harness
        .run()
        .expect_err("a failing download must be reported");

    assert!(error.contains("500"), "unexpected error text: {error}");
    assert_eq!(
        before,
        harness.library(),
        "a failed download must not touch the tree"
    );
}

#[test]
fn a_missing_url_is_reported_before_any_work() {
    let mut harness = Harness::new(PLAYLIST, EmbyMode::Off);
    harness.config.m3u_url = None;

    let error = harness.run().expect_err("M3U_URL is required");
    assert_eq!(error, "M3U_URL is not set");
    assert_eq!(harness.library(), BTreeSet::new());
}
