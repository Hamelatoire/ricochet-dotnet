//! The macOS Keychain and Linux Secret Service vaults (ADR 0092). Each keeps the same JSON object the C# app keeps, in
//! the same item: service `Ricochet`, account `secrets` on macOS, and attributes `service=ricochet account=secrets`
//! on Linux. Neither has been run on its own system from this codebase yet. CI must run them before cutover.

use std::collections::BTreeMap;

use crate::secrets::{SecretValue, SecretVault, VaultError, from_json, to_json};

/// The generic password item that holds the Secrets in the login Keychain. Same names as the C# app.
const SERVICE: &str = "Ricochet";
const ACCOUNT: &str = "secrets";

/// What the Keychain reports for an item that does not exist yet.
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(target_os = "macos")]
pub use keychain::KeychainVault;

#[cfg(target_os = "macos")]
mod keychain {
    use super::*;

    /// The Secrets as one generic password in the login Keychain.
    pub struct KeychainVault;

    impl SecretVault for KeychainVault {
        fn load(&self) -> Result<BTreeMap<String, SecretValue>, VaultError> {
            match security_framework::passwords::get_generic_password(SERVICE, ACCOUNT) {
                Ok(bytes) => from_json(&bytes)
                    .map_err(|_| VaultError::new("Your saved secrets in the Keychain could not be understood.")),
                Err(error) if error.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(BTreeMap::new()),
                Err(error) => Err(VaultError::new(format!(
                    "Your saved secrets could not be read from the Keychain (error {}).",
                    error.code()
                ))),
            }
        }

        fn save(&self, secrets: &BTreeMap<String, SecretValue>) -> Result<(), VaultError> {
            let bytes = to_json(secrets)?;
            security_framework::passwords::set_generic_password(SERVICE, ACCOUNT, &bytes).map_err(|error| {
                VaultError::new(format!("Your secrets could not be saved to the Keychain (error {}).", error.code()))
            })
        }
    }
}

#[cfg(target_os = "linux")]
pub use secret_service_vault::SecretServiceVault;

#[cfg(target_os = "linux")]
mod secret_service_vault {
    use std::collections::HashMap;

    use secret_service::{EncryptionType, SecretService};

    use super::*;

    /// The Secrets as one item in the default collection of the Secret Service.
    pub struct SecretServiceVault;

    fn attributes() -> HashMap<&'static str, &'static str> {
        HashMap::from([("service", "ricochet"), ("account", "secrets")])
    }

    fn unreachable(error: impl std::fmt::Display) -> VaultError {
        VaultError::new(format!("The Secret Service could not be reached: {error}"))
    }

    impl SecretVault for SecretServiceVault {
        fn load(&self) -> Result<BTreeMap<String, SecretValue>, VaultError> {
            async_io::block_on(async {
                let service = SecretService::connect(EncryptionType::Dh).await.map_err(unreachable)?;
                let collection = service.get_default_collection().await.map_err(unreachable)?;
                collection.ensure_unlocked().await.map_err(unreachable)?;
                let items = collection.search_items(attributes()).await.map_err(unreachable)?;
                let Some(item) = items.first() else {
                    return Ok(BTreeMap::new());
                };
                let bytes = item.get_secret().await.map_err(unreachable)?;
                from_json(&bytes).map_err(|_| VaultError::new("Your saved secrets in the Secret Service could not be understood."))
            })
        }

        fn save(&self, secrets: &BTreeMap<String, SecretValue>) -> Result<(), VaultError> {
            let bytes = to_json(secrets)?;
            async_io::block_on(async {
                let service = SecretService::connect(EncryptionType::Dh).await.map_err(unreachable)?;
                let collection = service.get_default_collection().await.map_err(unreachable)?;
                collection.ensure_unlocked().await.map_err(unreachable)?;
                // `replace` overwrites the item with the same attributes, so there is one Secrets item.
                collection
                    .create_item("Ricochet secrets", attributes(), &bytes, true, "text/plain")
                    .await
                    .map_err(|error| VaultError::new(format!("Your secrets could not be saved: {error}")))?;
                Ok(())
            })
        }
    }
}
