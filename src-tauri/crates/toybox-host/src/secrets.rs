//! A credential store that keeps tokens in a plain JSON file. Only for tests and
//! automation that must not touch the operating system's credential store
//! (`tbx` selects it with `TOYBOX_SECRETS=memory:<file>`); the file is not protected.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use toybox_remote::secrets::{SecretError, SecretStore};

pub struct FileStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileStore { path: path.into(), lock: Mutex::new(()) }
    }

    fn read(&self) -> BTreeMap<String, String> {
        std::fs::read(&self.path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    fn write(&self, map: &BTreeMap<String, String>) -> Result<(), SecretError> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let bytes = serde_json::to_vec_pretty(map).map_err(|error| SecretError(error.to_string()))?;
        std::fs::write(&self.path, bytes).map_err(|error| SecretError(format!("could not write {}: {error}", self.path.display())))
    }
}

impl SecretStore for FileStore {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(self.read().get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut map = self.read();
        map.insert(account.to_string(), secret.to_string());
        self.write(&map)
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut map = self.read();
        if map.remove(account).is_some() {
            self.write(&map)?;
        }
        Ok(())
    }
}
