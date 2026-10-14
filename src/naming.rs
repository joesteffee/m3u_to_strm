//! Title/channel-name parsing, ported pattern-for-pattern from `parse_m3u.py`.
//!
//! Every regex here is a verbatim copy of the Python one (Python's `re.match` is
//! reproduced by the leading `^`, and the regex crate uses the same leftmost-first,
//! lazy-quantifier semantics as CPython). Do not "simplify" these patterns: their
//! exact behaviour is pinned by `tests/python_parity.rs`.

#![allow(non_snake_case)] // regex accessors keep their Python constant names

use std::sync::OnceLock;

use regex::Regex;

use crate::pytext::py_trim;

macro_rules! static_re {
    ($name:ident, $pattern:expr) => {
        fn $name() -> &'static Regex {
            static RE: OnceLock<Regex> = OnceLock::new();
            RE.get_or_init(|| Regex::new($pattern).expect("invalid pattern"))
        }
    };
}

// "01-", "02 ", "001-", "06 EN - "
static_re!(
    NUMERIC_PREFIX,
    r"^\d+\s*(?:[-.]?\s*)?(?:[A-Z0-9+]{1,4}\s*-\s*)?"
);
// "EN - ", "NF - ", "D+ - "
static_re!(LANG_PREFIX, r"^[A-Z0-9+]{1,4}\s*-\s*");
static_re!(YEAR_IN_PARENS, r"\((\d{4})\)");
static_re!(ONLY_YEAR_IN_PARENS, r"^\s*\(\d{4}\)\s*$");
static_re!(ALL_CAPS_WORDS, r"^[A-Z\s,]+$");
// "Title - SERVICE - Title"
static_re!(
    MOVIE_DUPLICATE,
    r"(?i)^(.+?)\s*-\s*[A-Z0-9+]{1,4}\s*-\s*(.+?)(?:\s*\([^)]*\))?\s*$"
);
static_re!(STRIP_PARENS, r"\s*\([^()]*\)");
static_re!(HAS_LOWERCASE, r"[a-z]");
static_re!(TRAILING_ALL_CAPS, r"\s+[A-Z]{2,}(?:\s+[A-Z]{2,})*,?\s*$");
static_re!(
    SERIES_SIMPLE_DUPLICATE,
    r"(?i)^(.+?)\s*-\s*[A-Z0-9+]{1,4}\s*-\s*(.+?)(?:\s*\([^)]*\))?(?:\s*-\s*S\d{1,2}\s*E\d{1,2})?\s*$"
);
static_re!(
    SERIES_DUPLICATE_1,
    r"^(.+?)\s*\([^)]*\)\s*-\s*[A-Z0-9+]{1,4}\s*-\s*(.+?)\s*\((\d{4})\)"
);
static_re!(
    SERIES_DUPLICATE_2,
    r"^(.+?)\s*\((\d{4})\)\s*-\s*[A-Z0-9+]{1,4}\s*-\s*(.+?)(?:\s*-\s*S\d{1,2}\s*E\d{1,2}|$)"
);
static_re!(
    SERIES_DUPLICATE_3,
    r"^(.+?)\s*-\s*[A-Z0-9+]{1,4}\s*-\s*(.+?)(?:\s*-\s*S\d{1,2}\s*E\d{1,2}|$)"
);
static_re!(SEASON_EPISODE_ONLY, r"(?i)^S\d{1,2}\s*E\d{1,2}$");
static_re!(SEASON_EPISODE, r"(?i)S(\d{1,2})\s*E(\d{1,2})");
static_re!(CONTENT_ID, r"(?i)/(\d+)\.(mp4|mkv|avi|mov|m4v)$");
static_re!(INVALID_FILENAME_CHARS, r#"[\\/:"*?<>|#]+"#);

/// Group `n` of a match as `&str` (groups in these patterns are always present when it matches).
fn group<'t>(caps: &regex::Captures<'t>, n: usize) -> &'t str {
    caps.get(n).map(|m| m.as_str()).unwrap_or("")
}

/// Remove characters that are illegal in file names.
pub fn safe_filename(name: &str) -> String {
    py_trim(&INVALID_FILENAME_CHARS().replace_all(name, "")).to_string()
}

/// Split on top-level commas (commas outside parentheses), mirroring the character loop
/// the Python original uses. Parts are trimmed; empty parts produced by consecutive
/// commas are kept, exactly like the original.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0usize;
    for (index, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(py_trim(&s[start..index]));
                start = index + c.len_utf8();
            }
            _ => {}
        }
    }
    let tail = &s[start..];
    if !tail.is_empty() {
        parts.push(py_trim(tail));
    }
    parts
}

