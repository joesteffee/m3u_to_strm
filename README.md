# M3U to STRM Converter

A single static binary that downloads an M3U playlist and converts it into `.strm` files for
Emby/Jellyfin. Content is sorted into movie, series and live-TV directories, and Emby is asked to
refresh only the items that changed instead of rescanning the whole library.

Rewritten in Rust from the original Python script. Same environment variables, same container
paths, same output layout — the image is a drop-in replacement.

## Features

- Downloads an M3U playlist from a URL, with a configurable local cache.
- Classifies every entry by its stream URL into:
  - **Movies**: one `.strm` file per title inside a folder named after the movie
  - **Series**: `series/<show>/Season X/SXXEXX.strm`
  - **Live TV**: a single combined `livetv.m3u`
- **Emby integration**: refreshes only the item or folder that changed (no full library scans).
- Optional removal of orphaned `.strm` files and of directories left empty.
- Optional country-code filtering of live-TV channels.
- Repeatable passes on an interval, or a single pass and exit.
- Timestamped logging to stdout.
- One static binary in a `scratch` image: no interpreter, no runtime dependencies, ~3 MB.

## Environment variables

| Variable | Description | Required |
|----------|-------------|----------|
| `M3U_URL` | URL of the M3U playlist file | Yes |
| `EMBY_SERVER_URL` | Emby server URL (e.g. `http://emby:8096`) | No |
| `EMBY_API_KEY` | Emby API key for library refresh notifications | No (required together with `EMBY_SERVER_URL`) |
| `EMBY_MOVIES_PATH` | Host path Emby uses for movies, if it differs from the container path | No |
| `EMBY_SERIES_PATH` | Host path Emby uses for series, if it differs from the container path | No |
| `EMBY_LIVETV_PATH` | Host path Emby uses for live TV, if it differs from the container path | No |
| `REMOVE_FILES` | `true` removes directories left empty | No (default: false) |
| `REMOVE_ORPHANED` | `true` removes `.strm` files that disappeared from the playlist | No (default: false) |
| `INTERVAL_SECONDS` | Seconds between passes (`0` = run once and exit) | No (default: 0) |
| `MAX_ITEMS_PER_RUN` | Maximum items (movies + series) per pass; live TV is exempt. `0` = no limit | No (default: 0) |
| `M3U_CACHE_HOURS` | Hours a downloaded playlist stays valid. Re-downloads when missing, empty or older | No (default: 8) |
| `INCLUDE_COUNTRY_CODES` | Comma-separated 2-letter codes; keep only those live-TV channels (e.g. `US,GB`) | No |
| `FILTER_COUNTRY_CODES` | Comma-separated 2-letter codes; drop those live-TV channels | No |
| `LOG_LEVEL` | `DEBUG`, `INFO`, `WARNING` or `ERROR` | No (default: INFO) |

Only the exact string `true` (any case) enables `REMOVE_FILES` and `REMOVE_ORPHANED`.
`INCLUDE_COUNTRY_CODES` wins over `FILTER_COUNTRY_CODES`. Channel codes are read from a leading
`XX|` in `tvg-name`; channels without a code are never filtered.

## Directory structure

```
/usr/src/app/
├── movies/
│   └── [Movie Name (Year)]/
│       └── [Movie Name (Year)].strm
├── series/
│   └── [Series Name]/
│       └── Season X/
│           └── SXXEXX.strm
└── livetv/
    └── livetv.m3u
```

## Running the container

Pull the published image (multi-arch: `linux/amd64` and `linux/arm64`):

```bash
docker pull ghcr.io/joesteffee/m3u_to_strm:latest

docker run -d \
  -e M3U_URL="https://example.com/playlist.m3u" \
  -e EMBY_SERVER_URL="http://emby:8096" \
  -e EMBY_API_KEY="your-api-key-here" \
  -e EMBY_MOVIES_PATH="/mnt/media/movies" \
  -e EMBY_SERIES_PATH="/mnt/media/series" \
  -e EMBY_LIVETV_PATH="/mnt/media/livetv" \
  -e REMOVE_FILES="true" \
  -e INTERVAL_SECONDS=3600 \
  -v /path/to/movies:/usr/src/app/movies \
  -v /path/to/series:/usr/src/app/series \
  -v /path/to/livetv:/usr/src/app/livetv \
  --name m3u-to-strm \
  ghcr.io/joesteffee/m3u_to_strm:latest
```

