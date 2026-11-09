use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use ricochet_core::crash_log::*;
use uuid::Uuid;

/// A fresh folder under the system temp directory, removed when the test ends. Tests never write anywhere else.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("ricochet-crash-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("temp dir can be created");
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn context() -> CrashContext {
    CrashContext {
        app_version: "1.2.3".into(),
        os_description: "Test OS".into(),
        is_64_bit: true,
    }
}

fn entry(kind: &str, trace: &str) -> CrashEntry {
    CrashEntry {
        kind: kind.into(),
        trace: Some(trace.into()),
    }
}

fn at(minute: u32) -> Option<DateTime<Utc>> {
    Some(Utc.with_ymd_and_hms(2026, 1, 1, 0, minute, 0).unwrap())
}

fn crash_files(personal: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory_for(personal))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn write_creates_a_log_under_the_crashes_folder() {
    let dir = TempDir::new();
    let path = write(dir.path(), &[entry("System.InvalidOperationException", "   at Foo()")], &context(), at(0))
        .expect("log is written");

    assert!(path.starts_with(directory_for(dir.path())));
    assert!(path.exists());
    assert_eq!(path.file_name().unwrap().to_string_lossy(), "crash-20260101-000000-000.log");
}

#[test]
fn log_holds_kind_app_version_and_os() {
    let dir = TempDir::new();
    let path = write(dir.path(), &[entry("System.InvalidOperationException", "   at Foo()")], &context(), at(0))
        .unwrap();

    let text = fs::read_to_string(path).unwrap();

    assert!(text.contains("System.InvalidOperationException"));
    assert!(text.contains("App version: 1.2.3"));
    assert!(text.contains("Operating system: Test OS (64-bit)"));
}

#[test]
fn log_states_that_nothing_is_sent() {
    let text = format(&[entry("Boom", "")], &context(), Utc::now(), "");

    assert!(text.contains("Nothing is sent anywhere."));
}

#[test]
fn inner_errors_are_written_with_their_kinds_in_order() {
    let entries = [entry("Outer", "   at Outer()"), entry("Inner", "   at Inner()")];

    let text = format(&entries, &context(), Utc::now(), "");

    let outer = text.find("Exception:\n  Outer").expect("outer is written as the exception");
    let inner = text.find("Inner exception:\n  Inner").expect("inner is written as an inner exception");
    assert!(outer < inner);
    assert!(text.contains("    at Inner()"));
}

#[test]
fn an_entry_without_a_trace_writes_only_its_kind() {
    let entries = [CrashEntry { kind: "Bare".into(), trace: None }];

    let text = format(&entries, &context(), Utc::now(), "");

    assert!(text.contains("Exception:\n  Bare\n"));
}

#[test]
fn profile_folder_is_shortened_so_the_user_name_is_not_written() {
    let profile = r"C:\Users\alex";
    let trace = format!(r"   at Foo() in {profile}\src\Foo.rs:line 3");

    let redacted = redact_profile(&trace, profile);

    assert!(!redacted.contains(profile));
    assert_eq!(redacted, r"   at Foo() in ~\src\Foo.rs:line 3");
}

#[test]
fn profile_match_ignores_case() {
    assert_eq!(redact_profile(r"c:\users\ALEX\x", r"C:\Users\alex"), r"~\x");
}

#[test]
fn an_empty_profile_leaves_the_text_unchanged() {
    assert_eq!(redact_profile("at Foo()", ""), "at Foo()");
}

#[test]
fn the_written_log_has_the_profile_shortened_in_its_traces() {
    let profile = r"C:\Users\alex";
    let text = format(&[entry("Boom", &format!(r"   at Foo() in {profile}\a.rs"))], &context(), Utc::now(), profile);

    assert!(!text.contains("alex"));
    assert!(text.contains(r"~\a.rs"));
}

#[test]
fn credentials_in_traces_are_masked() {
    let trace = "   at Call() in https://internal.example/api?token=abc123&page=2\n\
                 password=hunter2 \n\
                 headers: Authorization: Bearer eyJhbGci.payload.sig\n\
                 api_key: k-999-live\n\
                 bare Bearer zzz.yyy";

    let text = format(&[entry("Boom", trace)], &context(), Utc::now(), "");

    for secret in ["abc123", "hunter2", "eyJhbGci", "k-999-live", "zzz.yyy"] {
        assert!(!text.contains(secret), "{secret} leaked into the log");
    }
    assert!(text.contains("token=***&page=2"), "the name stays and the other parameter is kept");
    assert!(text.contains("password=***"));
    assert!(text.contains("api_key: ***"));
    assert!(text.contains("Bearer ***"));
}

#[test]
fn mask_secrets_leaves_ordinary_text_alone() {
    let text = "   at Ricochet.Core.Sender.Send() in Sender.cs:line 42";

    assert_eq!(mask_secrets(text), text);
}

#[test]
fn secrets_are_masked_in_the_file_on_disk() {
    let dir = TempDir::new();
    let trace = "   at Call() in https://internal.example/api?token=abc123";

    let path = write(dir.path(), &[entry("Boom", trace)], &context(), at(0)).unwrap();

    let text = fs::read_to_string(path).unwrap();
    assert!(!text.contains("abc123"));
    assert!(text.contains("token=***"));
}

#[test]
fn the_log_says_what_it_leaves_out() {
    let text = format(&[entry("Boom", "")], &context(), Utc::now(), "");

    assert!(text.contains("Exception messages and request data are left out."));
}

#[test]
fn old_logs_are_pruned_past_the_limit_and_newer_ones_are_kept() {
    let dir = TempDir::new();
    let mut newest = None;
    for minute in 0..(MAX_LOGS as u32 + 5) {
        newest = write(dir.path(), &[entry("Boom", "")], &context(), at(minute));
    }

    let kept = crash_files(dir.path());

    assert_eq!(kept.len(), MAX_LOGS);
    let newest = newest.unwrap();
    assert!(newest.exists());
    assert!(!kept.iter().any(|name| name == "crash-20260101-000000-000.log"));
    assert!(kept.iter().any(|name| name == "crash-20260101-002400-000.log"));
    assert!(kept.iter().any(|name| name == "crash-20260101-000500-000.log"));
}

#[test]
fn prune_leaves_files_it_did_not_write_alone() {
    let dir = TempDir::new();
    let folder = directory_for(dir.path());
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("notes.txt"), "mine").unwrap();
    fs::write(folder.join("crash-old.txt"), "not a log").unwrap();

    for minute in 0..(MAX_LOGS as u32 + 2) {
        write(dir.path(), &[entry("Boom", "")], &context(), at(minute));
    }

    assert!(folder.join("notes.txt").exists());
    assert!(folder.join("crash-old.txt").exists());
}

#[test]
fn write_returns_none_instead_of_panicking_when_the_folder_cannot_be_written() {
    let dir = TempDir::new();
    let blocker = dir.path().join("not-a-folder");
    fs::write(&blocker, "file in the way").unwrap();

    let path = write(&blocker, &[entry("Boom", "")], &context(), at(0));

    assert_eq!(path, None);
}
