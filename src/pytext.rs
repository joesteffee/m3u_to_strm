//! Text helpers that reproduce the exact Python semantics the port depends on.
//!
//! The Python original used `str.strip()` and `str.splitlines()`; both differ from
//! Rust's `str::trim()` / `str::lines()` on a few code points, so they are reimplemented
//! here rather than approximated.

/// `str.strip()`: Unicode whitespace **plus** the C0 controls `\\x1c`-`\\x1f`,
/// which Python treats as whitespace and Rust does not.
pub fn py_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// `str.splitlines()` for Python 3 `str`: splits on `\\n`, `\\r`, `\\r\\n`, `\\v`, `\\f`,
/// `\\x1c`, `\\x1d`, `\\x1e`, `\\x85`, `\\u2028`, `\\u2029` and drops a trailing empty line.
///
/// Pure-ASCII input (every real playlist) takes a byte fast path.
pub fn py_split_lines(s: &str) -> Vec<&str> {
    if s.is_ascii() {
        return split_ascii_lines(s);
    }

    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut chars = s.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if !is_line_break(c) {
            continue;
        }
        let mut next = index + c.len_utf8();
        if c == '\r' && matches!(chars.peek(), Some((_, '\n'))) {
            next += 1;
            chars.next();
        }
        lines.push(&s[start..index]);
        start = next;
    }
    if start < s.len() {
        lines.push(&s[start..]);
    }
    lines
}

fn is_line_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r'
            | '\u{b}'
            | '\u{c}'
            | '\u{1c}'
            | '\u{1d}'
            | '\u{1e}'
            | '\u{85}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

fn split_ascii_lines(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut lines = Vec::with_capacity(bytes.len() / 64 + 1);
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if !matches!(b, b'\n' | b'\r' | 0x0b | 0x0c | 0x1c | 0x1d | 0x1e) {
            i += 1;
            continue;
        }
        let mut next = i + 1;
        if b == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            next = i + 2;
        }
        lines.push(&s[start..i]);
        start = next;
        i = next;
    }
    if start < bytes.len() {
        lines.push(&s[start..]);
    }
    lines
}

/// `hay.lower().find(needle) != -1` for an ASCII-lowercase `needle`, without allocating.
pub fn contains_ignore_ascii_case(hay: &str, needle: &str) -> bool {
    debug_assert!(needle.bytes().all(|b| !b.is_ascii_uppercase()));
    let hay = hay.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || hay.len() < needle.len() {
        return needle.is_empty();
    }
    hay.windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

/// `str.removeprefix()`/slice used by the Emby path mapping: strip a leading prefix.
pub fn strip_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.strip_prefix(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_matches_python_boundaries() {
        // Cases verified against CPython 3 `str.splitlines()`.
        assert_eq!(py_split_lines(""), Vec::<&str>::new());
        assert_eq!(py_split_lines("a"), vec!["a"]);
        assert_eq!(py_split_lines("a\n"), vec!["a"]);
        assert_eq!(py_split_lines("a\n\n"), vec!["a", ""]);
        assert_eq!(py_split_lines("\n"), vec![""]);
        assert_eq!(py_split_lines("a\r\nb"), vec!["a", "b"]);
        assert_eq!(py_split_lines("a\rb"), vec!["a", "b"]);
        assert_eq!(py_split_lines("a\r\n"), vec!["a"]);
        assert_eq!(py_split_lines("a\u{b}b\u{c}c"), vec!["a", "b", "c"]);
        assert_eq!(
            py_split_lines("a\u{1c}b\u{1d}c\u{1e}d"),
            vec!["a", "b", "c", "d"]
        );
        assert_eq!(py_split_lines("a\u{85}b"), vec!["a", "b"]);
        assert_eq!(py_split_lines("a\u{2028}b\u{2029}c"), vec!["a", "b", "c"]);
        // Multibyte text whose continuation bytes look like separators must survive intact.
        assert_eq!(py_split_lines("é⁅\u{2045}x"), vec!["é⁅⁅x"]);
        assert_eq!(py_split_lines("é\nx"), vec!["é", "x"]);
    }

    #[test]
    fn trim_matches_python_strip() {
        assert_eq!(py_trim("  hi \n\t"), "hi");
        assert_eq!(py_trim("\u{1c}\u{1d}hi\u{1e}"), "hi");
        assert_eq!(py_trim(" hi "), "hi");
        assert_eq!(py_trim("  "), "");
    }

    #[test]
    fn ascii_insensitive_contains() {
        assert!(contains_ignore_ascii_case("http://X/Movie/1", "/movie/"));
        assert!(!contains_ignore_ascii_case("http://X/movie", "/movie/"));
        assert!(contains_ignore_ascii_case("abc", ""));
    }
}