`EMBY_*_PATH` is only needed when Emby sees the media under different paths than the container
does. With those three variables set correctly, a changed title is refreshed in place rather than
by a library-wide scan.

To build the image locally instead:

```bash
docker build -t ghcr.io/joesteffee/m3u_to_strm:local .
```

### Troubleshooting

**`ERROR - ❌ Error: permission denied (os error 13)`** — the container writes as UID 99 / GID 100, so
the three mounted directories have to be writable by that user:

```bash
sudo chown -R 99:100 /path/to/movies /path/to/series /path/to/livetv
```

On a host with SELinux enforcing (Fedora, RHEL, Rocky) a bind mount also needs a label, otherwise the
write is refused even when the ownership is right. Add `:z` to every mount:

```bash
-v /path/to/movies:/usr/src/app/movies:z
```

unRAID does not use SELinux, so `:z` is not needed there.

**The playlist looks stale** — it is cached at `/tmp/playlist.m3u` inside the container for
`M3U_CACHE_HOURS` (default 8) hours. Delete that file, or lower the cache time, to force a download.

### unRAID

1. Copy the template below to `/boot/config/plugins/dockerMan/templates-user/my-m3u-to-strm.xml`,
   or use **Docker → Add Container → Add Container** and paste it into the template editor.

   ```xml
   <?xml version="1.0"?>
   <Container>
     <Repository>ghcr.io/joesteffee/m3u_to_strm</Repository>
     <Registry>https://cloud.docker.com/</Registry>
     <Name>m3u-to-strm</Name>
     <Privileged>false</Privileged>
     <Support/>
     <Project>m3u_to_strm</Project>
     <Overview>Converts an M3U playlist into STRM files for Emby/Jellyfin.</Overview>
     <Category>Tools:</Category>
     <WebUI/>
     <TemplateURL>false</TemplateURL>
     <Icon>docker</Icon>
     <ExtraParams/>
     <PostArgs/>
     <CPUset/>
     <DateInstalled>0</DateInstalled>
     <DonateText/>
     <DonateLink/>
     <Requires>Set M3U_URL. Emby variables are optional.</Requires>
     <Config Name="movies" Target="/usr/src/app/movies" Default="/mnt/user/media/movies" Mode="rw"
             Description="Movie library mount" Type="Path" Display="always" Required="true" Mask="false"/>
     <Config Name="series" Target="/usr/src/app/series" Default="/mnt/user/media/series" Mode="rw"
             Description="Series library mount" Type="Path" Display="always" Required="true" Mask="false"/>
     <Config Name="livetv" Target="/usr/src/app/livetv" Default="/mnt/user/media/livetv" Mode="rw"
             Description="Live TV mount" Type="Path" Display="always" Required="true" Mask="false"/>
     <Config Name="M3U_URL" Default="" Mode="rw" Description="Playlist URL"
             Type="Variable" Display="always" Required="true" Mask="false"/>
     <Config Name="EMBY_SERVER_URL" Default="" Mode="rw" Description="Emby URL, e.g. http://192.168.1.10:8096"
             Type="Variable" Display="always" Required="false" Mask="false"/>
     <Config Name="EMBY_API_KEY" Default="" Mode="rw" Description="Emby API key"
             Type="Variable" Display="always" Required="false" Mask="true"/>
     <Config Name="EMBY_MOVIES_PATH" Default="/mnt/user/media/movies" Mode="rw"
             Description="Path Emby sees for movies" Type="Variable" Display="always" Required="false" Mask="false"/>
     <Config Name="EMBY_SERIES_PATH" Default="/mnt/user/media/series" Mode="rw"
             Description="Path Emby sees for series" Type="Variable" Display="always" Required="false" Mask="false"/>
     <Config Name="EMBY_LIVETV_PATH" Default="/mnt/user/media/livetv" Mode="rw"
             Description="Path Emby sees for live TV" Type="Variable" Display="always" Required="false" Mask="false"/>
     <Config Name="REMOVE_FILES" Default="true" Mode="rw" Description="true removes empty directories"
             Type="Variable" Display="always" Required="false" Mask="false"/>
     <Config Name="REMOVE_ORPHANED" Default="false" Mode="rw" Description="true removes dropped STRM files"
             Type="Variable" Display="always" Required="false" Mask="false"/>
     <Config Name="INTERVAL_SECONDS" Default="3600" Mode="rw" Description="Seconds between passes, 0 runs once"
             Type="Variable" Display="always" Required="false" Mask="false"/>
     <Config Name="MAX_ITEMS_PER_RUN" Default="0" Mode="rw" Description="Max new items per pass, 0 is unlimited"
             Type="Variable" Display="always" Required="false" Mask="false"/>
     <Config Name="M3U_CACHE_HOURS" Default="8" Mode="rw" Description="Playlist cache lifetime in hours"
             Type="Variable" Display="always" Required="false" Mask="false"/>
   </Container>
   ```

