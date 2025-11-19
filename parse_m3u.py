#!/usr/bin/env python3
import os
import re
import sys
import time
import logging
import requests
from pathlib import Path
from typing import Tuple, Optional

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
INTERVAL_SECONDS = int(os.environ.get("INTERVAL_SECONDS", "0"))

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
    return re.sub(r'[\\/:"*?<>|]+', '', name).strip()

def parse_movie_name(tvg_name: str):
    # Remove language prefix like "EN - "
    tvg_name = re.sub(r'^[A-Z]{1,3}\s*-\s*', '', tvg_name).strip()
    # Preserve year at end if present
    year_match = re.search(r'\(\d{4}\)$', tvg_name)
    year = year_match.group(0) if year_match else ''
    # Remove any other parentheses content
    tvg_name = re.sub(r'\s*\([^()]*\)(?!$)', '', tvg_name).strip()
    if year:
        tvg_name += f" {year}"
    return safe_filename(tvg_name)

def parse_series_name(tvg_name: str):
    tvg_name = re.sub(r'^[A-Z]{1,3}\s*-\s*', '', tvg_name).strip()
    # Extract year at end
    year_match = re.search(r'\(\d{4}\)$', tvg_name)
    year = year_match.group(0) if year_match else ''
    tvg_name = re.sub(r'\s*\([^()]*\)(?!$)', '', tvg_name).strip()
    # Remove season/episode from name
    tvg_name = re.sub(r'\s+S\d{1,2}\s*E\d{1,2}.*$', '', tvg_name).strip()
    if year:
        tvg_name += f" {year}"
    return safe_filename(tvg_name)

def extract_season_episode(tvg_name: str):
    match = re.search(r'S(\d{1,2})\s*E(\d{1,2})', tvg_name, re.IGNORECASE)
    if match:
        season = f"Season {int(match.group(1))}"
        episode = f"S{int(match.group(1)):02d}E{int(match.group(2)):02d}"
        return season, episode
    return "Season 1", "S01E01"

def write_strm_file(directory: Path, filename: str, url: str) -> Tuple[Path, bool]:
    """
    Write a STRM file and return the filepath and whether it was newly created.
    Returns: (filepath, is_new)
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
    return filepath, is_new or url_changed

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
    This is more efficient than a full library scan.
    """
    if not EMBY_SERVER_URL or not EMBY_API_KEY:
        return False
    
    try:
        # Convert container path to Emby host path
        emby_path = convert_to_emby_path(path)
        logger.debug(f"Refreshing Emby library path: {emby_path} (container: {path})")
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
        logger.info(f"✅ Emby library refresh triggered for {path}")
        return True
    except Exception as e:
        logger.warning(f"⚠️ Error refreshing Emby library path {path}: {e}")
        return False

def notify_emby(filepath: Path, is_new: bool):
    """
    Notify Emby about a STRM file change.
    For new files: trigger library refresh for the parent directory
    For updated files: find item and refresh it
    """
    if not EMBY_SERVER_URL or not EMBY_API_KEY:
        logger.debug(f"Emby notification skipped (not configured) for {filepath}")
        return
    
    if is_new:
        # For new items, trigger library refresh for the parent directory
        # This will add the new item without a full library scan
        refresh_emby_library_path(filepath.parent)
    else:
        # For updated items, find the item and refresh it
        item_id = get_emby_item_by_path(filepath)
        if item_id:
            refresh_emby_item(item_id, "Update")
        else:
            # If item not found, try library refresh as fallback
            logger.warning(f"⚠️ Item not found in Emby for {filepath}, triggering library refresh")
            refresh_emby_library_path(filepath.parent)

def cleanup_empty_dirs(base_dir: Path):
    for d in base_dir.iterdir():
        if d.is_dir() and not any(f.suffix == ".strm" for f in d.rglob("*")):
            logger.info(f"🗑 Removing empty or orphaned directory: {d}")
            for f in d.rglob("*"):
                f.unlink()
            d.rmdir()

def process_playlist():
    movies = []
    series = []
    live_tv = []

    lines = TMP_PLAYLIST.read_text(encoding="utf-8").splitlines()
    i = 0
    while i < len(lines):
        if not lines[i].startswith("#EXTINF"):
            i += 1
            continue
        info = lines[i]
        url = lines[i+1] if i+1 < len(lines) else ""
        i += 2

        tvg_match = re.search(r'tvg-name="([^"]+)"', info)
        tvg_name = tvg_match.group(1).strip() if tvg_match else ""

        if not tvg_name:
            # Skip items without tvg-name
            continue

        if "/series/" in url:
            series.append((tvg_name, url))
        elif "/movie/" in url:
            movies.append((tvg_name, url))
        else:
            live_tv.append(info + "\n" + url)

    # Process movies
    logger.info(f"Processing {len(movies)} movie(s)")
    for tvg_name, url in movies:
        folder_name = parse_movie_name(tvg_name)
        filepath, is_new = write_strm_file(MOVIES_DIR / folder_name, folder_name, url)
        notify_emby(filepath, is_new)

    # Process series
    logger.info(f"Processing {len(series)} series episode(s)")
    for tvg_name, url in series:
        folder_name = parse_series_name(tvg_name)
        season, episode = extract_season_episode(tvg_name)
        filepath, is_new = write_strm_file(SERIES_DIR / folder_name / season, episode, url)
        notify_emby(filepath, is_new)

    # Process live TV
    if live_tv:
        logger.info(f"Processing {len(live_tv)} live TV channel(s)")
        LIVETV_DIR.mkdir(parents=True, exist_ok=True)
        live_file = LIVETV_DIR / "livetv.m3u"
        live_file.write_text("\n".join(live_tv), encoding="utf-8")
        logger.debug(f"Live TV playlist saved to {live_file}")

    # Cleanup
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
