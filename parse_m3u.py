#!/usr/bin/env python3
import os
import re
import time
import requests
from pathlib import Path

# Environment variables
EMBY_SERVER_URL = os.environ.get("EMBY_SERVER_URL")
EMBY_API_KEY = os.environ.get("EMBY_API_KEY")
M3U_URL = os.environ.get("M3U_URL")
MOVIES_DIR = Path("/usr/src/app/movies")
SERIES_DIR = Path("/usr/src/app/series")
LIVETV_DIR = Path("/usr/src/app/livetv")
REMOVE_FILES = os.environ.get("REMOVE_FILES", "false").lower() == "true"
INTERVAL_SECONDS = int(os.environ.get("INTERVAL_SECONDS", "0"))

# Temporary M3U file
TMP_PLAYLIST = Path("/tmp/playlist.m3u")

def download_playlist():
    print(f"⬇️ Downloading playlist from {M3U_URL}")
    resp = requests.get(M3U_URL)
    resp.raise_for_status()
    TMP_PLAYLIST.write_text(resp.text, encoding="utf-8")
    print(f"✅ Saved to {TMP_PLAYLIST}")

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

def write_strm_file(directory: Path, filename: str, url: str):
    directory.mkdir(parents=True, exist_ok=True)
    filepath = directory / f"{filename}.strm"
    filepath.write_text(url, encoding="utf-8")

def cleanup_empty_dirs(base_dir: Path):
    for d in base_dir.iterdir():
        if d.is_dir() and not any(f.suffix == ".strm" for f in d.rglob("*")):
            print(f"🗑 Removing empty or orphaned directory: {d}")
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
    for tvg_name, url in movies:
        folder_name = parse_movie_name(tvg_name)
        write_strm_file(MOVIES_DIR / folder_name, folder_name, url)

    # Process series
    for tvg_name, url in series:
        folder_name = parse_series_name(tvg_name)
        season, episode = extract_season_episode(tvg_name)
        write_strm_file(SERIES_DIR / folder_name / season, episode, url)

    # Process live TV
    if live_tv:
        LIVETV_DIR.mkdir(parents=True, exist_ok=True)
        live_file = LIVETV_DIR / "livetv.m3u"
        live_file.write_text("\n".join(live_tv), encoding="utf-8")

    # Cleanup
    if REMOVE_FILES:
        cleanup_empty_dirs(MOVIES_DIR)
        cleanup_empty_dirs(SERIES_DIR)

    print("✅ Processing complete.")

def main():
    while True:
        try:
            download_playlist()
            process_playlist()
        except Exception as e:
            print(f"❌ Error: {e}")
        if INTERVAL_SECONDS <= 0:
            break
        time.sleep(INTERVAL_SECONDS)

if __name__ == "__main__":
    main()