2. The container runs as user `99:100` (unRAID's `nobody:users`). Make sure that user can write to
   the three mounted directories: `chown -R 99:100 /mnt/user/media/{movies,series,livetv}`.
3. Start the container and check the log. With `INTERVAL_SECONDS=0` it converts once and exits,
   which suits an external scheduler; with a positive value it stays up and repeats.

## Command-line options

| Option | Effect |
|--------|--------|
| `--once` | Run a single pass and exit, even when `INTERVAL_SECONDS` is set |
| `--explain` | Print the resolved configuration and exit (no network, no writes) |
| `-h`, `--help` | Usage and environment summary |
| `-V`, `--version` | Version |

`--explain` is the quickest way to check that the environment is wired up correctly:

```bash
docker run --rm -e M3U_URL="https://example.com/playlist.m3u" \
  ghcr.io/joesteffee/m3u_to_strm:latest --explain
```

## How it works

1. Download the playlist, unless the cached copy is younger than `M3U_CACHE_HOURS`.
2. Read each `#EXTINF` entry, taking the title from `tvg-name` and the stream from the next line.
   Entries without a `tvg-name` are counted and skipped.
3. Classify by stream URL: `/movie/` is a movie, `/series/` is a series, everything else is live TV.
4. Write the files. Existing entries are only rewritten when their URL changed, so a repeat pass
   touches almost nothing. When a title has more than one version, files get an id suffix
   (`Title [123456].strm`) taken from the stream URL.
5. Tell Emby about the changes: new folders get a folder refresh, changed items get an item
   refresh, and — with `REMOVE_ORPHANED` — deleted items are removed from Emby before the file goes.
6. Remove orphaned `.strm` files, then optionally the directories left empty.

A playlist that yields no usable entries is treated as a failed download: nothing is deleted, the
cache file is removed so the next pass re-downloads, and three warnings are logged.

## Development

```bash
cargo test          # unit, parity and end-to-end tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

- `src/` holds the binary: `config`, `naming`, `playlist`, `strm`, `emby`, `runner`, plus small
  `fsx`, `http`, `paths`, `pytext` and `log` modules.
- `tests/parity.rs` replays `tests/fixtures/parity.json` — 388 inputs with the outputs recorded
  from the original Python implementation, including every filename-parsing and file-selection
  rule — so the Python contract stays enforceable now that the Python source is gone.
- `tests/pipeline.rs` runs the real code end to end against a local HTTP server for the playlist
  and a recording mock Emby server, covering classification, id-based filenames, orphan cleanup,
  `MAX_ITEMS_PER_RUN`, empty-playlist safety and the exact Emby requests made.
- `.github/workflows/ci.yml` runs format, clippy and tests.
  `.github/workflows/publish-container.yml` builds and pushes multi-arch images to `ghcr.io` on
  every push to `main` (plus `latest`) and on `v*.*.*` tags (semver tags).

## Differences from the Python original

The behaviour contract is the same, verified by the parity fixtures. Four rough edges were fixed:

- Filename matching for multiple versions uses a literal prefix. Python's `glob` read `[` and `]`
  in a title as a character class, so `[CR] Movie` could match unrelated files.
- A file literally named `.strm` counts as a stream file when deciding whether a folder is empty.
- Downloads and writes are atomic (write to a temporary file, then rename), so an interrupted run
  cannot leave a truncated playlist or `.strm` behind.
- Playlist downloads have a 300 second timeout; the Python request had none.

Non-UTF-8 playlist bytes are now a hard error instead of being silently decoded as latin-1.
Emby HTTP calls run on a small thread pool, so log lines for concurrent refreshes may interleave.

## License

This project is provided as-is for personal use.
