//! Secret values, kept out of Workspace files (ADR 0004, ADR 0092). All Secrets are one JSON object, encrypted as one
//! item, so each OS needs only "read" and "write". On Windows the item is the same DPAPI file the C# app writes, at
//! `secrets.dat` in the personal directory, so both apps see the same Secrets while they run side by side.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The file name of the Secrets, in the personal directory. The same name the C# app uses.
pub const SECRETS_FILE_NAME: &str = "secrets.dat";

/// A Secret value. It prints as a placeholder, it has no `Serialize`, and its bytes are cleared when it is dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(Vec<u8>);

impl SecretValue {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into().into_bytes())
    }

    /// The text of the value. Only the code that sends the value or stores it should call this.
    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("")
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        for byte in self.0.iter_mut() {
            *byte = 0;
        }
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(<hidden>)")
    }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<hidden>")
    }
}

/// The key of a Request's auth Secret, in the C# scheme: `auth/{requestId}/{field}`.
pub fn auth_key(request_id: &str, field: &str) -> String {
    format!("auth/{request_id}/{field}")
}

/// The key of a Request's header Secret: `header/{requestId}/{name}`. The C# app keeps no header Secrets, so this key is
/// the port's own, in the same shape as the auth keys. Header names are not case-sensitive, so the name is trimmed and
/// lower-cased. A value is only ever sent under the name it was saved with.
pub fn header_key(request_id: &str, name: &str) -> String {
    format!("header/{request_id}/{}", name.trim().to_ascii_lowercase())
}

/// Why the vault could not be read or written. The message is safe to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultError {
    message: String,
}

impl VaultError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Where the Secrets are kept. Each OS has one implementation. Callers must not save over a vault that failed to
/// load, because the save replaces the whole item.
pub trait SecretVault {
    fn load(&self) -> Result<BTreeMap<String, SecretValue>, VaultError>;
    fn save(&self, secrets: &BTreeMap<String, SecretValue>) -> Result<(), VaultError>;
}

/// Keeps Secrets only until the app closes. Used when the OS offers no secure storage (port of `InMemorySecretVault`).
#[derive(Default)]
pub struct InMemoryVault {
    secrets: Mutex<BTreeMap<String, SecretValue>>,
}

impl SecretVault for InMemoryVault {
    fn load(&self) -> Result<BTreeMap<String, SecretValue>, VaultError> {
        Ok(self.secrets.lock().map(|secrets| secrets.clone()).unwrap_or_default())
    }

    fn save(&self, secrets: &BTreeMap<String, SecretValue>) -> Result<(), VaultError> {
        match self.secrets.lock() {
            Ok(mut stored) => {
                *stored = secrets.clone();
                Ok(())
            }
            Err(_) => Err(VaultError::new("Your secrets could not be saved for this session.")),
        }
    }
}

/// The Secrets file in a personal directory.
pub fn secrets_path(personal_directory: &Path) -> PathBuf {
    personal_directory.join(SECRETS_FILE_NAME)
}

/// Serialises the Secrets to the JSON object the C# app writes.
pub(crate) fn to_json(secrets: &BTreeMap<String, SecretValue>) -> Result<Vec<u8>, VaultError> {
    let plain: BTreeMap<&str, &str> = secrets.iter().map(|(key, value)| (key.as_str(), value.expose())).collect();
    serde_json::to_vec(&plain).map_err(|error| VaultError::new(format!("The secrets could not be written: {error}")))
}

/// Reads the Secrets from the JSON object the C# app writes.
pub(crate) fn from_json(bytes: &[u8]) -> Result<BTreeMap<String, SecretValue>, serde_json::Error> {
    let map: BTreeMap<String, String> = serde_json::from_slice(bytes)?;
    Ok(map.into_iter().map(|(key, value)| (key, SecretValue::new(value))).collect())
}

#[cfg(windows)]
pub use windows_vault::DpapiVault;

#[cfg(windows)]
mod windows_vault {
    use std::ffi::c_void;
    use std::fs;
    use std::path::PathBuf;

