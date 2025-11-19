#!/usr/bin/env python3
import os
import re
import sys
import time
import logging
import requests
from pathlib import Path
from typing import Tuple, Optional, Set

# Configure logging to stdout
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - %(message)s',
    datefmt='%Y-%m-%d %H:%M:%S',
    stream=sys.stdout
)
logger = logging.getLogger(__name__)

# Environment variables
EMBY_SERVER_URL = os.environ.get("EMBY_SERVER_URL")
EMBY_API_KEY = os.environ.get("EMBY_API_KEY")
M3U_URL = os.environ.get("M3U_URL")
MOVIES_DIR = Path("/usr/src/app/movies")
SERIES_DIR = Path("/usr/src/app/series")
LIVETV_DIR = Path("/usr/src/app/livetv")
REMOVE_FILES = os.environ.get("REMOVE_FILES", "false").lower() == "true"
REMOVE_ORPHANED = os.environ.get("REMOVE_ORPHANED", "false").lower() == "true"
INTERVAL_SECONDS = int(os.environ.get("INTERVAL_SECONDS", "0"))
# Limit number of items (movies + series) to process per run (0 = no limit)
MAX_ITEMS_PER_RUN = int(os.environ.get("MAX_ITEMS_PER_RUN", "0"))

# Path mapping for Emby (container path -> host path)
# If not set, assumes container paths match host paths
EMBY_MOVIES_PATH = os.environ.get("EMBY_MOVIES_PATH")
EMBY_SERIES_PATH = os.environ.get("EMBY_SERIES_PATH")
EMBY_LIVETV_PATH = os.environ.get("EMBY_LIVETV_PATH")

# Temporary M3U file
TMP_PLAYLIST = Path("/tmp/playlist.m3u")

def download_playlist():
    logger.info(f"⬇️ Downloading playlist from {M3U_URL}")
    resp = requests.get(M3U_URL)
    resp.raise_for_status()
    TMP_PLAYLIST.write_text(resp.text, encoding="utf-8")
    logger.info(f"✅ Saved to {TMP_PLAYLIST}")

def safe_filename(name: str) -> str:
    # Remove invalid filesystem characters including # which can cause issues
    return re.sub(r'[\\/:"*?<>|#]+', '', name).strip()

def parse_movie_name(tvg_name: str):
    # Remove language prefix like "EN - ", "NF - ", "D+ - ", etc.
    tvg_name = re.sub(r'^[A-Z0-9+]{1,4}\s*-\s*', '', tvg_name).strip()
    # Extract year (can be anywhere, but typically after title)
    year_match = re.search(r'\((\d{4})\)', tvg_name)
    year = year_match.group(0) if year_match else ''
    # Remove all parentheses content (including year and other info)
    tvg_name = re.sub(r'\s*\([^()]*\)', '', tvg_name).strip()
    # Add year back if it was present
    if year:
        tvg_name += f" {year}"
    return safe_filename(tvg_name)

def parse_series_name(tvg_name: str):
    # Remove language prefix like "EN - ", "NF - ", "D+ - ", "SPT - ", etc.
    tvg_name = re.sub(r'^[A-Z0-9+]{1,4}\s*-\s*', '', tvg_name).strip()
    # Extract year (can appear before or after season/episode)
    year_match = re.search(r'\((\d{4})\)', tvg_name)
    year = year_match.group(0) if year_match else ''
    # Remove season/episode from name first (handles "S01 E02", "S01E02", ".S01E06", etc.)
    tvg_name = re.sub(r'[.\s]+S\d{1,2}\s*E\d{1,2}.*$', '', tvg_name, flags=re.IGNORECASE).strip()
    # Remove all parentheses content (including year, country codes like "(US)", etc.)
    tvg_name = re.sub(r'\s*\([^()]*\)', '', tvg_name).strip()
    # Add year back if it was present
    if year:
        tvg_name += f" {year}"
    return safe_filename(tvg_name)

def extract_season_episode(tvg_name: str):
    # Match "S01E02", "S01 E02", "S1E2", etc. (case insensitive)
    match = re.search(r'S(\d{1,2})\s*E(\d{1,2})', tvg_name, re.IGNORECASE)
    if match:
        season_num = int(match.group(1))
        episode_num = int(match.group(2))
        season = f"Season {season_num}"
        episode = f"S{season_num:02d}E{episode_num:02d}"
        return season, episode
    return "Season 1", "S01E01"

