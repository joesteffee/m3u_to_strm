//! Configuration from environment variables — the same set the Python script read.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::pytext::py_trim;

pub const DEFAULT_MOVIES_DIR: &str = "/usr/src/app/movies";
pub const DEFAULT_SERIES_DIR: &str = "/usr/src/app/series";
pub const DEFAULT_LIVETV_DIR: &str = "/usr/src/app/livetv";
pub const DEFAULT_TMP_PLAYLIST: &str = "/tmp/playlist.m3u";
pub const DEFAULT_M3U_CACHE_HOURS: i64 = 8;

/// Everything the run needs. Fields map 1:1 onto the Python module-level globals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub m3u_url: Option<String>,
    pub emby_server_url: Option<String>,
    pub emby_api_key: Option<String>,
    pub movies_dir: PathBuf,
    pub series_dir: PathBuf,
    pub livetv_dir: PathBuf,
    pub tmp_playlist: PathBuf,
    pub remove_files: bool,
    pub remove_orphaned: bool,
    pub interval_seconds: i64,
    pub max_items_per_run: i64,
    pub m3u_cache_hours: i64,
    pub emby_movies_path: Option<String>,
    pub emby_series_path: Option<String>,
    pub emby_livetv_path: Option<String>,
    /// Sorted for deterministic log output (Python logged `sorted(set)`).
    pub include_country_codes: BTreeSet<String>,
    pub filter_country_codes: BTreeSet<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            m3u_url: None,
            emby_server_url: None,
            emby_api_key: None,
            movies_dir: PathBuf::from(DEFAULT_MOVIES_DIR),
            series_dir: PathBuf::from(DEFAULT_SERIES_DIR),
            livetv_dir: PathBuf::from(DEFAULT_LIVETV_DIR),
            tmp_playlist: PathBuf::from(DEFAULT_TMP_PLAYLIST),
            remove_files: false,
            remove_orphaned: false,
            interval_seconds: 0,
            max_items_per_run: 0,
            m3u_cache_hours: DEFAULT_M3U_CACHE_HOURS,
            emby_movies_path: None,
            emby_series_path: None,
            emby_livetv_path: None,
            include_country_codes: BTreeSet::new(),
            filter_country_codes: BTreeSet::new(),
        }
    }
}

/// Country-code policy for live TV: include wins over exclude, off when neither is set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CountryRule {
    #[default]
    Off,
    Include(Vec<String>),
    Exclude(Vec<String>),
}

impl CountryRule {
    pub fn label(&self) -> &'static str {
        match self {
            CountryRule::Off => "off",
            CountryRule::Include(_) => "include",
            CountryRule::Exclude(_) => "exclude",
        }
    }

    pub fn codes(&self) -> Vec<&str> {
        match self {
            CountryRule::Off => Vec::new(),
            CountryRule::Include(codes) | CountryRule::Exclude(codes) => {
                codes.iter().map(String::as_str).collect()
            }
        }
    }
}

/// Whether a live channel is hidden by the country-code policy.
///
/// Names without a leading `XX|` are never filtered, whichever mode is active.
pub fn filtered_by_country(tvg_name: &str, rule: &CountryRule) -> bool {
    let Some(code) = crate::naming::extract_country_code(tvg_name) else {
        return false;
    };
    match rule {
        CountryRule::Off => false,
        CountryRule::Include(codes) => !codes.iter().any(|allowed| allowed == code),
        CountryRule::Exclude(codes) => codes.iter().any(|blocked| blocked == code),
    }
}

impl Config {
    /// Resolve the include/exclude sets into the policy the playlist parser uses.
    pub fn country_rule(&self) -> CountryRule {
        if !self.include_country_codes.is_empty() {
            CountryRule::Include(self.include_country_codes.iter().cloned().collect())
        } else if !self.filter_country_codes.is_empty() {
            CountryRule::Exclude(self.filter_country_codes.iter().cloned().collect())
        } else {
            CountryRule::Off
        }
    }

    pub fn from_env() -> Result<Config, String> {
        Self::build(|key| std::env::var(key).ok())
    }

