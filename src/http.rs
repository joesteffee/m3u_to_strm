//! HTTP plumbing shared by the playlist download and the Emby API calls.
//!
//! The Python original used `requests` with `timeout=10`. `ureq` is the blocking equivalent: no
//! async runtime, no thread pool, a few hundred kilobytes of code. TLS trust comes from the Mozilla
//! root store compiled into the binary, which is what makes a `scratch` container work without any
//! CA package installed.

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

pub const TIMEOUT_SECONDS: u64 = 10;

/// How long a playlist download may sit waiting for the next bytes before it is given up on.
///
/// There is deliberately no overall deadline: real playlists are tens of megabytes and a slow line
/// must be allowed to finish. An idle socket still dies, which is what the Python original (no
/// timeout at all) could not do.
///
/// The wait covers time to first byte too, and an IPTV backend that regenerates an 80 MB playlist
/// on demand can be silent for minutes before it answers, so this is generous on purpose.
pub const PLAYLIST_READ_TIMEOUT_SECONDS: u64 = 300;

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

/// Download `url` into `path`, streaming the body straight to disk.
///
/// Two things make this more than a `get_text` plus a write. `ureq` refuses to turn a body over
/// 10 MB into a `String` at all (`response too big for into_string`), and an IPTV playlist measured
/// 80 MB, so the body is copied as it arrives and never held in memory. It is also checked for
/// valid UTF-8 while streaming, which keeps the "garbage playlist is an error" rule without a
/// buffer. The bytes land in a temporary name and are renamed at the end, so an interrupted
/// download can never replace a good cached playlist.
pub fn fetch_to_file(url: &str, path: &Path) -> Result<(), String> {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    let agent = AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(TIMEOUT_SECONDS))
            .timeout_read(Duration::from_secs(PLAYLIST_READ_TIMEOUT_SECONDS))
            .build()
    });

    let response = agent.get(url).call().map_err(|err| error_text(&err))?;
    let status = response.status();
    if status != 200 {
        return Err(format!("{status} {} for url: {url}", reason(status)));
    }

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

    match stream_to(&mut response.into_reader(), &temp) {
        Ok(()) => fs::rename(&temp, path).map_err(|e| {
            format!(
                "renaming {} to {}: {}",
                temp.display(),
                path.display(),
                io_text(&e)
            )
        }),
        Err(err) => {
            let _ = fs::remove_file(&temp);
            Err(err)
        }
    }
}

fn stream_to(reader: &mut dyn Read, temp: &Path) -> Result<(), String> {
    let mut file = fs::File::create(temp)
        .map_err(|e| format!("writing {}: {}", temp.display(), io_text(&e)))?;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut utf8 = Utf8Stream::default();
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("reading the playlist: {}", io_text(&e)))?;
        if read == 0 {
            break;
        }
        utf8.push(&buffer[..read])?;
        file.write_all(&buffer[..read])
            .map_err(|e| format!("writing {}: {}", temp.display(), io_text(&e)))?;
    }
    utf8.finish()?;
    file.flush()
        .map_err(|e| format!("writing {}: {}", temp.display(), io_text(&e)))?;
    Ok(())
}

/// UTF-8 checker for a byte stream, so validity can be proven without the whole body in memory.
///
/// A character can be split across two reads, so an incomplete tail is carried over to the next
/// chunk instead of being reported as an error. Only the carried bytes (at most three) are copied.
#[derive(Default)]
struct Utf8Stream {
    carried: Vec<u8>,
    seen: usize,
}