def write_strm_file(directory: Path, filename: str, url: str) -> Tuple[Path, bool, bool]:
    """
    Write a STRM file and return the filepath, whether it was newly created, and if URL changed.
    Returns: (filepath, is_new, url_changed)
    """
    directory.mkdir(parents=True, exist_ok=True)
    filepath = directory / f"{filename}.strm"
    is_new = not filepath.exists()
    
    # Read existing content to check if URL changed
    url_changed = False
    if not is_new:
        try:
            existing_url = filepath.read_text(encoding="utf-8").strip()
            url_changed = existing_url != url
        except Exception:
            url_changed = True
    
    filepath.write_text(url, encoding="utf-8")
    if is_new:
        logger.debug(f"Created new STRM file: {filepath}")
    elif url_changed:
        logger.debug(f"Updated STRM file (URL changed): {filepath}")
    return filepath, is_new, url_changed

def convert_to_emby_path(file_path: Path) -> str:
    """
    Convert container path to the path that Emby sees on the host.
    Uses path mapping environment variables if set.
    """
    path_str = str(file_path)
    
    # Map container paths to host paths if environment variables are set
    if EMBY_MOVIES_PATH and path_str.startswith(str(MOVIES_DIR)):
        # Replace container movies path with host path
        relative_path = path_str[len(str(MOVIES_DIR)):].lstrip('/')
        return str(Path(EMBY_MOVIES_PATH) / relative_path) if relative_path else EMBY_MOVIES_PATH
    
    if EMBY_SERIES_PATH and path_str.startswith(str(SERIES_DIR)):
        # Replace container series path with host path
        relative_path = path_str[len(str(SERIES_DIR)):].lstrip('/')
        return str(Path(EMBY_SERIES_PATH) / relative_path) if relative_path else EMBY_SERIES_PATH
    
    if EMBY_LIVETV_PATH and path_str.startswith(str(LIVETV_DIR)):
        # Replace container livetv path with host path
        relative_path = path_str[len(str(LIVETV_DIR)):].lstrip('/')
        return str(Path(EMBY_LIVETV_PATH) / relative_path) if relative_path else EMBY_LIVETV_PATH
    
    # If no mapping is set, return original path (assumes paths match)
    return path_str

def get_emby_item_by_path(file_path: Path) -> Optional[str]:
    """
    Find an Emby item ID by file path.
    Returns the item ID if found, None otherwise.
    """
    if not EMBY_SERVER_URL or not EMBY_API_KEY:
        return None
    
    try:
        # Convert container path to Emby host path
        path_str = convert_to_emby_path(file_path)
        logger.debug(f"Looking up Emby item for path: {path_str} (container: {file_path})")
        
        # Query Emby for items at this path
        url = f"{EMBY_SERVER_URL.rstrip('/')}/emby/Items"
        params = {
            "Path": path_str,
            "Recursive": "false"
        }
        headers = {
            "X-Emby-Token": EMBY_API_KEY
        }
        
        resp = requests.get(url, params=params, headers=headers, timeout=10)
        resp.raise_for_status()
        
        data = resp.json()
        if "Items" in data and len(data["Items"]) > 0:
            return data["Items"][0]["Id"]
        
        return None
    except Exception as e:
        logger.warning(f"⚠️ Error finding Emby item for {file_path}: {e}")
        return None

def refresh_emby_item(item_id: str, update_type: str = "Update"):
    """
    Refresh a specific Emby item.
    update_type: "Update" for existing items, "Add" for new items
    """
    if not EMBY_SERVER_URL or not EMBY_API_KEY:
        return False
    
    try:
        url = f"{EMBY_SERVER_URL.rstrip('/')}/emby/Items/{item_id}/Refresh"
        params = {
            "Recursive": "false",
            "ImageRefreshMode": "FullRefresh",
            "MetadataRefreshMode": "FullRefresh",
            "ReplaceAllImages": "false",
            "ReplaceAllMetadata": "false"
        }
        headers = {
            "X-Emby-Token": EMBY_API_KEY
        }
        
        resp = requests.post(url, params=params, headers=headers, timeout=10)
        resp.raise_for_status()
        logger.info(f"✅ Emby item {item_id} refreshed ({update_type})")
        return True
    except Exception as e:
        logger.warning(f"⚠️ Error refreshing Emby item {item_id}: {e}")
        return False

