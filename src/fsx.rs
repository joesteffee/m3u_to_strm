//! Cached filesystem view.
//!
//! The Python original stat'ed and re-read the same directories over and over: one `glob()` per
//! playlist item, `rglob("*")` per directory during empty-dir cleanup, and `rglob("*.strm")` per
//! orphan pass. On a 50k-entry playlist that is millions of syscalls against the same few
//! directories.
//!
//! `Fs` reads each directory once per run and remembers the file contents it has already read, so
//! a run costs one `readdir` per directory touched. Writes update the cache in place, which is what
//! lets the cleanup passes stay O(files) instead of O(files x directories).

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::pytext::py_trim;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
}

type Children = Arc<Vec<Entry>>;

/// A run-scoped directory cache. Not thread safe on purpose: one run, one thread.
#[derive(Default)]
pub struct Fs {
    children: HashMap<PathBuf, Option<Children>>,
    contents: HashMap<PathBuf, Option<String>>,
    reads: AtomicUsize,
}

impl Fs {
    pub fn new() -> Self {
        Fs::default()
    }

    /// Drop every cached listing and file body, forcing the next call to hit the disk.
    pub fn forget_all(&mut self) {
        self.children.clear();
        self.contents.clear();
    }

    /// `Path.read_text(encoding="utf-8", errors="replace")` for the playlist file, which is the one
    /// input that must survive undecodable bytes.
    pub fn read_lossy(&mut self, path: &Path) -> Option<String> {
        let bytes = fs::read(path).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Number of `readdir` syscalls so far. Used by the tests to prove the caching works.
    pub fn directory_reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    fn load(dir: &Path) -> Option<Children> {
        let mut entries: Vec<Entry> = Vec::new();
        for entry in fs::read_dir(dir).ok()? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let file_type = entry.file_type().ok();
            // `Path.is_dir()` follows symlinks, so a symlinked directory counts as a directory here
            // too. One extra stat, only for symlinks.
            let is_dir = match file_type {
                Some(t) if t.is_symlink() => entry.path().is_dir(),
                Some(t) => t.is_dir(),
                None => false,
            };
            entries.push(Entry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir,
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Some(Arc::new(entries))
    }

    fn children(&mut self, dir: &Path) -> Option<Children> {
        if let Some(known) = self.children.get(dir) {
            return known.clone();
        }
        let loaded = Self::load(dir);
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.children.insert(dir.to_path_buf(), loaded.clone());
        loaded
    }

    /// `Path.exists()` — follows symlinks, because the directory listing reports them as present.
    pub fn exists(&mut self, path: &Path) -> bool {
        self.lookup(path).is_some()
    }

    pub fn is_dir(&mut self, path: &Path) -> bool {
        self.lookup(path).is_some_and(|entry| entry.is_dir)
    }

    fn lookup(&mut self, path: &Path) -> Option<Entry> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        let parent = path.parent()?;
        self.children(parent)?
            .iter()
            .find(|entry| entry.name == name)
            .cloned()
    }

    /// Immediate children of `dir`, sorted by name. Empty when `dir` cannot be read.
    pub fn entries(&mut self, dir: &Path) -> Vec<Entry> {
        self.children(dir)
            .map(|kids| kids.to_vec())
            .unwrap_or_default()
    }

    /// `Path.read_text(encoding="utf-8").strip()`. `None` covers every Python read failure:
    /// missing file, a directory, or bytes that are not valid UTF-8.
    pub fn read_stripped(&mut self, path: &Path) -> Option<String> {
        if let Some(cached) = self.contents.get(path) {
            return cached.clone();
        }
        let text = fs::read_to_string(path).ok()?;
        let stripped = py_trim(&text).to_string();
        self.contents
            .insert(path.to_path_buf(), Some(stripped.clone()));
        Some(stripped)
    }

    /// `Path.mkdir(parents=True, exist_ok=True)`
    pub fn create_dir_all(&mut self, dir: &Path) -> io::Result<()> {
        if dir.is_dir() {
            return Ok(());
        }
        fs::create_dir_all(dir)?;
        self.note_created(dir, true);
        Ok(())
    }

    /// Write `contents` to `path` through a temporary file, then rename, so a reader (Emby) never
    /// sees a half-written STRM file. The Python original truncated in place.
    pub fn write_file(&mut self, path: &Path, contents: &str) -> io::Result<()> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            // The scratch image ships no /tmp, so a writer has to be able to bring its own directory.
            fs::create_dir_all(parent)?;
        }
        let mut tmp_os = path.as_os_str().to_os_string();
        tmp_os.push(".m3u-to-strm.part");
        let tmp = PathBuf::from(&tmp_os);
        fs::write(&tmp, contents)?;
        match fs::rename(&tmp, path) {
            Ok(()) => {}
            Err(err) => {
                let _ = fs::remove_file(&tmp);
                return Err(err);
            }
        }
        self.note_created(path, false);
        self.contents
            .insert(path.to_path_buf(), Some(py_trim(contents).to_string()));
        Ok(())
    }

    pub fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)?;
        self.forget_subtree(path);
        Ok(())
    }

    /// `shutil.rmtree(dir)`
    pub fn remove_dir_all(&mut self, dir: &Path) -> io::Result<()> {
        let result = fs::remove_dir_all(dir);
        self.forget_subtree(dir);
        result
    }

    fn forget_subtree(&mut self, path: &Path) {
        self.contents.remove(path);
        self.children.remove(path);
        self.children.retain(|key, _| !key.starts_with(path));
        self.contents.retain(|key, _| !key.starts_with(path));
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            let name = name.to_string_lossy().into_owned();
            if let Some(Some(kids)) = self.children.get(parent).cloned() {
                if kids.iter().any(|kid| kid.name == name) {
                    let kept: Vec<Entry> = kids
                        .iter()
                        .filter(|kid| kid.name != name)
                        .cloned()
                        .collect();
                    self.children
                        .insert(parent.to_path_buf(), Some(Arc::new(kept)));
                }
            }
        }
    }

    /// Add `path` to every cached ancestor listing, so a create does not need a re-read.
    fn note_created(&mut self, path: &Path, is_dir: bool) {
        let mut node = path.to_path_buf();
        let mut node_is_dir = is_dir;
        while let (Some(parent), Some(name)) = (node.parent(), node.file_name()) {
            let name = name.to_string_lossy().into_owned();
            // Only maintain listings we already hold: updating the cache must never cost a
            // readdir. An uncached parent will be read from disk later and sees the new file.
            match self.children.get(parent).cloned() {
                Some(Some(kids)) => {
                    if !kids.iter().any(|kid| kid.name == name) {
                        let mut updated = kids.as_ref().clone();
                        updated.push(Entry {
                            name,
                            is_dir: node_is_dir,
                        });
                        updated.sort_by(|a, b| a.name.cmp(&b.name));
                        self.children
                            .insert(parent.to_path_buf(), Some(Arc::new(updated)));
                    }
                }
                // A listing cached as "missing" is now wrong: the directory exists because we just
                // made it. Drop the negative entry so the next lookup re-reads it, otherwise a
                // freshly written subtree looks empty and cleanup would delete it.
                Some(None) => {
                    self.children.remove(parent);
                }
                None => {}
            }
            node = parent.to_path_buf();
            node_is_dir = true;
        }
    }

    /// True when any `.strm` file exists anywhere below `dir`.
    ///
    /// `Path.suffix` of a file literally named `.strm` is empty, so Python's
    /// `any(f.suffix == ".strm" for f in d.rglob("*"))` did not count it. We do, deliberately: a
    /// file named `.strm` is a STRM file, and deleting a directory that still holds one is worse
    /// than leaving an odd directory behind.
    pub fn has_strm_under(&mut self, dir: &Path) -> bool {
        let Some(kids) = self.children(dir) else {
            return false;
        };
        for kid in kids.iter() {
            if kid.is_dir {
                if self.has_strm_under(&dir.join(&kid.name)) {
                    return true;
                }
            } else if kid.name.ends_with(".strm") {
                return true;
            }
        }
        false
    }

    /// Every `.strm` file below `base`, recursive (`base.rglob("*.strm")`).
    pub fn walk_strm(&mut self, base: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let Some(kids) = self.children(base) else {
            return found;
        };
        for kid in kids.iter() {
            let path = base.join(&kid.name);
            if kid.is_dir {
                found.extend(self.walk_strm(&path));
            } else if kid.name.ends_with(".strm") {
                found.push(path);
            }
        }
        found
    }

    /// `Path.is_symlink()` — a lstat, kept out of the cache because a directory entry cannot tell
    /// a symlink-to-directory from a real directory. Only called for removal candidates.
    pub fn is_symlink(&self, path: &Path) -> bool {
        unwrap_false(fs::symlink_metadata(path).map(|meta| meta.file_type().is_symlink()))
    }
}