    /// Test seam: build from an arbitrary environment-like lookup.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        Self::build(get)
    }

    fn build(get: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        // `os.environ.get(k)` returns None only for unset keys; an empty value stays empty.
        let text = |key: &str| get(key).filter(|v| !v.is_empty());
        // `os.environ.get(k, "false").lower() == "true"`
        let flag = |key: &str| {
            get(key)
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false)
        };
        // `int(os.environ.get(k, default))`
        let integer = |key: &str, default: i64| -> Result<i64, String> {
            match get(key) {
                None => Ok(default),
                Some(raw) => {
                    let trimmed = py_trim(&raw);
                    trimmed.parse::<i64>().map_err(|_| {
                        format!("invalid integer for environment variable {key:?}: {raw:?}")
                    })
                }
            }
        };
        // `set(code.strip().upper() for code in value.split(",") if code.strip())`
        let codes = |key: &str| -> BTreeSet<String> {
            get(key)
                .map(|value| {
                    value
                        .split(',')
                        .map(py_trim)
                        .filter(|code| !code.is_empty())
                        .map(|code| code.to_uppercase())
                        .collect()
                })
                .unwrap_or_default()
        };

        Ok(Config {
            m3u_url: text("M3U_URL"),
            emby_server_url: text("EMBY_SERVER_URL"),
            emby_api_key: text("EMBY_API_KEY"),
            movies_dir: PathBuf::from(DEFAULT_MOVIES_DIR),
            series_dir: PathBuf::from(DEFAULT_SERIES_DIR),
            livetv_dir: PathBuf::from(DEFAULT_LIVETV_DIR),
            tmp_playlist: PathBuf::from(DEFAULT_TMP_PLAYLIST),
            remove_files: flag("REMOVE_FILES"),
            remove_orphaned: flag("REMOVE_ORPHANED"),
            interval_seconds: integer("INTERVAL_SECONDS", 0)?,
            max_items_per_run: integer("MAX_ITEMS_PER_RUN", 0)?,
            m3u_cache_hours: integer("M3U_CACHE_HOURS", DEFAULT_M3U_CACHE_HOURS)?,
            emby_movies_path: text("EMBY_MOVIES_PATH"),
            emby_series_path: text("EMBY_SERIES_PATH"),
            emby_livetv_path: text("EMBY_LIVETV_PATH"),
            include_country_codes: codes("INCLUDE_COUNTRY_CODES"),
            filter_country_codes: codes("FILTER_COUNTRY_CODES"),
        })
    }

    /// Emby integration is active only when both server URL and API key are set.
    pub fn emby_enabled(&self) -> bool {
        self.emby_server_url
            .as_deref()
            .is_some_and(|u| !u.is_empty())
            && self.emby_api_key.as_deref().is_some_and(|k| !k.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_from(pairs: &[(&str, &str)]) -> Config {
        let map: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| map.get(k).cloned()).unwrap()
    }

    #[test]
    fn defaults_match_python() {
        let cfg = Config::from_lookup(|_| None).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.m3u_cache_hours, 8);
        assert_eq!(cfg.movies_dir, PathBuf::from("/usr/src/app/movies"));
        assert!(!cfg.remove_files);
        assert!(!cfg.emby_enabled());
    }

    #[test]
    fn booleans_only_match_the_literal_word_true() {
        let cfg = config_from(&[("REMOVE_FILES", "TRUE"), ("REMOVE_ORPHANED", "yes")]);
        assert!(cfg.remove_files);
        assert!(!cfg.remove_orphaned);
        let cfg = config_from(&[("REMOVE_FILES", "")]);
        assert!(!cfg.remove_files);
    }

    #[test]
    fn integers_accept_surrounding_whitespace_and_reject_garbage() {
        let cfg = config_from(&[("INTERVAL_SECONDS", " 3600 ")]);
        assert_eq!(cfg.interval_seconds, 3600);
        let err = Config::from_lookup(|k| (k == "MAX_ITEMS_PER_RUN").then(|| "many".to_string()));
        assert!(err.is_err());
        // An empty value is an error in Python too (`int("")` raises).
        let err = Config::from_lookup(|k| (k == "M3U_CACHE_HOURS").then(String::new));
        assert!(err.is_err());
    }

    #[test]
    fn country_codes_are_trimmed_uppercased_and_deduped() {
        let cfg = config_from(&[
            ("INCLUDE_COUNTRY_CODES", " us , ,GB\t,us"),
            ("FILTER_COUNTRY_CODES", ","),
        ]);
        assert_eq!(cfg.include_country_codes.len(), 2);
        assert!(cfg.include_country_codes.contains("US"));
        assert!(cfg.include_country_codes.contains("GB"));
        assert!(cfg.filter_country_codes.is_empty());
    }

    #[test]
    fn emby_requires_both_url_and_key() {
        assert!(
            config_from(&[("EMBY_SERVER_URL", "http://x:8096"), ("EMBY_API_KEY", "k")])
                .emby_enabled()
        );
        assert!(!config_from(&[("EMBY_SERVER_URL", "http://x:8096")]).emby_enabled());
        assert!(!config_from(&[("EMBY_API_KEY", "")]).emby_enabled());
    }
}
