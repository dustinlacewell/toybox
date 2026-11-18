//! Where access tokens live: the operating system's credential store (Windows
//! Credential Manager, macOS Keychain, the Secret Service on Linux). A token is
//! never written to `settings.json`, to a log or into an error message.
//!
//! The store sits behind a small trait so tests (and a machine without a
//! credential store) can use [`MemoryStore`] without touching the real one.

use std::collections::HashMap;
use std::sync::Mutex;

/// The service name tokens are filed under; the account is the library id.
pub const SERVICE: &str = "Toybox";

/// A credential-store failure, already phrased for a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretError(pub String);

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SecretError {}

pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError>;
    /// Removing what is not there is not an error.
    fn delete(&self, account: &str) -> Result<(), SecretError>;
}

/// The OS credential store.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

fn unavailable(action: &str, error: &keyring::Error) -> SecretError {
    SecretError(format!(
        "Toybox could not {action} in the system credential store ({}). Access tokens are kept only there, never in Toybox's own files, so remote libraries cannot be used until the credential store works.",
        // The keyring error text never contains the secret.
        error
    ))
}

impl KeyringStore {
    fn entry(account: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(SERVICE, account).map_err(|e| unavailable("open it", &e))
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        match Self::entry(account)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(unavailable("read the access token", &e)),
        }
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        Self::entry(account)?.set_password(secret).map_err(|e| unavailable("save the access token", &e))
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(unavailable("remove the access token", &e)),
        }
    }
}

/// A store that lives in memory. Used by the tests.
#[derive(Default)]
pub struct MemoryStore {
    secrets: Mutex<HashMap<String, String>>,
    /// Fail every call, like a machine whose credential store is locked or missing.
    pub broken: Mutex<bool>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn accounts(&self) -> Vec<String> {
        let mut accounts: Vec<String> = self.secrets.lock().unwrap().keys().cloned().collect();
        accounts.sort();
        accounts
    }
    fn check(&self) -> Result<(), SecretError> {
        if *self.broken.lock().unwrap() {
            Err(SecretError("Toybox could not reach the system credential store.".into()))
        } else {
            Ok(())
        }
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        self.check()?;
        Ok(self.secrets.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.check()?;
        self.secrets.lock().unwrap().insert(account.to_string(), secret.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), SecretError> {
        self.check()?;
        self.secrets.lock().unwrap().remove(account);
        Ok(())
    }
}
