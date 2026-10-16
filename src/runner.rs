//! The run loop: download the playlist, write STRM files, clean up what the playlist no longer has.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::config::Config;
use crate::emby::Emby;
use crate::fsx::Fs;
use crate::http;
use crate::log;
use crate::naming::{
    extract_content_id, extract_season_episode, parse_movie_name, parse_series_name,
};
use crate::playlist::{self, Item, Playlist};
use crate::strm;

/// One converter pass: refresh the playlist file, then rebuild the library from it.
pub fn run_cycle(config: &Config, emby: &Emby, fs: &mut Fs) -> Result<(), String> {
    download(config)?;
    process(config, emby, fs)
}

/// Fetch `M3U_URL` unless `TMP_PLAYLIST` is fresh enough to reuse.
pub fn download(config: &Config) -> Result<(), String> {
    let tmp = &config.tmp_playlist;
    let cache_hours = config.m3u_cache_hours;

    let mut should_download = false;
    let mut reason = String::new();
    match std::fs::metadata(tmp) {
        Err(_) => {
            should_download = true;
            reason = "file does not exist".to_string();
        }
        Ok(metadata) if metadata.len() == 0 => {
            should_download = true;
            reason = "file is empty".to_string();
        }
        Ok(metadata) => {
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let age_hours = SystemTime::now()
                .duration_since(modified)
                .unwrap_or_default()
                .as_secs_f64()
                / 3600.0;
            if age_hours > cache_hours as f64 {
                should_download = true;
                reason = format!("file is {age_hours:.1} hours old (cache: {cache_hours} hours)");
            } else {
                log::info(format!(
                    "ℹ️ Using cached playlist (age: {age_hours:.1} hours, cache: {cache_hours} hours)"
                ));
            }
        }
    }

    if !should_download {
        log::debug(format!("Using existing playlist file: {}", tmp.display()));
        return Ok(());
    }

    let url = config.m3u_url.as_deref().ok_or("M3U_URL is not set")?;
    log::info(format!("⬇️ Downloading playlist from {url} ({reason})"));
    let body = http::fetch(url)?;
    http::write_atomic(tmp, &body)?;
    log::info(format!("✅ Saved to {}", tmp.display()));
    Ok(())
}

/// Rebuild movies, series, and live TV from the playlist file.
pub fn process(config: &Config, emby: &Emby, fs: &mut Fs) -> Result<(), String> {
    let text = fs.read_lossy(&config.tmp_playlist).ok_or_else(|| {
        format!(
            "Cannot read playlist file {}",
            config.tmp_playlist.display()
        )
    })?;

    let rule = config.country_rule();
    let parsed = playlist::parse(&text, &rule);
    playlist::log_summary(&parsed, &rule);

    if parsed.is_empty() {
        log::warning("⚠️ Playlist contains no valid items (0 movies, 0 series, 0 live TV)");
        log::warning("⚠️ Skipping orphan cleanup to prevent accidental mass deletion");
        log::warning("⚠️ This may indicate a download failure or invalid playlist file");
        if config.tmp_playlist.exists() {
            match std::fs::remove_file(&config.tmp_playlist) {
                Ok(()) => log::info(format!(
                    "🗑 Deleted invalid playlist file {} (will be redownloaded next run)",
                    config.tmp_playlist.display()
                )),
                Err(e) => log::warning(format!(
                    "⚠️ Error deleting invalid playlist file {}: {}",
                    config.tmp_playlist.display(),
                    http::io_text(&e)
                )),
            }
        }
        return Ok(());
    }

    let mut processed_movies = BTreeSet::new();
    let mut processed_series = BTreeSet::new();
    let mut items_processed = 0usize;

    log::info(format!("Processing {} movie(s)", parsed.movies.len()));
    if config.max_items_per_run > 0 {
        log::info(format!(
            "Item processing limit: {} (movies + series, excluding Live TV)",
            config.max_items_per_run
        ));
    }

    let movies = process_category(
        config,
        emby,
        fs,
        &parsed.movies,
        &mut items_processed,
        &mut processed_movies,
        Category::Movie,
    )?;
    report(&movies, "Movies");
    if !movies.new_directories.is_empty() {
        emby.batch_refresh(&as_paths(&movies.new_directories));
    }

    let series = process_category(
        config,
        emby,
        fs,
        &parsed.series,
        &mut items_processed,
        &mut processed_series,
        Category::Series,
    )?;
    report(&series, "Series");
    if !series.new_directories.is_empty() {
        emby.batch_refresh(&as_paths(&series.new_directories));
    }

    if series.limit_reached {
        log::info(format!(
            "ℹ️ Processed {items_processed} items (limit: {}). Remaining items will be processed on next run.",
            config.max_items_per_run
        ));
    }

    if !parsed.live_tv.is_empty() {
        log::info(format!(
            "Processing {} live TV channel(s)",
            parsed.live_tv.len()
        ));
        let _ = fs.create_dir_all(&config.livetv_dir);
        let live_file = config.livetv_dir.join("livetv.m3u");
        fs.write_file(&live_file, &parsed.live_tv.join("\n"))
            .map_err(|err| http::io_text(&err))?;
        log::debug(format!("Live TV playlist saved to {}", live_file.display()));
    }

    cleanup_orphaned(
        config,
        emby,
        fs,
        &processed_movies,
        &config.movies_dir,
        "movie",
    );
    cleanup_orphaned(
        config,
        emby,
        fs,
        &processed_series,
        &config.series_dir,
        "series",
    );

    if config.remove_files {
        cleanup_empty_dirs(fs, &config.movies_dir);
        cleanup_empty_dirs(fs, &config.series_dir);
    }

    log::info("✅ Processing complete.");
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    Movie,
    Series,
}