/// Keep only the leading title when a name repeats itself as "French Title, English Title".
fn drop_duplicate_comma_title(name: &str) -> String {
    if !name.contains(',') {
        return name.to_string();
    }
    let parts = split_top_level_commas(name);
    if parts.len() <= 1 {
        return name.to_string();
    }
    let second = py_trim(parts[1]);
    if ONLY_YEAR_IN_PARENS().is_match(second) {
        return name.to_string();
    }
    let second_len = second.chars().count();
    if (second_len >= 5 && !ALL_CAPS_WORDS().is_match(second)) || second_len >= 10 {
        return parts[0].to_string();
    }
    name.to_string()
}

/// Build the movie folder/file name from a playlist `tvg-name`.
pub fn parse_movie_name(tvg_name: &str) -> String {
    let mut name = py_trim(&NUMERIC_PREFIX().replace_all(tvg_name, "")).to_string();
    name = py_trim(&LANG_PREFIX().replace_all(&name, "")).to_string();

    // Grab the year before the comma/duplicate clean-up can destroy it.
    let mut year = YEAR_IN_PARENS()
        .find(&name)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();

    name = drop_duplicate_comma_title(&name);

    if let Some(caps) = MOVIE_DUPLICATE().captures(&name) {
        let first = py_trim(group(&caps, 1));
        let second = py_trim(group(&caps, 2));
        if first.to_lowercase() == second.to_lowercase() {
            name = first.to_string();
        }
    }

    // Re-find the year: its position (and existence) may have changed.
    let year_match = YEAR_IN_PARENS().find(&name);
    let truncated = match &year_match {
        Some(m) => {
            year = m.as_str().to_string();
            py_trim(&name[..m.end()]).to_string()
        }
        None => name.clone(),
    };
    name = py_trim(&STRIP_PARENS().replace_all(&truncated, "")).to_string();

    // Trailing ALL-CAPS actor names only make sense after a mixed-case title.
    if HAS_LOWERCASE().is_match(&name) {
        name = py_trim(&TRAILING_ALL_CAPS().replace_all(&name, "")).to_string();
    }

    if !year.is_empty() {
        name.push(' ');
        name.push_str(&year);
    }
    safe_filename(&name)
}

/// Build the series folder name from a playlist `tvg-name`.
pub fn parse_series_name(tvg_name: &str) -> String {
    let mut name = py_trim(&NUMERIC_PREFIX().replace_all(tvg_name, "")).to_string();
    name = py_trim(&LANG_PREFIX().replace_all(&name, "")).to_string();
    name = drop_duplicate_comma_title(&name);

    if let Some(caps) = SERIES_SIMPLE_DUPLICATE().captures(&name) {
        let first = py_trim(group(&caps, 1));
        let second = py_trim(group(&caps, 2));
        if first.to_lowercase() == second.to_lowercase() {
            name = first.to_string();
            name = py_trim(&strip_season_episode_tail(&name)).to_string();
            if let Some(m) = YEAR_IN_PARENS().find(&name) {
                let year = m.as_str().to_string();
                name = py_trim(&STRIP_PARENS().replace_all(&name, "")).to_string();
                name.push(' ');
                name.push_str(&year);
            }
            return safe_filename(&name);
        }
    }

    // "Show (year-range) - LANG - Show (year) - S01E01"
    if let Some(caps) = SERIES_DUPLICATE_1().captures(&name) {
        let show = py_trim(group(&caps, 2));
        let year = group(&caps, 3);
        name = format!("{show} ({year})");
        name = py_trim(&strip_season_episode_tail(&name)).to_string();
        return safe_filename(&name);
    }

    // "Show (year) - LANG - Show - S01E01"
    if let Some(caps) = SERIES_DUPLICATE_2().captures(&name) {
        let show = py_trim(group(&caps, 3));
        if !show.is_empty() && !SEASON_EPISODE_ONLY().is_match(show) {
            return safe_filename(&format!("{show} ({})", group(&caps, 2)));
        }
    }

    // "Show - LANG - Show - S01E01" (only when no year is present anywhere)
    if !YEAR_IN_PARENS().is_match(&name) {
        if let Some(caps) = SERIES_DUPLICATE_3().captures(&name) {
            let mut show = py_trim(group(&caps, 2)).to_string();
            if !show.is_empty() && !SEASON_EPISODE_ONLY().is_match(&show) {
                show = py_trim(&strip_season_episode_tail(&show)).to_string();
                return safe_filename(&show);
            }
        }
    }

    let year = YEAR_IN_PARENS()
        .find(&name)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    name = py_trim(&strip_season_episode_tail(&name)).to_string();
    name = py_trim(&STRIP_PARENS().replace_all(&name, "")).to_string();
    if !year.is_empty() {
        name.push(' ');
        name.push_str(&year);
    }
    safe_filename(&name)
}

