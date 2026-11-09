use pretty_assertions::assert_eq;
use ricochet_core::dotenv::{self, DotEnvResult};
use ricochet_core::models::*;
use ricochet_core::variables::*;
use std::fs;
use std::path::PathBuf;

const SECRET: &str = "local-only-token-9f3a";

fn names(result: &DotEnvResult) -> Vec<&str> {
    result.variables.iter().map(|v| v.key.as_str()).collect()
}

fn values(result: &DotEnvResult) -> Vec<&str> {
    result.variables.iter().map(|v| v.value.as_str()).collect()
}

/// A fresh temporary folder, removed when dropped.
struct TempFolder(PathBuf);

impl TempFolder {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("ricochet-dotenv-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp folder");
        Self(path)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn reads_plain_quoted_and_exported_values() {
    let result = dotenv::parse("A=1\nB=\"two words\"\nexport C='three'\n");

    assert_eq!(names(&result), ["A", "B", "C"]);
    assert_eq!(values(&result), ["1", "two words", "three"]);
    assert!(result.skipped_lines.is_empty());
}

#[test]
fn blank_lines_and_comments_are_ignored() {
    let result = dotenv::parse("# a comment\n\n   \nTOKEN=abc\n");

    assert_eq!(names(&result), ["TOKEN"]);
    assert!(result.skipped_lines.is_empty());
}

#[test]
fn a_value_may_contain_equals_signs() {
    let result = dotenv::parse("KEY=abc=def==\n");

    assert_eq!(result.variables[0].value, "abc=def==");
}

#[test]
fn lines_that_are_not_assignments_are_reported_by_number() {
    let result = dotenv::parse("OK=1\nnot an assignment\n1BAD=x\n=nokey\nFINE=2\n");

    assert_eq!(names(&result), ["OK", "FINE"]);
    assert_eq!(result.skipped_lines, [2, 3, 4]);
}

#[test]
fn windows_line_endings_are_read() {
    let result = dotenv::parse("A=1\r\nB=2\r\n");

    assert_eq!(values(&result), ["1", "2"]);
}

#[test]
fn mismatched_quotes_are_kept_as_written() {
    let result = dotenv::parse("A=\"open\nB='x\"\nC=\"\"\n");

    assert_eq!(values(&result), ["\"open", "'x\"", ""]);
}

#[test]
fn export_prefix_allows_extra_spaces_before_the_name() {
    let result = dotenv::parse("export    SPACED=yes\n");

    assert_eq!(names(&result), ["SPACED"]);
    assert_eq!(values(&result), ["yes"]);
}

#[test]
fn spaces_around_the_key_and_value_are_trimmed() {
    let result = dotenv::parse("  KEY  =   value with inner spaces   \n");

    assert_eq!(names(&result), ["KEY"]);
    assert_eq!(values(&result), ["value with inner spaces"]);
}

#[test]
fn a_missing_file_gives_no_variables_and_no_error() {
    let folder = TempFolder::new("missing");

    let result = dotenv::read(&folder.0).expect("a missing file is not an error");

    assert!(result.variables.is_empty());
    assert!(result.skipped_lines.is_empty());
}

#[test]
fn the_file_is_read_from_the_workspace_folder() {
    let folder = TempFolder::new("read");
    fs::write(folder.0.join(dotenv::FILE_NAME), "TOKEN=from-file\n").unwrap();

    let result = dotenv::read(&folder.0).unwrap();

    assert_eq!(values(&result), ["from-file"]);
}

#[test]
fn a_utf8_byte_order_mark_is_not_part_of_the_first_name() {
    let folder = TempFolder::new("bom");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"FIRST=1\n");
    fs::write(folder.0.join(dotenv::FILE_NAME), bytes).unwrap();

    let result = dotenv::read(&folder.0).unwrap();

    assert_eq!(names(&result), ["FIRST"]);
    assert!(result.skipped_lines.is_empty());
}

#[test]
fn a_utf16_file_is_decoded_when_it_has_a_byte_order_mark() {
    let folder = TempFolder::new("utf16");
    let mut bytes = vec![0xFF, 0xFE];
    for unit in "WIDE=ok\n".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(folder.0.join(dotenv::FILE_NAME), bytes).unwrap();

    let result = dotenv::read(&folder.0).unwrap();

    assert_eq!(names(&result), ["WIDE"]);
    assert_eq!(values(&result), ["ok"]);
}

#[test]
fn values_are_secrets_and_an_empty_value_stays_plain() {
    let result = dotenv::parse("TOKEN=abc\nEMPTY=\nQUOTED_EMPTY=''\n");

    assert_eq!(result.variables.iter().map(|v| v.is_secret).collect::<Vec<_>>(), [true, false, false]);
    assert_eq!(values(&result), ["abc", "", ""]);
}

#[test]
fn every_parsed_variable_is_enabled_and_not_a_prompt() {
    let result = dotenv::parse("A=1\n");

    assert!(result.variables[0].enabled);
    assert!(!result.variables[0].is_prompt);
}

#[test]
fn a_dotenv_value_is_sent_when_nothing_else_has_the_name() {
    let dotenv_vars = dotenv::parse(&format!("API_TOKEN={SECRET}\n")).variables;
    let request = ApiRequest {
        url: "http://x.test/me".into(),
        auth: Some(RequestAuth {
            kind: auth_kinds::BEARER.into(),
            token: "{{API_TOKEN}}".into(),
            ..RequestAuth::default()
        }),
        ..ApiRequest::default()
    };

    let scope = layer(&[&dotenv_vars]);
    let resolved = resolve(&request, &scope);

    assert_eq!(resolved.sent.auth.as_ref().unwrap().token, SECRET);
}

#[test]
fn a_dotenv_value_overrides_a_collection_variable_and_a_later_override_wins_over_it() {
    let collection = vec![KeyValue::new("API_TOKEN", "from-collection")];
    let dotenv_vars = dotenv::parse(&format!("API_TOKEN={SECRET}\n")).variables;
    let flags = vec![KeyValue::new("API_TOKEN", "from-flag")];
    let request = ApiRequest {
        url: "http://x.test/{{API_TOKEN}}".into(),
        ..ApiRequest::default()
    };

    // The same order as the CLI: the collection, then .env, then --var, so the later one wins.
    let with_flag = layer(&[&collection, &dotenv_vars, &flags]);
    assert_eq!(resolve(&request, &with_flag).sent.url, "http://x.test/from-flag");

    let without_flag = layer(&[&collection, &dotenv_vars]);
    assert_eq!(resolve(&request, &without_flag).sent.url, format!("http://x.test/{SECRET}"));
}

#[test]
fn a_dotenv_value_is_not_written_into_the_request_or_its_history() {
    let dotenv_vars = dotenv::parse(&format!("API_TOKEN={SECRET}\n")).variables;
    let request = ApiRequest {
        url: "http://x.test/{{API_TOKEN}}".into(),
        ..ApiRequest::default()
    };

    let resolved = resolve(&request, &layer(&[&dotenv_vars]));

    assert_eq!(resolved.history.url, "http://x.test/{{API_TOKEN}}");
    assert!(!resolved.history.url.contains(SECRET));
}
