//! Tests for the macOS Keychain and Linux Secret Service vaults. Each test names its own item, so it never touches
//! a real user's Secrets, and removes it afterwards. CI runs these on macOS and Linux (ADR 0092).
#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use ricochet_core::secrets::{SecretValue, SecretVault, auth_key};
#[cfg(target_os = "macos")]
use ricochet_core::secrets_os::KeychainVault as Vault;
#[cfg(target_os = "linux")]
use ricochet_core::secrets_os::SecretServiceVault as Vault;

fn unique_name() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("ricochet-test-{}-{nanos}", std::process::id())
}

fn sample() -> BTreeMap<String, SecretValue> {
    let mut secrets = BTreeMap::new();
    secrets.insert(auth_key("r1", "token"), SecretValue::new("tok-os-1"));
    secrets.insert(auth_key("r2", "password"), SecretValue::new("pw-os-2"));
    secrets
}

#[test]
fn an_item_that_does_not_exist_loads_as_empty() {
    let vault = Vault::named(&unique_name());
    assert!(vault.load().unwrap().is_empty());
    vault.forget().unwrap();
}

#[test]
fn saved_secrets_load_back_and_saving_again_replaces_them() {
    let name = unique_name();
    let vault = Vault::named(&name);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vault.save(&sample()).unwrap();
        assert_eq!(vault.load().unwrap(), sample());

        let mut changed = sample();
        changed.remove(&auth_key("r2", "password"));
        vault.save(&changed).unwrap();
        assert_eq!(vault.load().unwrap(), changed, "the save replaces the whole item");
    }));
    vault.forget().unwrap();
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
