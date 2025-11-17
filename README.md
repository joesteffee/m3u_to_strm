# M3U to STRM Converter

A Python script that downloads M3U playlists and converts them into `.strm` files for use with media servers like Emby/Jellyfin. The script organizes content into separate directories for movies, series, and live TV.

## Features

- Downloads M3U playlists from a remote URL
- Parses and organizes content into:
  - **Movies**: Individual `.strm` files in movie-named folders
  - **Series**: Organized by show name, season, and episode
  - **Live TV**: Combined into a single `livetv.m3u` file
- Supports automatic cleanup of empty directories
- Configurable interval for periodic updates
- Dockerized for easy deployment

## Requirements

- Python 3.9+
- Docker (for containerized deployment)

## Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `M3U_URL` | URL to the M3U playlist file | Yes |
| `EMBY_SERVER_URL` | Emby server URL (for future use) | No |
| `EMBY_API_KEY` | Emby API key (for future use) | No |
| `REMOVE_FILES` | Set to "true" to remove empty directories | No (default: false) |
| `INTERVAL_SECONDS` | Seconds between playlist updates (0 = run once) | No (default: 0) |

## Directory Structure

The script creates the following directory structure:

```
/usr/src/app/
├── movies/
│   └── [Movie Name (Year)]/
│       └── [Movie Name (Year)].strm
├── series/
│   └── [Series Name (Year)]/
│       └── Season X/
│           └── SXXEXX.strm
└── livetv/
    └── livetv.m3u
```

## Usage

### Docker

1. Build the Docker image:
   ```bash
   docker build -t m3u_to_strm:latest .
   ```

2. Run the container:
   ```bash
   docker run -d \
     -e M3U_URL="https://example.com/playlist.m3u" \
     -e REMOVE_FILES="true" \
     -e INTERVAL_SECONDS=3600 \
     -v /path/to/movies:/usr/src/app/movies \
     -v /path/to/series:/usr/src/app/series \
     -v /path/to/livetv:/usr/src/app/livetv \
     m3u_to_strm:latest
   ```

### Local Python

1. Install dependencies:
   ```bash
   pip install -r requirements.txt
   ```

2. Set environment variables and run:
   ```bash
   export M3U_URL="https://example.com/playlist.m3u"
   python parse_m3u.py
   ```

## How It Works

1. Downloads the M3U playlist from the specified URL
2. Parses each entry to extract:
   - TV guide name (`tvg-name`)
   - Stream URL
   - Content type (movie/series/live TV)
3. For movies: Creates a folder named after the movie and a `.strm` file inside
4. For series: Organizes by show name, season, and episode number
5. For live TV: Combines all live TV streams into a single M3U file
6. Optionally removes empty directories if `REMOVE_FILES=true`

## Notes

- The script automatically sanitizes filenames to remove invalid characters
- Language prefixes (e.g., "EN - ") are automatically removed from titles
- Year information is preserved when present in the original title
- Series episodes are extracted from the `tvg-name` field using S##E## format

## License

This project is provided as-is for personal use.

