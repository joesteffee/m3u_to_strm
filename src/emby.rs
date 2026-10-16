//! Emby library API client.
//!
//! Every call is a no-op when `EMBY_SERVER_URL`/`EMBY_API_KEY` are unset, and every failure is
//! logged and swallowed: a dead Emby server must not stop the converter from writing STRMs.
//! All requests carry a 10 second timeout, like the original.

use std::path::Path;

use crate::config::Config;
use crate::http;
use crate::log;

/// Library refreshes and item deletions are pure network latency, so they run on a few threads.
pub const NETWORK_WORKERS: usize = 4;

/// The three verbs the Emby library API needs.
#[derive(Copy, Clone, Debug)]
enum Method {
    Get,
    Post,
    Delete,
}

pub struct Emby {
    config: Config,
    base_url: String,
    api_key: String,
    agent: ureq::Agent,
    enabled: bool,
}

impl Emby {
    pub fn new(config: &Config) -> Emby {
        Emby::with_agent(config, http::agent())
    }

    /// Separate agent so tests can point the client at a local mock server.
    pub fn with_agent(config: &Config, agent: ureq::Agent) -> Emby {
        let base_url = config
            .emby_server_url
            .as_deref()
            .unwrap_or("")
            .trim_end_matches('/')
            .to_string();
        let api_key = config.emby_api_key.clone().unwrap_or_default();
        Emby {
            config: config.clone(),
            base_url,
            api_key,
            agent,
            enabled: config.emby_enabled(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    fn send(&self, method: Method, url: &str) -> Result<String, String> {
        let request = match method {
            Method::Get => self.agent.get(url),
            Method::Post => self.agent.post(url),
            Method::Delete => self.agent.delete(url),
        };
        let request = request.set("X-Emby-Token", &self.api_key);
        match request.call() {
            Ok(response) => response
                .into_string()
                .map_err(|e| format!("reading response failed: {e}")),
            Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
            Err(e) => Err(http::error_text(&e)),
        }
    }

    /// Item ID of the library entry that matches this container path, if Emby knows it.
    pub fn item_id_by_path(&self, container_path: &Path) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let emby_path = self.config.emby_path(container_path);
        log::debug(format!(
            "Looking up Emby item for path: {emby_path} (container: {})",
            container_path.display()
        ));
        let url = format!(
            "{}/emby/Items?{}",
            self.base_url,
            http::query_string(&[("Path", &emby_path), ("Recursive", "false")])
        );
        match self.send(Method::Get, &url) {
            Ok(body) => match parse::first_item_id(&body) {
                Some(parse::Parsed::Id(id)) => Some(id),
                Some(parse::Parsed::Missing) => None,
                None => {
                    log::warning(format!(
                        "⚠️ Error finding Emby item for {}: response is not valid JSON",
                        container_path.display()
                    ));
                    None
                }
            },
            Err(error) => {
                log::warning(format!(
                    "⚠️ Error finding Emby item for {}: {error}",
                    container_path.display()
                ));
                None
            }
        }
    }

    pub fn refresh_item(&self, item_id: &str, update_type: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let url = format!(
            "{}/emby/Items/{item_id}/Refresh?{}",
            self.base_url,
            http::query_string(&[
                ("Recursive", "false"),
                ("ImageRefreshMode", "FullRefresh"),
                ("MetadataRefreshMode", "FullRefresh"),
                ("ReplaceAllImages", "false"),
                ("ReplaceAllMetadata", "false"),
            ])
        );
        match self.send(Method::Post, &url) {
            Ok(_) => {
                log::info(format!("✅ Emby item {item_id} refreshed ({update_type})"));
                true
            }
            Err(error) => {
                log::warning(format!("⚠️ Error refreshing Emby item {item_id}: {error}"));
                false
            }
        }
    }

    /// Ask Emby to scan the library folder that contains `container_path`.
    pub fn refresh_library(&self, container_path: &Path) -> bool {
        if !self.enabled {
            return false;
        }
        let emby_path = self.config.emby_path(container_path);
        let shown = container_path.display();
        log::debug(format!(
            "Triggering Emby library refresh for path: {emby_path} (container: {shown})"
        ));
        let url = format!(
            "{}/emby/Library/Refresh?{}",
            self.base_url,
            http::query_string(&[("Path", &emby_path), ("Recursive", "true")])
        );
        match self.send(Method::Post, &url) {
            Ok(_) => {
                log::info(format!(
                    "✅ Emby library refresh triggered for {shown} (will scan for new files)"
                ));
                true
            }
            Err(error) => {
                log::warning(format!(
                    "⚠️ Error triggering Emby library refresh for {shown}: {error}"
                ));
                false
            }
        }
    }

    pub fn delete_item(&self, item_id: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let url = format!("{}/emby/Items/{item_id}", self.base_url);
        match self.send(Method::Delete, &url) {
            Ok(_) => {
                log::info(format!("✅ Emby item {item_id} deleted from library"));
                true
            }
            Err(error) => {
                log::warning(format!("⚠️ Error deleting Emby item {item_id}: {error}"));
                false
            }
        }
    }

    /// A file whose URL changed: refresh the known item, or fall back to scanning its folder.
    pub fn notify_updated(&self, container_path: &Path) {
        let shown = container_path.display();
        if !self.enabled {
            log::debug(format!(
                "Emby notification skipped (not configured) for {shown}"
            ));
            return;
        }
        match self.item_id_by_path(container_path) {
            Some(item_id) => {
                self.refresh_item(&item_id, "Update");
            }
            None => {
                log::warning(format!(
                    "⚠️ Item not found in Emby for {shown}, triggering library refresh"
                ));
                let parent = container_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| container_path.to_path_buf());
                self.refresh_library(&parent);
            }
        }
    }

    pub fn batch_refresh(&self, directories: &[&Path]) {
        if !self.enabled || directories.is_empty() {
            return;
        }
        let word = if directories.len() == 1 {
            "directory"
        } else {
            "directories"
        };
        log::info(format!(
            "Triggering Emby library refresh for {} {word} with new files",
            directories.len()
        ));
        for directory in directories {
            self.refresh_library(directory);
        }
    }

    /// Refresh every changed file in parallel: each call is a round trip to Emby.
    pub fn notify_all_updated(&self, paths: &[&Path]) {
        run_in_parallel(paths, |path| self.notify_updated(path));
    }

    /// Look up and delete each orphaned file in parallel, returning how many items Emby recognised.
    pub fn delete_all(&self, paths: &[&Path]) -> usize {
        run_in_parallel(paths, |path| {
            match self.item_id_by_path(path) {
                Some(item_id) => {
                    self.delete_item(&item_id);
                    // The original counted recognised items, whatever the DELETE call returned.
                    1
                }
                None => {
                    log::debug(format!(
                        "Item not found in Emby for {}, skipping Emby deletion",
                        path.display()
                    ));
                    0
                }
            }
        })
        .into_iter()
        .sum()
    }

    /// Whether a missing/unreachable server should be retried; used to keep log noise down.
    pub fn is_connection_failure(error: &str) -> bool {
        error.contains("dns") || error.contains("Connection refused") || error.contains("timeout")
    }
}

/// Run `work` over `items` on a small pool of threads, preserving input order in the result.
///
/// Threads are cheaper to spin up than a tokio runtime for a job that is a handful of blocking
/// HTTP calls, and it keeps the binary dependency-light.
fn run_in_parallel<T: Send + Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let work = &work;
    if items.len() < 2 || !crate::log::parallel_http_enabled() {
        return items.iter().map(&work).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let items = &items;
    let results: std::sync::Mutex<Vec<Option<R>>> = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        let _ = &next;
        let _ = &results;
        for _ in 0..NETWORK_WORKERS.min(items.len()) {
            let next = &next;
            let results = &results;
            scope.spawn(move || loop {
                let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if index >= items.len() {
                    break;
                }
                let result = work(&items[index]);
                let mut results = results.lock().unwrap_or_else(|error| error.into_inner());
                if results.len() > index {
                    results[index] = Some(result);
                } else {
                    while results.len() < index {
                        results.push(None);
                    }
                    results.push(Some(result));
                }
            });
        }
    });
    let results = results.into_inner().unwrap_or_else(|e| e.into_inner());
    results
        .into_iter()
        .map(|slot| slot.expect("every item is produced exactly once"))
        .collect()
}

