//! Playlist parsing: turn `#EXTINF` lines into movies, series episodes, and live TV channels.
//!
//! Classification is by URL path, which is what the original did after group-title guessing proved
//! unreliable: a line whose URL contains `/movie/` is a movie, `/series/` is an episode, anything
//! else is a live channel — whatever its `group-title` says.

use crate::config::CountryRule;
use crate::log;
use crate::naming::{extract_country_code, parse_movie_name, parse_series_name};
use crate::pytext::{self, py_trim};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub tvg_name: String,
    pub url: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Playlist {
    pub movies: Vec<Item>,
    pub series: Vec<Item>,
    /// Live TV lines are kept verbatim (`#EXTINF...` + URL) and written back out as one playlist.
    pub live_tv: Vec<String>,
    pub extinf_count: usize,
    pub no_tvg_name_count: usize,
    pub filtered_live_count: usize,
}

impl Playlist {
    pub fn is_empty(&self) -> bool {
        self.movies.is_empty() && self.series.is_empty() && self.live_tv.is_empty()
    }
}

static TVG_NAME: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static GROUP_TITLE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

fn tvg_name_pattern() -> &'static regex::Regex {
    TVG_NAME.get_or_init(|| regex::Regex::new(r#"tvg-name="([^"]+)""#).expect("valid pattern"))
}

fn group_title_pattern() -> &'static regex::Regex {
    GROUP_TITLE
        .get_or_init(|| regex::Regex::new(r#"group-title="([^"]+)""#).expect("valid pattern"))
}

pub fn parse(playlist_text: &str, rule: &CountryRule) -> Playlist {
    let lines = pytext::py_split_lines(playlist_text);
    let mut out = Playlist::default();
    log::debug(format!("Playlist has {} total lines", lines.len()));

    let mut index = 0;
    while index < lines.len() {
        let info = lines[index];
        if !info.starts_with("#EXTINF") {
            index += 1;
            continue;
        }
        index += 1;
        let url = lines.get(index).copied().unwrap_or("");
        index += 1;
        out.extinf_count += 1;

        let Some(captures) = tvg_name_pattern().captures(info) else {
            out.no_tvg_name_count += 1;
            log::debug(format!(
                "Skipping item without tvg-name: {}...",
                truncate(info, 100)
            ));
            continue;
        };
        let tvg_name = py_trim(&captures[1]).to_string();
        if tvg_name.is_empty() {
            out.no_tvg_name_count += 1;
            log::debug(format!(
                "Skipping item without tvg-name: {}...",
                truncate(info, 100)
            ));
            continue;
        }

        let group_title = group_title_pattern()
            .captures(info)
            .map(|captures| py_trim(&captures[1]).to_lowercase())
            .unwrap_or_default();

        if pytext::contains_ignore_ascii_case(url, "/movie/") {
            log::debug(format!("Found movie: {tvg_name} (group: {group_title})"));
            out.movies.push(Item {
                tvg_name,
                url: url.to_string(),
            });
        } else if pytext::contains_ignore_ascii_case(url, "/series/") {
            log::debug(format!("Found series: {tvg_name} (group: {group_title})"));
            out.series.push(Item {
                tvg_name,
                url: url.to_string(),
            });
        } else if crate::config::filtered_by_country(&tvg_name, rule) {
            out.filtered_live_count += 1;
            log::debug(format!(
                "Filtered live TV channel: {tvg_name} (country code: {})",
                extract_country_code(&tvg_name).unwrap_or("None")
            ));
        } else {
            out.live_tv.push(format!("{info}\n{url}"));
        }
    }

    out
}

/// First `max` characters, the way `info[:100]` sliced characters (not bytes).
fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The log lines the original emitted right after classifying.
pub fn log_summary(playlist: &Playlist, rule: &CountryRule) {
    log::info(format!("Found {} #EXTINF entries", playlist.extinf_count));
    if playlist.no_tvg_name_count > 0 {
        log::warning(format!(
            "Skipped {} entries without tvg-name",
            playlist.no_tvg_name_count
        ));
    }
    match rule {
        CountryRule::Include(codes) => log::info(format!(
            "Country code filtering: INCLUDE mode active (only: {})",
            codes.join(", ")
        )),
        CountryRule::Exclude(codes) => log::info(format!(
            "Country code filtering: EXCLUDE mode active (excluding: {})",
            codes.join(", ")
        )),
        CountryRule::Off => {}
    }
    if playlist.filtered_live_count > 0 {
        log::info(format!(
            "Filtered out {} live TV channel(s) based on country code rules",
            playlist.filtered_live_count
        ));
    }
    log::info(format!(
        "Classified: {} movies, {} series, {} live TV",
        playlist.movies.len(),
        playlist.series.len(),
        playlist.live_tv.len()
    ));
}

/// Movie folder names are computed once per item, so the whole playlist can be parsed in parallel.
pub fn movie_target(tvg_name: &str) -> String {
    parse_movie_name(tvg_name)
}

/// Series folder name plus the `Season NN` subdirectory.
pub fn series_target(tvg_name: &str) -> (String, String) {
    let (season, _episode) = crate::naming::extract_season_episode(tvg_name);
    (parse_series_name(tvg_name), season)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, _url: &str, group: Option<&str>) -> String {
        match group {
            Some(group) => format!(r#"#EXTINF:-1 tvg-name="{name}" group-title="{group}",{name}"#),
            None => format!(r#"#EXTINF:-1 tvg-name="{name}",{name}"#),
        }
    }

    #[test]
    fn classification_uses_the_url_not_the_group_title() {
        let text = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            entry("Movie One", "http://h/movie/1.mp4", Some("Movies")),
            "http://host/movie/1.mp4",
            entry("Show One", "http://h/series/1.mkv", Some("Series")),
            "http://host/series/1.mkv",
            entry("US|News", "http://h/live/1.ts", Some("News")),
            "http://host/live/1.ts"
        );
        let playlist = parse(&text, &CountryRule::Off);
        assert_eq!(playlist.movies.len(), 1);
        assert_eq!(playlist.series.len(), 1);
        assert_eq!(playlist.live_tv.len(), 1);
        assert_eq!(playlist.extinf_count, 3);
    }

    #[test]
    fn a_movie_group_with_a_live_url_is_still_live_tv() {
        let text = format!(
            "{}\n{}",
            entry("Channel", "http://host/live/1.ts", Some("Movies")),
            "http://host/live/1.ts"
        );
        let playlist = parse(&text, &CountryRule::Off);
        assert!(playlist.movies.is_empty());
        assert_eq!(playlist.live_tv.len(), 1);
    }

    #[test]
    fn url_matching_ignores_case() {
        let text = format!(
            "{}\n{}\n{}\n{}",
            entry("A", "http://h/MOVIE/1.mp4", None),
            "http://h/MOVIE/1.mp4",
            entry("B", "http://h/Series/1.mkv", None),
            "http://h/Series/1.mkv"
        );
        let playlist = parse(&text, &CountryRule::Off);
        assert_eq!(playlist.movies.len(), 1);
        assert_eq!(playlist.series.len(), 1);
    }

    #[test]
    fn entries_without_a_name_are_counted_and_skipped() {
        let text = format!(
            "#EXTINF:-1 tvg-id=\"x\",No Name\n{}\n",
            "http://host/movie/1.mp4"
        );
        let playlist = parse(&text, &CountryRule::Off);
        assert_eq!(playlist.no_tvg_name_count, 1);
        assert_eq!(playlist.extinf_count, 1);
        assert!(playlist.movies.is_empty());
    }

    #[test]
    fn an_empty_name_is_skipped() {
        let text = format!(
            "{}\n{}",
            r##"#EXTINF:-1 tvg-name="",Nothing"##, "http://host/movie/1.mp4"
        );
        let playlist = parse(&text, &CountryRule::Off);
        assert_eq!(playlist.no_tvg_name_count, 1);
    }

    #[test]
    fn country_codes_filter_only_live_tv() {
        let rule = CountryRule::Exclude(vec!["US".to_string()]);
        let text = [
            (
                entry("US|News", "http://h/live/1.ts", None),
                "http://h/live/1.ts",
            ),
            (
                entry("DE|News", "http://h/live/2.ts", None),
                "http://h/live/2.ts",
            ),
            (
                entry("US|Movie", "http://h/movie/1.mp4", None),
                "http://h/movie/1.mp4",
            ),
        ]
        .map(|(info, url)| format!("{info}\n{url}"))
        .join("\n");

        let playlist = parse(&text, &rule);
        assert_eq!(playlist.live_tv.len(), 1);
        assert!(playlist.live_tv[0].contains("DE|News"));
        assert_eq!(playlist.filtered_live_count, 1);
        assert_eq!(playlist.movies.len(), 1);
    }

    #[test]
    fn live_entries_keep_the_raw_info_line_and_url() {
        let info = entry("Channel", "http://h/live/1.ts", None);
        let playlist = parse(&format!("{info}\nhttp://h/live/1.ts"), &CountryRule::Off);
        assert_eq!(
            playlist.live_tv,
            vec![format!("{info}\nhttp://h/live/1.ts")]
        );
    }

    #[test]
    fn a_trailing_entry_without_a_url_is_still_counted() {
        let info = entry("Channel", "http://h/live/1.ts", None);
        let playlist = parse(&info, &CountryRule::Off);
        assert_eq!(playlist.extinf_count, 1);
        assert_eq!(playlist.live_tv, vec![format!("{info}\n")]);
    }

    #[test]
    fn non_extinf_lines_are_ignored() {
        let text = format!(
            "#EXTM3U\n#EXT-X-VERSION:3\n{}\n{}",
            entry("A", "http://h/movie/1", None),
            "http://h/movie/1"
        );
        let playlist = parse(&text, &CountryRule::Off);
        assert_eq!(playlist.extinf_count, 1);
    }
}
