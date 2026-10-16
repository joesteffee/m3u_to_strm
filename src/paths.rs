//! Mapping between the container's paths and the paths Emby sees.
//!
//! `EMBY_MOVIES_PATH` and friends say where the same library is mounted for Emby. Nothing else
//! about the two paths is assumed to match, which is why the conversion is a prefix swap.

use std::path::Path;

use crate::config::Config;

impl Config {
    /// `convert_to_emby_path()`: swap the container directory for the Emby-mounted one.
    ///
    /// The prefix test is a plain string comparison, exactly as the Python original had it. That
    /// means a container path of `/usr/src/app/movies_extra/...` also matches the movies mapping —
    /// kept for compatibility, since a real library mounted next to the container root is rare and
    /// silently changing the mapping would be worse than reproducing it.
    pub fn emby_path(&self, path: &Path) -> String {
        let path_text = path.to_string_lossy();
        let mappings = [
            (&self.movies_dir, &self.emby_movies_path),
            (&self.series_dir, &self.emby_series_path),
            (&self.livetv_dir, &self.emby_livetv_path),
        ];

        for (container_dir, emby_dir) in mappings {
            let Some(emby_dir) = emby_dir.as_deref() else {
                continue;
            };
            let container_text = container_dir.to_string_lossy();
            if !path_text.starts_with(container_text.as_ref()) {
                continue;
            }
            let relative = path_text[container_text.len()..].trim_start_matches('/');
            return if relative.is_empty() {
                emby_dir.to_string()
            } else {
                join_posix(emby_dir, relative)
            };
        }

        path_text.into_owned()
    }
}

/// `pathlib.Path(base) / relative` for POSIX strings, including the double-slash rule that keeps
/// `//server/share` intact.
fn join_posix(base: &str, relative: &str) -> String {
    let bytes = base.as_bytes();
    let double_root = bytes.starts_with(b"//") && !bytes.starts_with(b"///");
    let segments: Vec<&str> = base
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();

    if segments.is_empty() {
        return relative.to_string();
    }
    let head = if double_root {
        format!("//{}", segments.join("/"))
    } else if base.starts_with('/') {
        format!("/{}", segments.join("/"))
    } else {
        segments.join("/")
    };
    format!("{head}/{relative}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            emby_movies_path: Some("/mnt/media/movies".into()),
            emby_series_path: Some("/mnt/media/tv".into()),
            ..Config::default()
        }
    }

    #[test]
    fn movie_paths_are_swapped() {
        let cfg = config();
        assert_eq!(
            cfg.emby_path(Path::new(
                "/usr/src/app/movies/Film (2001)/Film (2001).strm"
            )),
            "/mnt/media/movies/Film (2001)/Film (2001).strm"
        );
    }

    #[test]
    fn series_paths_are_swapped() {
        let cfg = config();
        assert_eq!(
            cfg.emby_path(Path::new(
                "/usr/src/app/series/Show (2020)/Season 01/S01E01.strm"
            )),
            "/mnt/media/tv/Show (2020)/Season 01/S01E01.strm"
        );
    }

    #[test]
    fn unmapped_paths_pass_through() {
        let cfg = config();
        assert_eq!(
            cfg.emby_path(Path::new("/usr/src/app/livetv/one.strm")),
            "/usr/src/app/livetv/one.strm"
        );
        assert_eq!(
            cfg.emby_path(Path::new("/somewhere/else.strm")),
            "/somewhere/else.strm"
        );
    }

    #[test]
    fn a_trailing_slash_in_the_mapping_does_not_double_up() {
        let mut cfg = config();
        cfg.emby_movies_path = Some("/mnt/media/".into());
        assert_eq!(
            cfg.emby_path(Path::new("/usr/src/app/movies/f.strm")),
            "/mnt/media/f.strm"
        );
    }

    #[test]
    fn the_directory_itself_maps_to_the_mapping() {
        let cfg = config();
        assert_eq!(
            cfg.emby_path(Path::new("/usr/src/app/movies")),
            "/mnt/media/movies"
        );
    }

    #[test]
    fn relative_mappings_work() {
        let mut cfg = config();
        cfg.emby_movies_path = Some("movies".into());
        assert_eq!(
            cfg.emby_path(Path::new("/usr/src/app/movies/f.strm")),
            "movies/f.strm"
        );
    }

    #[test]
    fn live_mounts_keep_their_double_slash() {
        let mut cfg = config();
        cfg.emby_livetv_path = Some("//server/share/live".into());
        assert_eq!(
            cfg.emby_path(Path::new("/usr/src/app/livetv/a.strm")),
            "//server/share/live/a.strm"
        );
    }

    #[test]
    fn join_posix_matches_pathlib() {
        assert_eq!(join_posix("/mnt/media", "a/b.strm"), "/mnt/media/a/b.strm");
        assert_eq!(join_posix("/mnt/media/", "a"), "/mnt/media/a");
        assert_eq!(join_posix("//srv/share", "a"), "//srv/share/a");
        assert_eq!(join_posix("///srv/share", "a"), "/srv/share/a");
        assert_eq!(join_posix("./x", "a"), "x/a");
        assert_eq!(join_posix(".", "a/b"), "a/b");
        assert_eq!(join_posix("a//b/", "a"), "a/b/a");
    }
}