// `re.sub(r'[\s\-\.]+S\d{1,2}\s*E\d{1,2}.*$', '', name, flags=re.IGNORECASE)`
static_re!(SEASON_EPISODE_TAIL, r"(?i)[\s\-\.]+S\d{1,2}\s*E\d{1,2}.*$");
pub fn strip_season_episode_tail(name: &str) -> String {
    SEASON_EPISODE_TAIL().replace_all(name, "").into_owned()
}

/// `(Season folder, episode file prefix)`; defaults to `Season 1` / `S01E01`.
pub fn extract_season_episode(tvg_name: &str) -> (String, String) {
    match SEASON_EPISODE().captures(tvg_name) {
        Some(caps) => {
            let season: u32 = group(&caps, 1).parse().unwrap_or(0);
            let episode: u32 = group(&caps, 2).parse().unwrap_or(0);
            (
                format!("Season {season}"),
                format!("S{season:02}E{episode:02}"),
            )
        }
        None => ("Season 1".to_string(), "S01E01".to_string()),
    }
}

/// Numeric content id taken from the stream URL, e.g. `.../822036.mp4` -> `822036`.
pub fn extract_content_id(url: &str) -> Option<String> {
    CONTENT_ID()
        .captures(url)
        .map(|caps| group(&caps, 1).to_string())
}

/// Leading `XX|` country code of a channel name.
pub fn extract_country_code(tvg_name: &str) -> Option<&str> {
    let bytes = tvg_name.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_uppercase()
        && bytes[1].is_ascii_uppercase()
        && bytes[2] == b'|'
    {
        Some(&tvg_name[..2])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_filename_strips_invalid_characters() {
        assert_eq!(safe_filename("  a/b:c*d?e<f>g#h|i  "), "abcdefghi");
        assert_eq!(safe_filename("Joker (2019)"), "Joker (2019)");
    }

    #[test]
    fn movie_names_drop_prefixes_and_duplicates() {
        assert_eq!(
            parse_movie_name("01 - EN - The Matrix (1999)"),
            "The Matrix (1999)"
        );
        assert_eq!(
            parse_movie_name("Disney Insider - D+ - Disney Insider (2022)"),
            "Disney Insider (2022)"
        );
        assert_eq!(
            parse_movie_name("The Pledge JACK NICHOLSON (2001)"),
            "The Pledge (2001)"
        );
        // Second comma part is a bare year: keep the whole title.
        assert_eq!(parse_movie_name("Being (1998), 1998"), "Being (1998)");
        // "TV" is not long enough to count as an alternate title, so the pair survives the
        // comma rule; the trailing ALL-CAPS rule then eats " TV" and leaves the comma behind.
        // That is what the Python original returns, so the parity is kept on purpose.
        assert_eq!(parse_movie_name("Film, TV"), "Film,");
    }

    #[test]
    fn series_names_keep_show_year_and_drop_episode_data() {
        assert_eq!(
            parse_series_name(
                "06 EN - Friends (1994) - S01E02 - The One With The Sonogram At The End"
            ),
            "Friends (1994)"
        );
        assert_eq!(
            parse_series_name("Mighty Express - NF - Mighty Express - S01E03"),
            "Mighty Express"
        );
        assert_eq!(
            parse_series_name("Show (2020) - EN - Other Name - S02E05"),
            "Other Name (2020)"
        );
    }

    #[test]
    fn season_episode_extraction() {
        assert_eq!(
            extract_season_episode("Show S01 E02 Title"),
            ("Season 1".into(), "S01E02".into())
        );
        assert_eq!(
            extract_season_episode("show s9e10"),
            ("Season 9".into(), "S09E10".into())
        );
        assert_eq!(
            extract_season_episode("no episode here"),
            ("Season 1".into(), "S01E01".into())
        );
    }

    #[test]
    fn content_id_and_country_code() {
        assert_eq!(
            extract_content_id("http://h/a/b/822036.mp4").as_deref(),
            Some("822036")
        );
        assert_eq!(
            extract_content_id("http://h/a/b/822036.MKV").as_deref(),
            Some("822036")
        );
        assert_eq!(extract_content_id("http://h/a/b/822036.mp4?x=1"), None);
        assert_eq!(extract_content_id("http://h/a/b/movie.mp4"), None);
        assert_eq!(extract_country_code("AR| BEIN SPORT"), Some("AR"));
        assert_eq!(extract_country_code("A| BEIN"), None);
        assert_eq!(extract_country_code("Regular Channel"), None);
        assert_eq!(extract_country_code("AR"), None);
    }
}