impl Category {
    fn remaining_label(self) -> &'static str {
        match self {
            Category::Movie => "movie(s)",
            Category::Series => "series episode(s)",
        }
    }
}

struct CategoryReport {
    added: usize,
    updated: usize,
    skipped_by_limit: usize,
    new_directories: BTreeSet<PathBuf>,
    limit_reached: bool,
}

fn report(report: &CategoryReport, label: &str) {
    if report.added > 0 || report.updated > 0 {
        log::info(format!(
            "{label}: {} added, {} updated",
            report.added, report.updated
        ));
    }
    if report.skipped_by_limit > 0 {
        log::info(format!(
            "{label}: {} skipped due to processing limit",
            report.skipped_by_limit
        ));
    }
}

fn as_paths(set: &BTreeSet<PathBuf>) -> Vec<&Path> {
    set.iter().map(|p| p.as_path()).collect()
}

/// Where an entry's STRM file lives, and what it is called.
fn layout(category: Category, item: &Item) -> (PathBuf, String) {
    match category {
        Category::Movie => {
            let folder = parse_movie_name(&item.tvg_name);
            (PathBuf::from(folder.clone()), folder)
        }
        Category::Series => {
            let folder = parse_series_name(&item.tvg_name);
            let (season, episode) = extract_season_episode(&item.tvg_name);
            (PathBuf::from(folder).join(season), episode)
        }
    }
}

fn process_category(
    config: &Config,
    emby: &Emby,
    fs: &mut Fs,
    items: &[Item],
    items_processed: &mut usize,
    processed: &mut BTreeSet<PathBuf>,
    category: Category,
) -> Result<CategoryReport, String> {
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut handled = 0usize;
    let mut unchanged = 0usize;
    let mut new_directories = BTreeSet::new();
    let mut limit_reached = false;
    let mut changed_files = Vec::new();

    for item in items {
        let (relative_dir, file_name) = layout(category, item);
        let directory = match category {
            Category::Movie => config.movies_dir.join(&relative_dir),
            Category::Series => config.series_dir.join(&relative_dir),
        };
        let content_id = extract_content_id(&item.url);

        // Any STRM in the target folder already holding this exact URL means "nothing to do",
        // whichever file it happens to live in.
        let existing = find_file_with_url(fs, &directory, &item.url);
        if let Some(existing) = existing {
            unchanged += 1;
            processed.insert(existing);
            continue;
        }

        if config.max_items_per_run > 0 && *items_processed >= config.max_items_per_run as usize {
            limit_reached = true;
            log::warning(format!(
                "⚠️ Item processing limit reached ({}). Skipping remaining {} {}",
                config.max_items_per_run,
                items.len() - handled - unchanged,
                category.remaining_label()
            ));
            break;
        }

        let outcome = strm::write(fs, &directory, &file_name, &item.url, content_id.as_deref())?;
        processed.insert(outcome.path.clone());
        *items_processed += 1;
        handled += 1;

        if outcome.is_new {
            added += 1;
            if let Some(parent) = outcome.path.parent() {
                new_directories.insert(parent.to_path_buf());
            }
        } else if outcome.url_changed {
            updated += 1;
            changed_files.push(outcome.path);
        }
    }

    // Refresh every changed item in one parallel pass instead of a serial round trip each.
    if !changed_files.is_empty() {
        let refs: Vec<&Path> = changed_files.iter().map(|p| p.as_path()).collect();
        emby.notify_all_updated(&refs);
    }

    Ok(CategoryReport {
        added,
        updated,
        skipped_by_limit: items.len() - handled - unchanged,
        new_directories,
        limit_reached,
    })
}

