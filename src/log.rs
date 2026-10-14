//! Minimal line logger matching the Python original's format:
//! `YYYY-MM-DD HH:MM:SS - LEVEL - message` on stdout.
//!
//! `LOG_LEVEL=DEBUG` is an optional extra knob (the Python build always ran at INFO);
//! everything else is unchanged.

use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, MutexGuard};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum Level {
    Debug = 10,
    Info = 20,
    Warning = 30,
    Error = 40,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warning => "WARNING",
            Level::Error => "ERROR",
        }
    }
}

static MIN_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

/// When `Some`, lines are collected here instead of stdout (tests only).
static CAPTURE: Mutex<Option<Vec<String>>> = Mutex::new(None);

pub fn init_from_env() {
    let level = match std::env::var("LOG_LEVEL").unwrap_or_default().as_str() {
        "DEBUG" => Level::Debug,
        "WARNING" | "WARN" => Level::Warning,
        "ERROR" => Level::Error,
        _ => Level::Info,
    };
    MIN_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn min_level() -> Level {
    match MIN_LEVEL.load(Ordering::Relaxed) {
        10 => Level::Debug,
        30 => Level::Warning,
        40 => Level::Error,
        _ => Level::Info,
    }
}

/// Cheap guard for debug lines whose message needs allocation to build.
#[inline]
pub fn enabled(level: Level) -> bool {
    level >= min_level()
}

fn emit(level: Level, msg: &str) {
    if level < min_level() {
        return;
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("{stamp} - {} - {msg}", level.label());
    // One lock for both sinks so concurrent workers never interleave half-lines.
    let mut guard: MutexGuard<Option<Vec<String>>> = match CAPTURE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    match guard.as_mut() {
        Some(buf) => buf.push(line),
        None => {
            println!("{line}");
            let _ = std::io::stdout().flush();
        }
    }
}

pub fn debug(msg: impl AsRef<str>) {
    emit(Level::Debug, msg.as_ref());
}
pub fn info(msg: impl AsRef<str>) {
    emit(Level::Info, msg.as_ref());
}
pub fn warning(msg: impl AsRef<str>) {
    emit(Level::Warning, msg.as_ref());
}
pub fn error(msg: impl AsRef<str>) {
    emit(Level::Error, msg.as_ref());
}

fn lock_capture() -> std::sync::MutexGuard<'static, Option<Vec<String>>> {
    match CAPTURE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Sets the minimum level explicitly and returns the previous one.
pub fn set_level(level: Level) -> Level {
    let previous = min_level();
    MIN_LEVEL.store(level as u8, Ordering::Relaxed);
    previous
}

/// Redirects log lines into an in-memory buffer for as long as it is alive.
///
/// Only one capture can be live at a time, so tests that use it are serialised. A capture also
/// raises the level to DEBUG, so assertions can see every line the program is able to emit.
pub struct Capture {
    _serial: std::sync::MutexGuard<'static, ()>,
    previous_level: Level,
}

impl Capture {
    pub fn start() -> Capture {
        let serial = capture_lock();
        let previous_level = set_level(Level::Debug);
        *lock_capture() = Some(Vec::new());
        Capture {
            _serial: serial,
            previous_level,
        }
    }

    /// Forget everything captured so far, keeping the capture (and its serialisation lock) alive.
    ///
    /// Lets a test assert about one pass without the lines of the pass before it.
    pub fn clear(&mut self) {
        *lock_capture() = Some(Vec::new());
    }

    pub fn lines(&self) -> Vec<String> {
        lock_capture().as_ref().cloned().unwrap_or_default()
    }

    /// The lines of one level, e.g. only the warnings.
    pub fn lines_at(&self, level: Level) -> Vec<String> {
        let tag = format!(" - {} - ", level.label());
        self.lines()
            .into_iter()
            .filter(|line| line.contains(&tag))
            .collect()
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        *lock_capture() = None;
        set_level(self.previous_level);
    }
}

fn capture_lock() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    match SERIAL.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Whether blocking HTTP calls may run on helper threads.
///
/// Set `M3U_PARALLEL_HTTP=0` to force strictly sequential Emby calls (slower, but the log order
/// matches the Python original exactly). Unit tests use it for deterministic assertions.
pub fn parallel_http_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("M3U_PARALLEL_HTTP")
            .map(|v| v != "0")
            .unwrap_or(true)
    })
}