def refresh_emby_library_path(path: Path):
    """
    Trigger Emby library refresh for a specific path (for new items).
    This triggers a library scan of the specified path to detect and add new files.
    """
    if not EMBY_SERVER_URL or not EMBY_API_KEY:
        return False
    
    try:
        # Convert container path to Emby host path
        emby_path = convert_to_emby_path(path)
        logger.debug(f"Triggering Emby library refresh for path: {emby_path} (container: {path})")
        
        # POST to Library/Refresh endpoint - this triggers an actual library scan/refresh
        # The Path parameter tells Emby which directory to scan for new files
        url = f"{EMBY_SERVER_URL.rstrip('/')}/emby/Library/Refresh"
        params = {
            "Path": emby_path,
            "Recursive": "true"
        }
        headers = {
            "X-Emby-Token": EMBY_API_KEY
        }
        
        resp = requests.post(url, params=params, headers=headers, timeout=10)
        resp.raise_for_status()
        logger.info(f"✅ Emby library refresh triggered for {path} (will scan for new files)")
        return True
    except Exception as e:
        logger.warning(f"⚠️ Error triggering Emby library refresh for {path}: {e}")
        return False

def delete_emby_item(item_id: str):
    """
    Delete an item from Emby library.
    Returns True if successful, False otherwise.
    """
    if not EMBY_SERVER_URL or not EMBY_API_KEY:
        return False
    
    try:
        url = f"{EMBY_SERVER_URL.rstrip('/')}/emby/Items/{item_id}"
        headers = {
            "X-Emby-Token": EMBY_API_KEY
        }
        
        resp = requests.delete(url, headers=headers, timeout=10)
        resp.raise_for_status()
        logger.info(f"✅ Emby item {item_id} deleted from library")
        return True
    except Exception as e:
        logger.warning(f"⚠️ Error deleting Emby item {item_id}: {e}")
        return False

def notify_emby_updated(filepath: Path):
    """
    Notify Emby about an updated STRM file (URL changed).
    Finds the item and refreshes it directly (more efficient than library scan).
    """
    if not EMBY_SERVER_URL or not EMBY_API_KEY:
        logger.debug(f"Emby notification skipped (not configured) for {filepath}")
        return
    
    # For updated items (URL changed), find the item and refresh it directly
    # This avoids full library scans and only updates the specific item
    item_id = get_emby_item_by_path(filepath)
    if item_id:
        refresh_emby_item(item_id, "Update")
    else:
        # If item not found, try library refresh as fallback
        logger.warning(f"⚠️ Item not found in Emby for {filepath}, triggering library refresh")
        refresh_emby_library_path(filepath.parent)

def batch_refresh_directories(directories: Set[Path]):
    """
    Batch refresh multiple directories in Emby.
    Only refreshes directories that actually have new files.
    """
    if not EMBY_SERVER_URL or not EMBY_API_KEY:
        return
    
    if not directories:
        return
    
    logger.info(f"Triggering Emby library refresh for {len(directories)} directory/directories with new files")
    for directory in directories:
        # For new items, trigger library refresh for the parent directory
        # This will scan the directory and add new STRM files to Emby's library
        # The Library/Refresh endpoint triggers an actual library scan, not just a search
        refresh_emby_library_path(directory)

def cleanup_empty_dirs(base_dir: Path):
    for d in base_dir.iterdir():
        if d.is_dir() and not any(f.suffix == ".strm" for f in d.rglob("*")):
            logger.info(f"🗑 Removing empty or orphaned directory: {d}")
            for f in d.rglob("*"):
                f.unlink()
            d.rmdir()

def find_all_strm_files(base_dir: Path) -> Set[Path]:
    """Find all existing STRM files in a directory."""
    strm_files = set()
    if base_dir.exists():
        for strm_file in base_dir.rglob("*.strm"):
            strm_files.add(strm_file)
    return strm_files