fn unwrap_false(result: io::Result<bool>) -> bool {
    result.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writing_a_file_creates_a_missing_parent_directory() {
        // The scratch image has no /tmp; the writers must not assume a parent exists.
        let root = tempdir().unwrap();
        let mut fs = Fs::new();
        let target = root.path().join("deep/nested/file.strm");
        fs.write_file(&target, "url").unwrap();
        assert_eq!(fs.read_stripped(&target).as_deref(), Some("url"));
    }

    #[test]
    fn caches_readdir_and_reads() {
        let dir = std::env::temp_dir().join(format!("m3u-fsx-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.strm"), " http://x/1 \n").unwrap();

        let mut fsx = Fs::new();
        assert!(fsx.exists(&dir.join("a.strm")));
        assert!(!fsx.exists(&dir.join("nope.strm")));
        assert!(fsx.is_dir(&dir.join("sub")));
        assert_eq!(fsx.directory_reads(), 1);

        // Repeat queries are served from the cache.
        assert!(fsx.exists(&dir.join("a.strm")));
        assert!(!fsx.exists(&dir.join("nope.strm")));
        assert_eq!(fsx.directory_reads(), 1);

        // read_stripped trims like Path.read_text().strip() and caches the value.
        assert_eq!(
            fsx.read_stripped(&dir.join("a.strm")).as_deref(),
            Some("http://x/1")
        );
        assert_eq!(
            fsx.read_stripped(&dir.join("a.strm")).as_deref(),
            Some("http://x/1")
        );

        // A directory is not readable text: None, like IsADirectoryError.
        assert_eq!(fsx.read_stripped(&dir.join("sub")), None);

        fsx.write_file(&dir.join("b.strm"), "http://x/2").unwrap();
        assert!(fsx.exists(&dir.join("b.strm")));
        assert_eq!(
            fsx.read_stripped(&dir.join("b.strm")).as_deref(),
            Some("http://x/2")
        );
        assert_eq!(fsx.directory_reads(), 1, "writes must not force a re-read");

        assert_eq!(fsx.walk_strm(&dir).len(), 2);
        assert!(fsx.has_strm_under(&dir));
        assert!(!fsx.has_strm_under(&dir.join("sub")));

        // has_strm_under() above listed sub/ for the first time, so the running total has moved
        // on; what must not move is the count *across* a removal.
        let reads_before_remove = fsx.directory_reads();
        fsx.remove_file(&dir.join("a.strm")).unwrap();
        assert!(!fsx.exists(&dir.join("a.strm")));
        assert_eq!(
            fsx.directory_reads(),
            reads_before_remove,
            "removes must not force a re-read"
        );
        assert_eq!(fsx.walk_strm(&dir).len(), 1);

        fsx.create_dir_all(&dir.join("deep/nested")).unwrap();
        assert!(fsx.is_dir(&dir.join("deep")));
        assert!(fsx.is_dir(&dir.join("deep/nested")));

        fsx.remove_dir_all(&dir.join("deep")).unwrap();
        assert!(!fsx.exists(&dir.join("deep")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn creating_a_directory_repairs_a_cached_missing_parent() {
        let root = std::env::temp_dir().join(format!("m3u-fsx-nested-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let show = root.join("Severance");
        let season = show.join("Season 1");
        let mut fsx = Fs::new();
        // Something looks inside a directory that does not exist yet: that answer is cached.
        assert!(!fsx.is_dir(&season));

        fsx.create_dir_all(&season).unwrap();
        fsx.write_file(&season.join("S01E01.strm"), "http://h/1.mkv")
            .unwrap();

        assert!(
            fsx.is_dir(&show),
            "the new parent must stop looking missing"
        );
        assert!(
            fsx.has_strm_under(&show),
            "a subtree written this run must not look empty to the cleanup passes"
        );
        assert_eq!(fsx.walk_strm(&root), vec![season.join("S01E01.strm")]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unreadable_directory_looks_empty_instead_of_panicking() {
        let mut fsx = Fs::new();
        let missing = Path::new("/definitely/not/here");
        assert_eq!(fsx.entries(missing), Vec::<Entry>::new());
        assert!(!fsx.exists(&missing.join("x")));
        assert_eq!(fsx.walk_strm(missing), Vec::<PathBuf>::new());
    }
}
