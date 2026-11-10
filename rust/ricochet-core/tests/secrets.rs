//! Tests for the Windows vault: the file is encrypted, round-trips, and a damaged file is kept aside.
#![cfg(windows)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use ricochet_core::secrets::{DpapiVault, SecretValue, SecretVault, auth_key};

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("ricochet-vault-{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample() -> BTreeMap<String, SecretValue> {
    let mut secrets = BTreeMap::new();
    secrets.insert(auth_key("r1", "token"), SecretValue::new("tok-unique-7731"));
    secrets.insert(auth_key("r2", "password"), SecretValue::new("pw-unique-4418"));
    secrets
}

#[test]
fn the_file_round_trips_and_holds_no_plain_text() {
    let dir = temp_dir("round");
    let vault = DpapiVault::new(dir.join("secrets.dat"));
    assert!(vault.load().unwrap().is_empty(), "no file means no secrets");

    vault.save(&sample()).unwrap();
    let on_disk = fs::read(dir.join("secrets.dat")).unwrap();
    let text = String::from_utf8_lossy(&on_disk);
    assert!(!text.contains("tok-unique-7731") && !text.contains("pw-unique-4418"), "the file must be encrypted");
    assert!(!dir.join("secrets.dat.tmp").exists(), "the temporary file is moved into place");

    let loaded = vault.load().unwrap();
    assert_eq!(loaded.get(&auth_key("r1", "token")).unwrap().expose(), "tok-unique-7731");
    assert_eq!(loaded, sample());
    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_damaged_file_is_kept_aside_and_reported() {
    let dir = temp_dir("damaged");
    let path = dir.join("secrets.dat");
    fs::write(&path, b"not a DPAPI blob").unwrap();

    let vault = DpapiVault::new(&path);
    let error = vault.load().unwrap_err();
    assert!(error.message().contains("could not be decrypted"), "{}", error.message());
    assert!(!path.exists(), "the damaged file is moved, not overwritten");
    let kept: Vec<_> = fs::read_dir(&dir).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    assert!(kept.iter().any(|name| name.starts_with("secrets.dat.unreadable-")), "{kept:?}");
    fs::remove_dir_all(dir).ok();
}