def cleanup_orphaned_files(processed_files: Set[Path], base_dir: Path, content_type: str):
    """Remove STRM files that exist in the directory but weren't in the current playlist."""
    if not REMOVE_ORPHANED:
        return
    
    existing_files = find_all_strm_files(base_dir)
    orphaned_files = existing_files - processed_files
    
    if orphaned_files:
        logger.info(f"Found {len(orphaned_files)} orphaned {content_type} STRM file(s) to remove")
        deleted_count = 0
        for orphaned_file in orphaned_files:
            try:
                # Try to find and delete the item from Emby before removing the file
                if EMBY_SERVER_URL and EMBY_API_KEY:
                    item_id = get_emby_item_by_path(orphaned_file)
                    if item_id:
                        delete_emby_item(item_id)
                        deleted_count += 1
                    else:
                        logger.debug(f"Item not found in Emby for {orphaned_file}, skipping Emby deletion")
                
                # Remove the file from filesystem
                orphaned_file.unlink()
                logger.debug(f"🗑 Removed orphaned file: {orphaned_file}")
            except Exception as e:
                logger.warning(f"⚠️ Error removing orphaned file {orphaned_file}: {e}")
        
        if deleted_count > 0:
            logger.info(f"✅ Deleted {deleted_count} orphaned {content_type} item(s) from Emby library")
        
        # Clean up empty directories after removing orphaned files
        cleanup_empty_dirs(base_dir)