fn find_file_with_url(fs: &mut Fs, directory: &Path, url: &str) -> Option<PathBuf> {
    if !fs.is_dir(directory) {
        return None;
    }
    fs.entries(directory)
        .iter()
        .filter(|entry| !entry.is_dir && entry.name.ends_with(".strm"))
        .map(|entry| directory.join(&entry.name))
        .find(|path| fs.read_stripped(path).as_deref() == Some(url))
}

/// Delete STRM files the playlist no longer mentions, then drop folders that became empty.
fn cleanup_orphaned(
    config: &Config,
    emby: &Emby,
    fs: &mut Fs,
    processed: &BTreeSet<PathBuf>,
    base: &Path,
    content_type: &str,
) {
    if !config.remove_orphaned {
        return;
    }

    let orphans: Vec<PathBuf> = fs
        .walk_strm(base)
        .into_iter()
        .filter(|path| !processed.contains(path))
        .collect();

    if !orphans.is_empty() {
        log::info(format!(
            "Found {} orphaned {content_type} STRM file(s) to remove",
            orphans.len()
        ));
        let refs: Vec<&Path> = orphans.iter().map(|p| p.as_path()).collect();
        let deleted_from_emby = emby.delete_all(&refs);

        for orphan in &orphans {
            match fs.remove_file(orphan) {
                Ok(()) => log::debug(format!("🗑 Removed orphaned file: {}", orphan.display())),
                Err(error) => log::warning(format!(
                    "⚠️ Error removing orphaned file {}: {error}",
                    orphan.display()
                )),
            }
        }

        if deleted_from_emby > 0 {
            log::info(format!(
                "✅ Deleted {deleted_from_emby} orphaned {content_type} item(s) from Emby library"
            ));
        }
    }

    cleanup_empty_dirs(fs, base);
}

/// Remove top-level folders that no longer contain a STRM file anywhere below them.
fn cleanup_empty_dirs(fs: &mut Fs, base: &Path) {
    for entry in fs.entries(base) {
        if !entry.is_dir {
            continue;
        }
        let directory = base.join(&entry.name);
        if fs.has_strm_under(&directory) {
            continue;
        }
        log::info(format!(
            "🗑 Removing empty or orphaned directory: {}",
            directory.display()
        ));
        if let Err(error) = fs.remove_dir_all(&directory) {
            log::warning(format!(
                "⚠️ Error removing directory {}: {error}",
                directory.display()
            ));
        }
    }
}

/// Lookup counts, for tests and for `--stats`.
#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub directory_reads: usize,
}

impl Stats {
    pub fn of(fs: &Fs) -> Stats {
        Stats {
            directory_reads: fs.directory_reads(),
        }
    }
}

/// Group playlist items by target folder so the filesystem is touched once per folder.
///
/// Not used for the writing pass itself (order and the per-run item limit require the original
/// order); kept for `--explain`, which reports how many folders a playlist would touch.
#[allow(dead_code)]
pub fn folders_for(playlist: &Playlist) -> HashMap<PathBuf, usize> {
    let mut counts = HashMap::new();
    for item in &playlist.movies {
        let (dir, _) = layout(Category::Movie, item);
        *counts.entry(dir).or_insert(0) += 1;
    }
    for item in &playlist.series {
        let (dir, _) = layout(Category::Series, item);
        *counts.entry(dir).or_insert(0) += 1;
    }
    counts
}
