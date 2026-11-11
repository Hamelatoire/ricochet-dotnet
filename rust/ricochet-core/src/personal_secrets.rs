//! Port of `src/Ricochet.Core/PersonalSecrets.cs` (ADR 0026): Secrets that belong to this person on this machine, not to
//! any Workspace.
//!
//! They live in the same vault as Workspace Secrets (see `secrets`), under the `personal/` prefix, so saving a Workspace
//! never removes them. These functions only read and change the in-memory map; loading and saving the vault is up to
//! the caller, through `secrets::default_vault` in the personal directory (see `personal_state`).
//!
//! Not ported yet: `WorkspaceSecrets`, which keeps Workspace keys apart from this prefix.

use std::collections::BTreeMap;

use crate::secrets::SecretValue;

/// The prefix of every personal Secret key. Workspace Secret keys never start with it.
pub const PREFIX: &str = "personal/";

/// The key of the client certificate's password.
pub const CLIENT_CERTIFICATE_PASSWORD_KEY: &str = "personal/clientCertificatePassword";

/// The client certificate's password, or None when none is saved. An empty password counts as none.
pub fn client_certificate_password(vault: &BTreeMap<String, SecretValue>) -> Option<&str> {
    vault
        .get(CLIENT_CERTIFICATE_PASSWORD_KEY)
        .filter(|password| !password.is_empty())
        .map(SecretValue::expose)
}

/// A copy of the vault with the password replaced. An empty or missing password removes it. The original is not changed.
pub fn with_client_certificate_password(
    vault: &BTreeMap<String, SecretValue>,
    password: Option<&str>,
) -> BTreeMap<String, SecretValue> {
    let mut result = vault.clone();
    match password.filter(|password| !password.is_empty()) {
        Some(password) => {
            result.insert(CLIENT_CERTIFICATE_PASSWORD_KEY.to_string(), SecretValue::new(password));
        }
        None => {
            result.remove(CLIENT_CERTIFICATE_PASSWORD_KEY);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "client-cert-password";

    #[test]
    fn the_password_is_stored_as_a_personal_secret_and_can_be_removed() {
        let saved = with_client_certificate_password(&BTreeMap::new(), Some(PASSWORD));

        assert_eq!(client_certificate_password(&saved), Some(PASSWORD));

        let cleared = with_client_certificate_password(&saved, Some(""));
        assert_eq!(client_certificate_password(&cleared), None);
        assert!(cleared.is_empty());
    }

    #[test]
    fn a_missing_password_also_removes_it() {
        let saved = with_client_certificate_password(&BTreeMap::new(), Some(PASSWORD));

        let cleared = with_client_certificate_password(&saved, None);

        assert_eq!(client_certificate_password(&cleared), None);
        assert!(cleared.is_empty());
    }

    #[test]
    fn the_original_vault_is_not_changed() {
        let saved = with_client_certificate_password(&BTreeMap::new(), Some(PASSWORD));

        let _ = with_client_certificate_password(&saved, None);

        assert_eq!(client_certificate_password(&saved), Some(PASSWORD));
    }

    #[test]
    fn an_empty_saved_password_reads_as_none() {
        let mut vault = BTreeMap::new();
        vault.insert(CLIENT_CERTIFICATE_PASSWORD_KEY.to_string(), SecretValue::new(""));

        assert_eq!(client_certificate_password(&vault), None);
    }

    #[test]
    fn the_key_sits_under_the_personal_prefix() {
        assert_eq!(PREFIX, "personal/");
        assert_eq!(CLIENT_CERTIFICATE_PASSWORD_KEY, "personal/clientCertificatePassword");
        assert!(CLIENT_CERTIFICATE_PASSWORD_KEY.starts_with(PREFIX));
    }

    #[test]
    fn other_keys_are_kept_when_the_password_changes() {
        let mut vault = BTreeMap::new();
        vault.insert("auth/r1/token".to_string(), SecretValue::new("t"));

        let saved = with_client_certificate_password(&vault, Some(PASSWORD));

        assert_eq!(saved.get("auth/r1/token").map(SecretValue::expose), Some("t"));
        assert_eq!(client_certificate_password(&saved), Some(PASSWORD));
    }
}
