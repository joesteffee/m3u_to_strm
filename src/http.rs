//! HTTP plumbing shared by the playlist download and the Emby API calls.
//!
//! The Python original used `requests` with `timeout=10`. `ureq` is the blocking equivalent: no
//! async runtime, no thread pool, a few hundred kilobytes of code. TLS trust comes from the Mozilla
//! root store compiled into the binary, which is what makes a `scratch` container work without any
//! CA package installed.

use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

pub const TIMEOUT_SECONDS: u64 = 10;

/// Timeout for the playlist download itself; see [`fetch`].
pub const PLAYLIST_TIMEOUT_SECONDS: u64 = 300;

pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(TIMEOUT_SECONDS))
        .build()
}

/// Percent-encode parameters the way `requests` does (`application/x-www-form-urlencoded`):
/// unreserved characters pass through, a space becomes `+`, everything else becomes `%XX`.
pub fn query_string(params: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (key, value) in params {
        if !out.is_empty() {
            out.push('&');
        }
        encode_into(key, &mut out);
        out.push('=');
        encode_into(value, &mut out);
    }
    out
}

fn encode_into(value: &str, out: &mut String) {
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
}

/// GET a document and hand back its body as text.
///
/// Deviation from Python: `requests` decoded the body with the charset from `Content-Type` and
/// fell back to ISO-8859-1, which silently produced mojibake for mislabelled UTF-8. Here a body
/// that is not valid UTF-8 is an error, because writing garbage channel names into a library is
/// worse than failing the run loudly.
pub fn get_text(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    let response = agent.get(url).call().map_err(|err| error_text(&err))?;
    let status = response.status();
    if status != 200 {
        return Err(format!("{status} {} for url: {url}", reason(status)));
    }
    response
        .into_string()
        .map_err(|err| format!("response body is not valid UTF-8: {err}"))
}

/// Status code of a failed call, when the server did answer.
pub fn status_of(err: &ureq::Error) -> Option<u16> {
    match err {
        ureq::Error::Status(code, _) => Some(*code),
        _ => None,
    }
}

/// Reason phrase for the codes this service actually sees; everything else prints bare.
pub fn reason(status: u16) -> &'static str {
    match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

/// Message text for a failed request, shaped like the `requests` exception message.
pub fn error_text(err: &ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, _) => {
            format!("HTTP status code {code} when fetching {}", err_url(err))
        }
        other => other.to_string(),
    }
}

fn err_url(err: &ureq::Error) -> String {
    match err {
        ureq::Error::Status(_, response) => response.get_url().to_string(),
        _ => "<unknown url>".to_string(),
    }
}

/// Turn an `io::Error` into the message Python would have printed for the same failure.
pub fn io_text(err: &io::Error) -> String {
    match err.raw_os_error() {
        Some(errno) => format!("{} (os error {errno})", err.kind()),
        None => err.to_string(),
    }
}

/// Long-timeout fetch for the playlist download.
///
/// The Python original called `requests.get(url)` with no timeout at all, which can hang forever
/// on a half-dead CDN. 300 s is generous for a multi-megabyte playlist and still recovers.
pub fn fetch(url: &str) -> Result<String, String> {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    let agent = AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(PLAYLIST_TIMEOUT_SECONDS))
            .build()
    });
    get_text(agent, url)
}

/// Write `body` to `path` via a temporary file + rename, so a crash mid-download cannot leave a
/// truncated playlist that the next run would treat as valid.
pub fn write_atomic(path: &Path, body: &str) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        // The scratch image ships no /tmp, so the cache directory has to come with the file.
        fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {}", parent.display(), io_text(&e)))?;
    }
    let temp = path.with_file_name(format!(
        "{}.m3u-to-strm.part",
        path.file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| "download".into())
    ));
    fs::write(&temp, body).map_err(|e| format!("writing {}: {}", temp.display(), io_text(&e)))?;
    fs::rename(&temp, path).map_err(|e| {
        format!(
            "renaming {} to {}: {}",
            temp.display(),
            path.display(),
            io_text(&e)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_atomic_creates_a_missing_parent_directory() {
        let root = tempdir().unwrap();
        let target = root.path().join("nested/tmp/playlist.m3u");
        write_atomic(&target, "#EXTM3U").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "#EXTM3U");
    }

    #[test]
    fn query_encoding_matches_requests() {
        let query = query_string(&[
            ("Path", "/mnt/media/Movie (2001).strm"),
            ("Recursive", "false"),
            ("odd", "a+b&c=d é"),
        ]);
        assert_eq!(
            query,
            "Path=%2Fmnt%2Fmedia%2FMovie+%282001%29.strm&Recursive=false&odd=a%2Bb%26c%3Dd+%C3%A9"
        );
    }

    #[test]
    fn unreserved_characters_are_left_alone() {
        assert_eq!(query_string(&[("k", "aZ09-_.~")]), "k=aZ09-_.~");
    }
}