impl Utf8Stream {
    fn push(&mut self, chunk: &[u8]) -> Result<(), String> {
        if self.carried.is_empty() {
            match std::str::from_utf8(chunk) {
                Ok(_) => {
                    self.seen += chunk.len();
                    return Ok(());
                }
                // `None` means the input merely stopped in the middle of a character.
                Err(err) if err.error_len().is_none() => {
                    let tail = chunk[err.valid_up_to()..].to_vec();
                    self.seen += err.valid_up_to();
                    self.carried = tail;
                    return Ok(());
                }
                Err(err) => return Err(invalid_utf8(self.seen + err.valid_up_to())),
            }
        }

        self.carried.extend_from_slice(chunk);
        match std::str::from_utf8(&self.carried) {
            Ok(_) => {
                self.seen += self.carried.len();
                self.carried.clear();
                Ok(())
            }
            Err(err) if err.error_len().is_none() => {
                let tail = self.carried[err.valid_up_to()..].to_vec();
                self.seen += err.valid_up_to();
                self.carried = tail;
                Ok(())
            }
            Err(err) => Err(invalid_utf8(self.seen + err.valid_up_to())),
        }
    }

    fn finish(&self) -> Result<(), String> {
        if self.carried.is_empty() {
            Ok(())
        } else {
            Err(invalid_utf8(self.seen))
        }
    }
}

fn invalid_utf8(byte: usize) -> String {
    format!("response body is not valid UTF-8: byte {byte} is not a valid sequence")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Answer one request with `body`, on a port of its own.
    fn serve_once(body: Vec<u8>) -> String {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/playlist.m3u",
            listener.local_addr().unwrap().port()
        );
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            );
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        });
        url
    }

    #[test]
    fn a_playlist_larger_than_the_string_limit_downloads() {
        // ureq's into_string gives up at 10 MB; a real subscriber playlist measured 80 MB.
        let body = "#EXTINF:-1 tvg-name=\"Channel\",Channel\nhttp://x/1.ts\n".repeat(400_000);
        let size = body.len();
        assert!(
            size > 10 * 1024 * 1024,
            "fixture must exceed the {} MB ureq gives into_string",
            size
        );
        let root = tempdir().unwrap();
        let target = root.path().join("playlist.m3u");
        fetch_to_file(&serve_once(body.into_bytes()), &target).unwrap();
        let downloaded = std::fs::read(&target).unwrap();
        assert_eq!(downloaded.len(), size);
        assert!(!target
            .with_file_name("playlist.m3u.m3u-to-strm.part")
            .exists());
    }

    #[test]
    fn a_body_that_is_not_utf8_is_an_error_and_is_not_kept() {
        let root = tempdir().unwrap();
        let target = root.path().join("playlist.m3u");
        let url = serve_once(b"#EXTM3U\n#EXTINF:-1 tvg-name=\"Bad \xff\xfe name\"\n".to_vec());
        let err = fetch_to_file(&url, &target).unwrap_err();
        assert!(err.contains("not valid UTF-8"), "unexpected error: {err}");
        assert!(!target.exists(), "a rejected playlist must not land");
    }

    #[test]
    fn a_download_lands_in_a_directory_that_does_not_exist_yet() {
        let root = tempdir().unwrap();
        let target = root.path().join("nested/tmp/playlist.m3u");
        fetch_to_file(&serve_once(b"#EXTM3U\n".to_vec()), &target).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "#EXTM3U\n");
    }

    #[test]
    fn a_character_split_between_reads_is_not_an_error() {
        let mut stream = Utf8Stream::default();
        for byte in "#EXTINF:-1 Café \u{1F3AC}".as_bytes().chunks(1) {
            stream.push(byte).unwrap();
        }
        stream.finish().unwrap();
    }

    #[test]
    fn a_playlist_that_ends_mid_character_is_an_error() {
        let mut stream = Utf8Stream::default();
        stream.push(b"name: caf\xc3").unwrap();
        let err = stream.finish().unwrap_err();
        assert!(err.contains("not valid UTF-8"), "unexpected error: {err}");
    }

    #[test]
    fn an_invalid_byte_is_reported_at_its_position() {
        let mut stream = Utf8Stream::default();
        let err = stream.push(b"abcd\xffef").unwrap_err();
        assert_eq!(
            err,
            "response body is not valid UTF-8: byte 4 is not a valid sequence"
        );
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