/// Minimum slice of the Emby `Items` response we need: the first item's `Id`.
///
/// Hand-rolled instead of pulling in serde: one string lookup per request, and it keeps the
/// dependency tree (and cold-start cost) small.
mod parse {
    pub enum Parsed {
        Id(String),
        Missing,
    }

    struct Scan<'a> {
        text: &'a str,
        bytes: &'a [u8],
        at: usize,
    }

    impl<'a> Scan<'a> {
        fn new(text: &'a str) -> Scan<'a> {
            Scan {
                text,
                bytes: text.as_bytes(),
                at: 0,
            }
        }

        fn peek(&self) -> Option<u8> {
            self.bytes.get(self.at).copied()
        }

        fn skip_ws(&mut self) {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.at += 1;
            }
        }

        fn take(&mut self, byte: u8) -> Option<()> {
            if self.peek() == Some(byte) {
                self.at += 1;
                Some(())
            } else {
                None
            }
        }

        /// Read a quoted string. Returns the raw source span, not JSON-unescaped, which is fine
        /// for keys and for the item IDs Emby returns (hex GUIDs).
        fn string(&mut self) -> Option<&'a str> {
            self.take(b'"')?;
            let start = self.at;
            loop {
                match self.bytes.get(self.at)? {
                    b'"' => {
                        let value = &self.text[start..self.at];
                        self.at += 1;
                        return Some(value);
                    }
                    b'\\' => self.at += 2,
                    _ => self.at += 1,
                }
            }
        }

        fn skip_value(&mut self) -> Option<()> {
            self.skip_ws();
            match self.peek()? {
                b'"' => {
                    self.string()?;
                    Some(())
                }
                // An object is `key: value` pairs, an array is bare values: skipping them with one
                // loop would stop at the `:` of every object member and reject the whole document.
                b'{' => {
                    self.at += 1;
                    self.skip_ws();
                    if self.take(b'}').is_some() {
                        return Some(());
                    }
                    loop {
                        self.skip_ws();
                        self.string()?;
                        self.skip_ws();
                        self.take(b':')?;
                        self.skip_value()?;
                        self.skip_ws();
                        if self.take(b',').is_some() {
                            continue;
                        }
                        return self.take(b'}');
                    }
                }
                b'[' => {
                    self.at += 1;
                    self.skip_ws();
                    if self.take(b']').is_some() {
                        return Some(());
                    }
                    loop {
                        self.skip_value()?;
                        self.skip_ws();
                        if self.take(b',').is_some() {
                            continue;
                        }
                        return self.take(b']');
                    }
                }
                _ => {
                    let start = self.at;
                    while !matches!(
                        self.peek()?,
                        b',' | b'}' | b']' | b' ' | b'\n' | b'\t' | b'\r'
                    ) {
                        self.at += 1;
                    }
                    if self.at == start {
                        None
                    } else {
                        Some(())
                    }
                }
            }
        }

        /// Walk an object looking for `wanted`, returning its scalar value as text.
        fn scalar_at(&mut self, wanted: &str) -> Option<Option<String>> {
            self.skip_ws();
            self.take(b'{')?;
            self.skip_ws();
            if self.peek() == Some(b'}') {
                self.at += 1;
                return Some(None);
            }
            loop {
                self.skip_ws();
                let key = self.string()?;
                self.skip_ws();
                self.take(b':')?;
                self.skip_ws();
                if key == wanted {
                    let value = self.scalar()?;
                    return Some(Some(value));
                }
                self.skip_value()?;
                self.skip_ws();
                if self.take(b',').is_some() {
                    continue;
                }
                self.take(b'}')?;
                return Some(None);
            }
        }

        fn scalar(&mut self) -> Option<String> {
            match self.peek()? {
                b'"' => self.string().map(|s| s.to_string()),
                _ => {
                    let start = self.at;
                    while !matches!(
                        self.peek()?,
                        b',' | b'}' | b']' | b' ' | b'\n' | b'\t' | b'\r'
                    ) {
                        self.at += 1;
                    }
                    if self.at == start {
                        None
                    } else {
                        Some(self.text[start..self.at].to_string())
                    }
                }
            }
        }
    }

    /// `Some(None)` means valid JSON with no usable item; `None` means the body was not JSON.
    pub fn first_item_id(body: &str) -> Option<Parsed> {
        let mut scan = Scan::new(body);
        scan.skip_ws();
        scan.take(b'{')?;
        scan.skip_ws();
        if scan.peek() == Some(b'}') {
            return Some(Parsed::Missing);
        }
        loop {
            scan.skip_ws();
            let key = scan.string()?;
            scan.skip_ws();
            scan.take(b':')?;
            scan.skip_ws();
            if key == "Items" {
                scan.take(b'[')?;
                scan.skip_ws();
                if scan.peek() != Some(b'{') {
                    return Some(Parsed::Missing);
                }
                return Some(match scan.scalar_at("Id")? {
                    Some(id) => Parsed::Id(id),
                    None => Parsed::Missing,
                });
            }
            scan.skip_value()?;
            scan.skip_ws();
            if scan.take(b',').is_some() {
                continue;
            }
            return Some(Parsed::Missing);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse::{first_item_id, Parsed};

    fn parsed(body: &str) -> Option<Option<String>> {
        match first_item_id(body) {
            Some(Parsed::Id(id)) => Some(Some(id)),
            Some(Parsed::Missing) => Some(None),
            None => None,
        }
    }

    #[test]
    fn reads_the_first_item_id() {
        assert_eq!(
            parsed(r#"{"Items":[{"Id":"42","Name":"X"},{"Id":"43"}],"TotalRecordCount":2}"#),
            Some(Some("42".into()))
        );
    }

    #[test]
    fn an_empty_or_absent_items_list_is_not_an_error() {
        assert_eq!(parsed(r#"{"Items":[]}"#), Some(None));
        assert_eq!(parsed(r#"{"TotalRecordCount":0}"#), Some(None));
        assert_eq!(parsed("{}"), Some(None));
    }

    #[test]
    fn malformed_bodies_are_reported_as_errors() {
        assert_eq!(parsed("<html>500</html>"), None);
        assert_eq!(parsed(""), None);
        assert_eq!(parsed(r#"{"Items":"nope"}"#), None);
    }

    #[test]
    fn sibling_keys_and_nested_objects_do_not_confuse_the_lookup() {
        // "Id" nested inside another value must not be mistaken for the item id.
        assert_eq!(
            parsed(r#"{"Total":5,"Items":[{"Name":"M","ProviderIds":{"Id":"bad"},"Id":"good"}]}"#),
            Some(Some("good".into()))
        );
        // A string value that mentions "Items" must not be treated as the array.
        assert_eq!(
            parsed(r#"{"Note":"Items: []","Items":[{"Id":"7"}]}"#),
            Some(Some("7".into()))
        );
        assert_eq!(
            parsed(r#"{"Items":[{"ProviderIds":{"Id":"inner"}}]}"#),
            Some(None)
        );
    }

    #[test]
    fn escaped_quotes_inside_strings_are_skipped() {
        assert_eq!(
            parsed(r#"{"Note":"a \" char","Items":[{"Id":"9"}]}"#),
            Some(Some("9".into()))
        );
    }
}
