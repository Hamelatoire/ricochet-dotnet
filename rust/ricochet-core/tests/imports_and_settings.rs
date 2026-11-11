//! Ports `WorkspaceImportTests.cs`, the `SaveSettings` cases of `WorkspaceStoreTests.cs` and `OpenApiFetcherTests.cs`:
//! unique names and globals merging for imports, saving settings without losing other keys in state.json, and
//! fetching an OpenAPI description from a loopback server.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;

use ricochet_core::models::KeyValue;
use ricochet_core::personal_state::{self, PersonalSettings};
use ricochet_core::{openapi, postman};

#[test]
fn a_clashing_name_gets_the_first_free_number_ignoring_case() {
    assert_eq!(postman::unique_name("Shop", ["Other"]), "Shop");
    assert_eq!(postman::unique_name("Shop", ["shop", "Shop (2)"]), "Shop (3)");
}

#[test]
fn merging_globals_replaces_same_names_and_lists_changed_values() {
    let existing = vec![KeyValue::new("host", "a"), KeyValue::new("same", "x")];
    let incoming = vec![KeyValue::new("host", "b"), KeyValue::new("same", "x"), KeyValue::new("new", "n")];
    let (merged, overwritten) = postman::merge_globals(&existing, &incoming);
    let pairs: Vec<(&str, &str)> = merged.iter().map(|v| (v.key.as_str(), v.value.as_str())).collect();
    assert_eq!(pairs, vec![("host", "b"), ("same", "x"), ("new", "n")]);
    assert_eq!(overwritten, vec!["host".to_string()]);
}

#[test]
fn saving_settings_keeps_every_other_key_in_state_json() {
    let dir = std::env::temp_dir().join(format!("ricochet-settings-{}", ricochet_core::models::new_id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("state.json"), r#"{"theme":"Dark","settings":{"futureSetting":7,"wrapResponse":false}}"#).unwrap();

    let settings = PersonalSettings { request_timeout_seconds: 9, verify_tls_certificates: false, ..PersonalSettings::default() };
    personal_state::save_settings(&dir, &settings).unwrap();

    let saved: serde_json::Value = serde_json::from_str(&fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
    assert_eq!(saved["theme"], "Dark");
    assert_eq!(saved["settings"]["futureSetting"], 7);
    assert_eq!(saved["settings"]["requestTimeoutSeconds"], 9);
    assert_eq!(saved["settings"]["verifyTlsCertificates"], false);
    assert_eq!(personal_state::load(&dir).state.settings, settings);
    let _ = fs::remove_dir_all(&dir);
}

/// Answers one request with `status` and `body`.
fn serve_once(status: &str, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let status = status.to_string();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                break;
            }
        }
        let reply = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        stream.write_all(reply.as_bytes()).unwrap();
    });
    format!("http://127.0.0.1:{port}/openapi.json")
}

#[test]
fn fetch_reads_the_description() {
    let address = serve_once("200 OK", r#"{"openapi":"3.0.0"}"#);
    assert_eq!(openapi::fetch(&address).unwrap(), r#"{"openapi":"3.0.0"}"#);
}

#[test]
fn fetch_reports_a_failed_status_and_refuses_other_schemes() {
    let address = serve_once("404 Not Found", "");
    assert_eq!(openapi::fetch(&address).unwrap_err(), "The server answered 404 Not Found.");
    assert_eq!(openapi::fetch("file:///etc/passwd").unwrap_err(), "Enter a web address that starts with http:// or https://.");
}