    use chrono::Utc;
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData};

    use super::{SecretValue, SecretVault, VaultError, from_json, to_json};
    use std::collections::BTreeMap;

    /// The Secrets as a DPAPI-encrypted file, tied to the current Windows user. The same format as the C# app.
    pub struct DpapiVault {
        path: PathBuf,
    }

    impl DpapiVault {
        pub fn new(path: impl Into<PathBuf>) -> Self {
            Self { path: path.into() }
        }

        pub fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    /// Encrypts with the current user's DPAPI key and no extra entropy, as the C# app does.
    fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
        let input = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` points at `plain`, which lives for the call; `output` is freed with LocalFree below.
        unsafe { CryptProtectData(&input, None, None, None, None, 0, &mut output) }.map_err(|error| error.to_string())?;
        // SAFETY: on success DPAPI sets `output` to a buffer of `cbData` bytes that we own.
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
        // SAFETY: the buffer was allocated by DPAPI with LocalAlloc, so LocalFree releases it.
        unsafe { LocalFree(HLOCAL(output.pbData as *mut c_void)) };
        Ok(bytes)
    }

    /// Decrypts a DPAPI blob. Fails when the blob was made by another user or is damaged.
    fn unprotect(encrypted: &[u8]) -> Result<Vec<u8>, String> {
        let input = CRYPT_INTEGER_BLOB { cbData: encrypted.len() as u32, pbData: encrypted.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `protect`.
        unsafe { CryptUnprotectData(&input, None, None, None, None, 0, &mut output) }.map_err(|error| error.to_string())?;
        // SAFETY: as in `protect`.
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
        // SAFETY: as in `protect`.
        unsafe { LocalFree(HLOCAL(output.pbData as *mut c_void)) };
        Ok(bytes)
    }

    impl SecretVault for DpapiVault {
        fn load(&self) -> Result<BTreeMap<String, SecretValue>, VaultError> {
            if !self.path.exists() {
                return Ok(BTreeMap::new());
            }
            let encrypted = fs::read(&self.path)
                .map_err(|error| VaultError::new(format!("Your saved secrets could not be read: {error}")))?;
            let decoded = unprotect(&encrypted).and_then(|plain| from_json(&plain).map_err(|error| error.to_string()));
            match decoded {
                Ok(secrets) => Ok(secrets),
                Err(_) => {
                    // Unreadable for good, for example after a profile reset. Keep it aside, as the C# app does.
                    let kept = format!("{}.unreadable-{}", self.path.display(), Utc::now().format("%Y%m%d%H%M%S"));
                    fs::rename(&self.path, &kept)
                        .map_err(|error| VaultError::new(format!("Your saved secrets could not be decrypted: {error}")))?;
                    Err(VaultError::new(format!("Your saved secrets could not be decrypted, and were kept at {kept}.")))
                }
            }
        }

        fn save(&self, secrets: &BTreeMap<String, SecretValue>) -> Result<(), VaultError> {
            let plain = to_json(secrets)?;
            let encrypted = protect(&plain).map_err(|error| VaultError::new(format!("Your secrets could not be saved: {error}")))?;
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| VaultError::new(format!("Your secrets could not be saved: {error}")))?;
            }
            // Written to a temporary file and moved into place, so a crash cannot leave half a vault.
            let temp = self.path.with_extension("dat.tmp");
            fs::write(&temp, &encrypted).map_err(|error| VaultError::new(format!("Your secrets could not be saved: {error}")))?;
            fs::rename(&temp, &self.path).map_err(|error| VaultError::new(format!("Your secrets could not be saved: {error}")))
        }
    }
}

/// The vault for this OS, when one is available. Windows keeps a file in the personal folder. macOS and Linux use
/// the system's store, and do not need the folder.
pub fn default_vault(personal_directory: &Path) -> Option<Box<dyn SecretVault>> {
    #[cfg(windows)]
    {
        return Some(Box::new(DpapiVault::new(secrets_path(personal_directory))));
    }
    #[cfg(target_os = "macos")]
    {
        let _ = personal_directory;
        return Some(Box::new(crate::secrets_os::KeychainVault::ricochet()));
    }
    #[cfg(target_os = "linux")]
    {
        let _ = personal_directory;
        return Some(Box::new(crate::secrets_os::SecretServiceVault::ricochet()));
    }
    #[allow(unreachable_code)]
    {
        // An unknown OS: Secrets last only for this session, as in the C# app.
        let _ = personal_directory;
        Some(Box::new(InMemoryVault::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_never_prints_its_value() {
        let secret = SecretValue::new("hunter2");
        assert_eq!(format!("{secret:?}"), "SecretValue(<hidden>)");
        assert_eq!(format!("{secret}"), "<hidden>");
        assert_eq!(secret.expose(), "hunter2");
    }

    #[test]
    fn the_in_memory_vault_round_trips_for_the_session() {
        let vault = InMemoryVault::default();
        assert!(vault.load().unwrap().is_empty());
        let mut secrets = BTreeMap::new();
        secrets.insert("k".to_string(), SecretValue::new("v"));
        vault.save(&secrets).unwrap();
        assert_eq!(vault.load().unwrap().get("k").map(|value| value.expose()), Some("v"));
    }

    #[test]
    fn keys_follow_the_csharp_scheme() {
        assert_eq!(auth_key("r1", "token"), "auth/r1/token");
    }

    #[test]
    fn the_json_matches_the_csharp_object() {
        let mut secrets = BTreeMap::new();
        secrets.insert(auth_key("r1", "password"), SecretValue::new("p@ss"));
        let bytes = to_json(&secrets).unwrap();
        assert_eq!(String::from_utf8(bytes.clone()).unwrap(), r#"{"auth/r1/password":"p@ss"}"#);
        assert_eq!(from_json(&bytes).unwrap(), secrets);
    }
}