def process_playlist():
    movies = []
    series = []
    live_tv = []

    lines = TMP_PLAYLIST.read_text(encoding="utf-8").splitlines()
    logger.debug(f"Playlist has {len(lines)} total lines")
    
    extinf_count = 0
    no_tvg_name_count = 0
    i = 0
    while i < len(lines):
        if not lines[i].startswith("#EXTINF"):
            i += 1
            continue
        extinf_count += 1
        info = lines[i]
        url = lines[i+1] if i+1 < len(lines) else ""
        i += 2

        tvg_match = re.search(r'tvg-name="([^"]+)"', info)
        tvg_name = tvg_match.group(1).strip() if tvg_match else ""

        if not tvg_name:
            # Skip items without tvg-name
            no_tvg_name_count += 1
            logger.debug(f"Skipping item without tvg-name: {info[:100]}...")
            continue

        # Extract group-title for classification (for logging/debugging)
        group_match = re.search(r'group-title="([^"]+)"', info)
        group_title = group_match.group(1).strip().lower() if group_match else ""
        
        # URL pattern is the most reliable indicator
        # Movies: /movie/ in URL path
        # Series: /series/ in URL path
        # Live TV: neither pattern (just username/password/number)
        url_lower = url.lower()
        is_series = False
        is_movie = False
        
        # Check URL patterns first (most reliable)
        # Look for /movie/ or /series/ as path segments
        if "/movie/" in url_lower:
            is_movie = True
        elif "/series/" in url_lower:
            is_series = True
        # If URL doesn't have /movie/ or /series/, it's live TV
        # (regardless of group-title)
        
        if is_series:
            series.append((tvg_name, url))
            logger.debug(f"Found series: {tvg_name} (group: {group_title})")
        elif is_movie:
            movies.append((tvg_name, url))
            logger.debug(f"Found movie: {tvg_name} (group: {group_title})")
        else:
            live_tv.append(info + "\n" + url)
    
    logger.info(f"Found {extinf_count} #EXTINF entries")
    if no_tvg_name_count > 0:
        logger.warning(f"Skipped {no_tvg_name_count} entries without tvg-name")
    logger.info(f"Classified: {len(movies)} movies, {len(series)} series, {len(live_tv)} live TV")

    # Safety check: If playlist is empty or has no valid entries, skip orphan cleanup
    # This prevents mass deletion when download fails or playlist is invalid
    total_valid_items = len(movies) + len(series) + len(live_tv)
    if total_valid_items == 0:
        logger.warning("⚠️ Playlist contains no valid items (0 movies, 0 series, 0 live TV)")
        logger.warning("⚠️ Skipping orphan cleanup to prevent accidental mass deletion")
        logger.warning("⚠️ This may indicate a download failure or invalid playlist file")
        return

    # Track all processed files for orphan cleanup
    processed_movie_files = set()
    processed_series_files = set()
    
    # Track directories with new files for batch refresh
    new_movie_directories = set()
    new_series_directories = set()
    
    # Track total items processed (for limit)
    total_items_processed = 0
    items_limit_reached = False

    # Process movies
    logger.info(f"Processing {len(movies)} movie(s)")
    if MAX_ITEMS_PER_RUN > 0:
        logger.info(f"Item processing limit: {MAX_ITEMS_PER_RUN} (movies + series, excluding Live TV)")
    
    movies_added = 0
    movies_updated = 0
    movies_processed = 0
    for tvg_name, url in movies:
        # Check if limit reached
        if MAX_ITEMS_PER_RUN > 0 and total_items_processed >= MAX_ITEMS_PER_RUN:
            movies_skipped = len(movies) - movies_processed
            items_limit_reached = True
            logger.warning(f"⚠️ Item processing limit reached ({MAX_ITEMS_PER_RUN}). Skipping remaining {movies_skipped} movie(s)")
            break
        
        folder_name = parse_movie_name(tvg_name)
        filepath, is_new, url_changed = write_strm_file(MOVIES_DIR / folder_name, folder_name, url)
        processed_movie_files.add(filepath)
        total_items_processed += 1
        movies_processed += 1
        
        if is_new:
            movies_added += 1
            # Track directory for batch refresh (only if new)
            new_movie_directories.add(filepath.parent)
        elif url_changed:
            movies_updated += 1
            # For updated files, refresh the specific item immediately
            notify_emby_updated(filepath)
    
    movies_skipped = len(movies) - movies_processed
    if movies_added > 0 or movies_updated > 0:
        logger.info(f"Movies: {movies_added} added, {movies_updated} updated")
    if movies_skipped > 0:
        logger.info(f"Movies: {movies_skipped} skipped due to processing limit")
    
    # Batch refresh directories with new movie files
    if new_movie_directories:
        batch_refresh_directories(new_movie_directories)

    # Process series
    logger.info(f"Processing {len(series)} series episode(s)")
    series_added = 0
    series_updated = 0
    series_processed = 0
    for tvg_name, url in series:
        # Check if limit reached
        if MAX_ITEMS_PER_RUN > 0 and total_items_processed >= MAX_ITEMS_PER_RUN:
            series_skipped = len(series) - series_processed
            items_limit_reached = True
            logger.warning(f"⚠️ Item processing limit reached ({MAX_ITEMS_PER_RUN}). Skipping remaining {series_skipped} series episode(s)")
            break
        
        folder_name = parse_series_name(tvg_name)
        season, episode = extract_season_episode(tvg_name)
        filepath, is_new, url_changed = write_strm_file(SERIES_DIR / folder_name / season, episode, url)
        processed_series_files.add(filepath)
        total_items_processed += 1
        series_processed += 1
        
        if is_new:
            series_added += 1
            # Track directory for batch refresh (only if new)
            new_series_directories.add(filepath.parent)
        elif url_changed:
            series_updated += 1
            # For updated files, refresh the specific item immediately
            notify_emby_updated(filepath)
    
    series_skipped = len(series) - series_processed
    if series_added > 0 or series_updated > 0:
        logger.info(f"Series: {series_added} added, {series_updated} updated")
    if series_skipped > 0:
        logger.info(f"Series: {series_skipped} skipped due to processing limit")
    
    # Batch refresh directories with new series files
    if new_series_directories:
        batch_refresh_directories(new_series_directories)
    
    if items_limit_reached:
        logger.info(f"ℹ️ Processed {total_items_processed} items (limit: {MAX_ITEMS_PER_RUN}). Remaining items will be processed on next run.")

    # Process live TV
    if live_tv:
        logger.info(f"Processing {len(live_tv)} live TV channel(s)")
        LIVETV_DIR.mkdir(parents=True, exist_ok=True)
        live_file = LIVETV_DIR / "livetv.m3u"
        live_file.write_text("\n".join(live_tv), encoding="utf-8")
        logger.debug(f"Live TV playlist saved to {live_file}")

    # Cleanup orphaned files (files that exist but weren't in current playlist)
    cleanup_orphaned_files(processed_movie_files, MOVIES_DIR, "movie")
    cleanup_orphaned_files(processed_series_files, SERIES_DIR, "series")

    # Cleanup empty directories
    if REMOVE_FILES:
        cleanup_empty_dirs(MOVIES_DIR)
        cleanup_empty_dirs(SERIES_DIR)

    logger.info("✅ Processing complete.")

def main():
    logger.info("Starting M3U to STRM converter")
    while True:
        try:
            download_playlist()
            process_playlist()
        except Exception as e:
            logger.error(f"❌ Error: {e}", exc_info=True)
        if INTERVAL_SECONDS <= 0:
            break
        if INTERVAL_SECONDS > 0:
            logger.info(f"Waiting {INTERVAL_SECONDS} seconds until next update...")
        time.sleep(INTERVAL_SECONDS)

if __name__ == "__main__":
    main()
