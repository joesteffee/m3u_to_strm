//! Parity contract with the original Python implementation.
//!
//! `tests/fixtures/parity.json` records the outputs of `parse_m3u.py` for a corpus of inputs,
//! captured before the rewrite. Every case is replayed against the Rust functions here, so the
//! rewritten behaviour cannot drift from the behaviour users depend on — and any future change
//! that does drift fails this test. The fixture is plain JSON: no Python is needed to run it.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use m3u_to_strm::config::{self, Config};
use m3u_to_strm::fsx::Fs;
use m3u_to_strm::naming;
use m3u_to_strm::strm;

fn fixture_cases() -> Vec<Value> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parity.json");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let document: Value = serde_json::from_str(&text).expect("parity.json is not valid JSON");
    document["cases"]
        .as_array()
        .expect("parity.json needs a cases array")
        .clone()
}

/// A string is a code list here; `null` or `""` means "not configured", like the env var.
fn codes(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string).filter(|v| !v.is_empty())
}

fn rust_output(name: &str, args: &[Value]) -> Value {
    let text = |index: usize| -> String {
        args[index]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: argument {index} is not a string"))
            .to_string()
    };

    match name {
        "safe_filename" => json!(naming::safe_filename(&text(0))),
        "parse_movie_name" => json!(naming::parse_movie_name(&text(0))),
        "parse_series_name" => json!(naming::parse_series_name(&text(0))),
        "extract_season_episode" => {
            let (season, episode) = naming::extract_season_episode(&text(0));
            json!([season, episode])
        }
        "extract_content_id" => json!(naming::extract_content_id(&text(0))),
        "extract_country_code" => json!(naming::extract_country_code(&text(0))),
        "should_filter_channel" => {
            // Built through the same env-parsing path the binary uses, so the code normalisation
            // (trim, uppercase, dedupe, empty-means-unset) is covered too.
            let config = Config::from_lookup(|key| match key {
                "INCLUDE_COUNTRY_CODES" => args[1].as_array().map(|list| {
                    list.iter()
                        .map(|v| v.as_str().unwrap())
                        .collect::<Vec<_>>()
                        .join(",")
                }),
                "FILTER_COUNTRY_CODES" => args[2].as_array().map(|list| {
                    list.iter()
                        .map(|v| v.as_str().unwrap())
                        .collect::<Vec<_>>()
                        .join(",")
                }),
                _ => None,
            })
            .expect("country code config");
            json!(config::filtered_by_country(
                &text(0),
                &config.country_rule()
            ))
        }
        "convert_to_emby_path" => {
            let config = Config::from_lookup(|key| match key {
                "EMBY_MOVIES_PATH" => codes(&args[1]),
                "EMBY_SERIES_PATH" => codes(&args[2]),
                "EMBY_LIVETV_PATH" => codes(&args[3]),
                _ => None,
            })
            .expect("path mapping config");
            json!(config.emby_path(&PathBuf::from(text(0))))
        }
        other => panic!("no Rust binding for fixture function {other}"),
    }
}

#[test]
fn rust_matches_the_python_reference_for_every_case() {
    let cases = fixture_cases();
    assert!(
        cases.len() > 300,
        "the parity corpus should stay broad, found {}",
        cases.len()
    );

    let mut failures = Vec::new();
    for case in &cases {
        let name = case["fn"].as_str().expect("case needs fn");
        let args = case["args"].as_array().expect("case needs args");
        let got = rust_output(name, args);
        if got != case["expect"] {
            failures.push(format!(
                "{name}({}): python = {}, rust = {}",
                serde_json::to_string(args).unwrap(),
                serde_json::to_string(&case["expect"]).unwrap(),
                serde_json::to_string(&got).unwrap(),
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} parity cases differ from the Python reference:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// Replay the recorded `write_strm_file` decisions: given the files already on disk, which file
/// does the converter pick, and what does the directory look like afterwards?
#[test]
fn strm_file_selection_matches_the_python_reference() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parity.json");
    let text = fs::read_to_string(&path).unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let cases = document["strm_cases"]
        .as_array()
        .expect("parity.json needs a strm_cases array");
    assert!(
        cases.len() > 15,
        "expected a broad corpus, found {}",
        cases.len()
    );

    let mut failures = Vec::new();
    for case in cases {
        let args = case["args"].as_array().expect("case args");
        let existing = args[0].as_object().expect("existing files object");
        let filename = args[1].as_str().expect("filename");
        let url = args[2].as_str().expect("url");
        let content_id = args[3].as_str();

        let workspace = tempfile::tempdir().expect("tempdir");
        for (name, contents) in existing {
            fs::write(
                workspace.path().join(name),
                contents.as_str().expect("file contents"),
            )
            .unwrap();
        }

        let outcome = strm::write(&mut Fs::new(), workspace.path(), filename, url, content_id)
            .expect("write_strm_file failed");
        let files: serde_json::Map<String, Value> = fs::read_dir(workspace.path())
            .unwrap()
            .flatten()
            .map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let contents = fs::read_to_string(entry.path()).unwrap();
                (name, Value::String(contents))
            })
            .collect();

        let got = json!({
            "path": outcome.path.file_name().unwrap().to_string_lossy(),
            "is_new": outcome.is_new,
            "url_changed": outcome.url_changed,
            "files": Value::Object(files),
        });
        if got != case["expect"] {
            failures.push(format!(
                "write_strm_file({}):\n  python = {}\n  rust   = {}",
                serde_json::to_string(args).unwrap(),
                serde_json::to_string_pretty(&case["expect"]).unwrap(),
                serde_json::to_string_pretty(&got).unwrap(),
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} write_strm_file cases differ from the Python reference:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn every_reference_function_is_covered() {
    let covered: Vec<String> = fixture_cases()
        .iter()
        .map(|case| case["fn"].as_str().expect("case needs fn").to_string())
        .collect();
    for function in [
        "safe_filename",
        "parse_movie_name",
        "parse_series_name",
        "extract_season_episode",
        "extract_content_id",
        "extract_country_code",
        "should_filter_channel",
        "convert_to_emby_path",
    ] {
        assert!(
            covered.iter().any(|seen| seen == function),
            "no parity cases for {function}"
        );
    }
}
