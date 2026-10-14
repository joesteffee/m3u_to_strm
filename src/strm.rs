//! Choosing which `.strm` file a playlist entry owns, and writing it.
//!
//! A title can have several sources ("versions") in the same folder. The original disambiguates by
//! the numeric content ID at the end of the stream URL: the first version owns
//! `Title.strm`, later ones get `Title [12345].strm`. That rule is reproduced here exactly,
//! including which file wins when a folder already holds several.

use std::path::{Path, PathBuf};

use crate::fsx::Fs;
use crate::log;
use crate::naming::extract_content_id;

pub struct Outcome {
    pub path: PathBuf,
    pub is_new: bool,
    pub url_changed: bool,
}

fn id_name(filename: &str, content_id: &str) -> String {
    format!("{filename} [{content_id}].strm")
}

fn matches_prefix(name: &str, filename: &str) -> bool {
    name.starts_with(filename) && name.ends_with(".strm")
}

/// Write `url` into the `.strm` file that owns this entry.
///
/// Unlike the original's `glob(f"{filename}*.strm")`, the prefix test is literal, so a title
/// containing `[` or `]` stops being read as a glob character class. That was a real bug: such a
/// title matched unrelated siblings instead of its own files.
pub fn write(
    fs: &mut Fs,
    directory: &Path,
    filename: &str,
    url: &str,
    content_id: Option<&str>,
) -> Result<Outcome, String> {
    let _ = fs.create_dir_all(directory);

    let base_name = format!("{filename}.strm");
    let base = directory.join(&base_name);
    let siblings = fs.entries(directory);
    let matching: Vec<&str> = siblings
        .iter()
        .map(|entry| entry.name.as_str())
        .filter(|name| matches_prefix(name, filename))
        .collect();

    let has_id_files = matching
        .iter()
        .any(|name| *name != base_name.as_str() && name.contains('['));
    let has_base_file = matching.contains(&base_name.as_str());
    let has_multiple_versions = (has_base_file && has_id_files) || matching.len() > 1;

    let id_path = |content_id: &str| directory.join(id_name(filename, content_id));

    let final_path = if has_multiple_versions {
        match content_id {
            Some(content_id) => {
                let id_file = id_path(content_id);
                // Every branch here lands on the ID file; the read only explains why in the log.
                if fs.exists(&base) && !fs.exists(&id_file) {
                    if let Some(existing) = fs.read_stripped(&base) {
                        if extract_content_id(&existing).as_deref() == Some(content_id) {
                            log::debug(format!(
                                "Migrating base file to ID-based filename {} (multiple versions exist)",
                                id_file.file_name().unwrap_or_default().to_string_lossy()
                            ));
                        } else {
                            log::debug(format!(
                                "Multiple versions detected: using ID-based filename {}",
                                id_file.file_name().unwrap_or_default().to_string_lossy()
                            ));
                        }
                    }
                } else if fs.exists(&id_file) {
                    log::debug(format!(
                        "Using existing ID-based file {}",
                        id_file.file_name().unwrap_or_default().to_string_lossy()
                    ));
                } else {
                    log::debug(format!(
                        "Multiple versions detected: using ID-based filename {}",
                        id_file.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }
                id_file
            }
            None => base,
        }
    } else if let Some(content_id) = content_id {
        if has_base_file {
            match fs.read_stripped(&base) {
                Some(existing) => {
                    let existing_id = extract_content_id(&existing);
                    if existing_id.is_some() && existing_id.as_deref() != Some(content_id) {
                        let id_file = id_path(content_id);
                        log::debug(format!(
                            "Duplicate version detected: creating {} (existing: {}, new: {content_id})",
                            id_file.file_name().unwrap_or_default().to_string_lossy(),
                            existing_id.unwrap_or_default()
                        ));
                        id_file
                    } else {
                        base
                    }
                }
                None => base,
            }
        } else {
            let id_file = id_path(content_id);
            if fs.exists(&id_file) {
                id_file
            } else {
                base
            }
        }
    } else {
        base
    };

    let is_new = !fs.exists(&final_path);
    let mut url_changed = false;
    if !is_new {
        url_changed = match fs.read_stripped(&final_path) {
            Some(existing) => existing != url,
            None => true,
        };
    }

    fs.write_file(&final_path, url)
        .map_err(|err| crate::http::io_text(&err))?;

    if is_new {
        log::debug(format!("Created new STRM file: {}", final_path.display()));
    } else if url_changed {
        log::debug(format!(
            "Updated STRM file (URL changed): {}",
            final_path.display()
        ));
    }

    Ok(Outcome {
        path: final_path,
        is_new,
        url_changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Dir(tempfile::TempDir);

    fn dir() -> Dir {
        Dir(tempfile::tempdir().unwrap())
    }

    impl Dir {
        fn path(&self) -> &Path {
            self.0.path()
        }

        fn write(&self, name: &str, contents: &str) {
            fs::write(self.path().join(name), contents).unwrap();
        }

        fn read(&self, name: &str) -> String {
            fs::read_to_string(self.path().join(name)).unwrap()
        }
    }

    #[test]
    fn a_first_version_writes_the_plain_filename() {
        let dir = dir();
        let mut fs = Fs::new();
        let outcome = write(
            &mut fs,
            dir.path(),
            "Blade Runner (1982)",
            "http://h/movie/111.mp4",
            Some("111"),
        )
        .unwrap();
        assert!(outcome.is_new);
        assert!(!outcome.url_changed);
        assert_eq!(outcome.path, dir.path().join("Blade Runner (1982).strm"));
        assert_eq!(
            dir.read("Blade Runner (1982).strm"),
            "http://h/movie/111.mp4"
        );
    }

    #[test]
    fn writing_is_atomic_looking_so_no_temp_file_survives() {
        let dir = dir();
        let mut fs = Fs::new();
        write(&mut fs, dir.path(), "M", "u", None).unwrap();
        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec!["M.strm".to_string()]);
    }

    #[test]
    fn the_same_url_is_not_reported_as_changed() {
        let dir = dir();
        let mut fs = Fs::new();
        dir.write("M.strm", "http://h/movie/111.mp4");
        let outcome = write(
            &mut fs,
            dir.path(),
            "M",
            "http://h/movie/111.mp4",
            Some("111"),
        )
        .unwrap();
        assert!(!outcome.is_new);
        assert!(!outcome.url_changed);
    }

    #[test]
    fn a_different_url_on_the_same_file_is_reported_as_changed() {
        let dir = dir();
        let mut fs = Fs::new();
        dir.write("M.strm", "http://old/111.mp4");
        let outcome = write(&mut fs, dir.path(), "M", "http://new/111.mp4", Some("111")).unwrap();
        assert!(!outcome.is_new);
        assert!(outcome.url_changed);
        assert_eq!(dir.read("M.strm"), "http://new/111.mp4");
    }

    #[test]
    fn a_second_version_with_a_different_id_gets_its_own_file() {
        let dir = dir();
        let mut fs = Fs::new();
        dir.write("M.strm", "http://h/movie/111.mp4");
        let outcome = write(
            &mut fs,
            dir.path(),
            "M",
            "http://h/movie/789012.mp4",
            Some("789012"),
        )
        .unwrap();
        assert_eq!(outcome.path, dir.path().join("M [789012].strm"));
        assert!(outcome.is_new);
        // The first version is left alone.
        assert_eq!(dir.read("M.strm"), "http://h/movie/111.mp4");
        assert_eq!(dir.read("M [789012].strm"), "http://h/movie/789012.mp4");
    }

    #[test]
    fn once_two_versions_exist_every_version_uses_its_id_file() {
        let dir = dir();
        let mut fs = Fs::new();
        dir.write("M.strm", "http://h/movie/111.mp4");
        dir.write("M [789012].strm", "http://h/movie/789012.mp4");
        let outcome = write(
            &mut fs,
            dir.path(),
            "M",
            "http://h/movie/111.mp4",
            Some("111"),
        )
        .unwrap();
        assert_eq!(outcome.path, dir.path().join("M [111].strm"));
    }

    #[test]
    fn an_existing_id_file_wins_when_there_is_no_base_file() {
        let dir = dir();
        let mut fs = Fs::new();
        dir.write("M [789012].strm", "http://h/movie/789012.mp4");
        // Same content id, so the existing ID file is reused rather than the plain base name —
        // and because the URL differs, the write reports a change, like the Python original.
        let outcome = write(
            &mut fs,
            dir.path(),
            "M",
            "http://other/789012.mp4",
            Some("789012"),
        )
        .unwrap();
        assert_eq!(outcome.path, dir.path().join("M [789012].strm"));
        assert!(outcome.url_changed);
        assert!(!outcome.is_new);
        assert_eq!(dir.read("M [789012].strm"), "http://other/789012.mp4");
        // Re-writing the same URL reports no change.
        let again = write(
            &mut fs,
            dir.path(),
            "M",
            "http://other/789012.mp4",
            Some("789012"),
        )
        .unwrap();
        assert_eq!(again.path, dir.path().join("M [789012].strm"));
        assert!(!again.url_changed);
        assert!(!again.is_new);
    }

    #[test]
    fn a_base_file_without_a_content_id_is_untouched_by_id_logic() {
        let dir = dir();
        let mut fs = Fs::new();
        dir.write("M.strm", "http://h/live/1.ts");
        let outcome = write(&mut fs, dir.path(), "M", "http://new/1.ts", None).unwrap();
        assert_eq!(outcome.path, dir.path().join("M.strm"));
        assert!(outcome.url_changed);
    }

    #[test]
    fn a_base_url_that_cannot_be_read_counts_as_changed() {
        let dir = dir();
        let mut fs = Fs::new();
        dir.write("M.strm", "\u{fffd}invalid");
        fs.forget_all();
        let outcome = write(&mut fs, dir.path(), "M", "http://h/movie/111.mp4", None).unwrap();
        assert!(outcome.url_changed);
    }

    #[test]
    fn brackets_in_a_title_match_literally_instead_of_as_a_glob() {
        let dir = dir();
        let mut fs = Fs::new();
        // Unrelated sibling that Python's glob pattern "[0]" would have matched.
        dir.write("0.strm", "unrelated");
        let outcome = write(
            &mut fs,
            dir.path(),
            "Movie [0]",
            "http://h/movie/5.mp4",
            Some("5"),
        )
        .unwrap();
        assert_eq!(outcome.path, dir.path().join("Movie [0].strm"));
        assert!(outcome.is_new);
        assert_eq!(dir.read("0.strm"), "unrelated");
    }

    #[test]
    fn the_directory_is_created_when_missing() {
        let dir = dir();
        let mut fs = Fs::new();
        let target = dir.path().join("Movie (2001)");
        write(&mut fs, &target, "Movie (2001)", "u", None).unwrap();
        assert!(target.join("Movie (2001).strm").exists());
    }

    #[test]
    fn series_episodes_are_named_by_their_episode_code() {
        let dir = dir();
        let mut fs = Fs::new();
        let episode_dir = dir.path().join("The Wire").join("Season 1");
        let outcome = write(
            &mut fs,
            &episode_dir,
            "S01E01",
            "http://h/series/333444.mkv",
            Some("333444"),
        )
        .unwrap();
        assert_eq!(outcome.path, episode_dir.join("S01E01.strm"));
        dir.write("S01E01.strm", "");
        let second = {
            let mut fs = Fs::new();
            write(
                &mut fs,
                &episode_dir,
                "S01E01",
                "http://h/series/999.mkv",
                Some("999"),
            )
            .unwrap()
        };
        assert_eq!(second.path, episode_dir.join("S01E01 [999].strm"));
    }
}
