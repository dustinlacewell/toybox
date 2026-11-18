//! `server.json`: the server's name and its access tokens. Only the SHA-256 of
//! a token is stored. The file is written atomically, and a running server
//! re-reads it when it changes, so `toybox-server token ...` takes effect
//! without a restart.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::timeutil::now_secs;

pub const TOKEN_PREFIX: &str = "tbx_";

/// What a token may do. Ordered: each role includes the ones before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Browse and download.
    Read,
    /// Also import, edit and delete.
    Write,
    /// Also manage tokens.
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Read => "read",
            Role::Write => "write",
            Role::Admin => "admin",
        }
    }
    pub fn parse(text: &str) -> Option<Role> {
        match text.trim().to_ascii_lowercase().as_str() {
            "read" => Some(Role::Read),
            "write" => Some(Role::Write),
            "admin" => Some(Role::Admin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenRecord {
    pub id: String,
    pub name: String,
    pub role: Role,
    /// Lower-case hex SHA-256 of the full token text.
    pub sha256: String,
    /// Seconds since the Unix epoch.
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    #[serde(default = "one")]
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub tokens: Vec<TokenRecord>,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidName(String),
    DuplicateName(String),
    NotFound(String),
    LastAdmin,
    Io(String),
    Parse(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidName(why) => f.write_str(why),
            ConfigError::DuplicateName(name) => write!(f, "A token named \"{name}\" already exists."),
            ConfigError::NotFound(key) => write!(f, "No token matches \"{key}\"."),
            ConfigError::LastAdmin => f.write_str(
                "That is the last admin token. Create another admin token first, so you cannot lock yourself out.",
            ),
            ConfigError::Io(why) => write!(f, "Could not read or write the server settings: {why}"),
            ConfigError::Parse(why) => write!(f, "The server settings file is not valid: {why}"),
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn hash_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// `tbx_` followed by 32 random bytes, base64url without padding.
pub fn new_token_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    format!("{TOKEN_PREFIX}{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

pub fn random_hex(len_bytes: usize) -> String {
    let mut bytes = vec![0u8; len_bytes];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex(&bytes)
}

fn validate_name(name: &str) -> Result<String, ConfigError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ConfigError::InvalidName("A token needs a name.".into()));
    }
    if name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(ConfigError::InvalidName(
            "A token name can be up to 64 characters and may not contain control characters.".into(),
        ));
    }
    Ok(name.to_string())
}

impl ServerConfig {
    pub fn new(name: &str) -> ServerConfig {
        ServerConfig { version: 1, name: name.trim().to_string(), tokens: Vec::new() }
    }

    /// Create a token. Returns its record and the secret, which is not stored
    /// anywhere and cannot be shown again.
    pub fn add_token(&mut self, name: &str, role: Role) -> Result<(TokenRecord, String), ConfigError> {
        let name = validate_name(name)?;
        if self.tokens.iter().any(|t| t.name.eq_ignore_ascii_case(&name)) {
            return Err(ConfigError::DuplicateName(name));
        }
        let secret = new_token_secret();
        let mut id = random_hex(4);
        while self.tokens.iter().any(|t| t.id == id) {
            id = random_hex(4);
        }
        let record = TokenRecord { id, name, role, sha256: hash_token(&secret), created_at: now_secs(), last_used_at: None };
        self.tokens.push(record.clone());
        Ok((record, secret))
    }

    /// Remove a token by id or by name. The last admin token cannot be removed.
    pub fn remove_token(&mut self, key: &str) -> Result<TokenRecord, ConfigError> {
        let key = key.trim();
        let index = self
            .tokens
            .iter()
            .position(|t| t.id == key)
            .or_else(|| self.tokens.iter().position(|t| t.name.eq_ignore_ascii_case(key)))
            .ok_or_else(|| ConfigError::NotFound(key.to_string()))?;
        if self.tokens[index].role == Role::Admin && self.tokens.iter().filter(|t| t.role == Role::Admin).count() == 1 {
            return Err(ConfigError::LastAdmin);
        }
        Ok(self.tokens.remove(index))
    }

    /// Find the token a presented secret belongs to. Every stored hash is
    /// compared, in constant time, so timing does not reveal how close a guess was.
    pub fn authenticate(&self, secret: &str) -> Option<&TokenRecord> {
        let presented = hash_token(secret);
        let mut found = None;
        for token in &self.tokens {
            if bool::from(token.sha256.as_bytes().ct_eq(presented.as_bytes())) {
                found = Some(token);
            }
        }
        found
    }

    pub fn load(path: &Path) -> Result<ServerConfig, ConfigError> {
        let bytes = std::fs::read(path).map_err(|e| ConfigError::Io(format!("{}: {e}", path.display())))?;
        serde_json::from_slice(&bytes).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Write atomically: a temporary file in the same folder, then a rename,
    /// so a reader never sees half a file. Readable by its owner only.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = serde_json::to_vec_pretty(self).map_err(|e| ConfigError::Parse(e.to_string()))?;
        write_atomic(path, &text).map_err(|e| ConfigError::Io(format!("{}: {e}", path.display())))
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let temp = dir.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id(),
        random_hex(4)
    ));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // Run as root by mistake, the command line must not hand the file to
        // root and lock the service out: keep the owner of the file it replaces.
        #[cfg(unix)]
        {
            if let Ok(previous) = std::fs::metadata(path) {
                use std::os::unix::fs::MetadataExt;
                let _ = std::os::unix::fs::chown(&temp, Some(previous.uid()), Some(previous.gid()));
            }
        }
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp { modified: meta.modified().ok(), len: meta.len() })
}

struct Cache {
    config: Result<Arc<ServerConfig>, ConfigError>,
    stamp: Option<Stamp>,
    read_at: Instant,
    /// Last use per token id, recorded in memory and written back every so often.
    used: HashMap<String, u64>,
    flushed: HashMap<String, u64>,
}

/// The server's live view of `server.json`.
pub struct ConfigStore {
    path: PathBuf,
    cache: Mutex<Cache>,
}

/// The file is re-read at least this often even when its timestamp did not
/// change, in case a very fast edit left the same timestamp and length.
const FORCE_REREAD: std::time::Duration = std::time::Duration::from_secs(2);

impl ConfigStore {
    pub fn open(path: &Path) -> Result<ConfigStore, ConfigError> {
        let config = ServerConfig::load(path)?;
        Ok(ConfigStore {
            path: path.to_path_buf(),
            cache: Mutex::new(Cache {
                config: Ok(Arc::new(config)),
                stamp: stamp(path),
                read_at: Instant::now(),
                used: HashMap::new(),
                flushed: HashMap::new(),
            }),
        })
    }

    fn refresh(&self, cache: &mut Cache) {
        let now = stamp(&self.path);
        if now != cache.stamp || cache.read_at.elapsed() > FORCE_REREAD || cache.config.is_err() {
            cache.config = ServerConfig::load(&self.path).map(Arc::new);
            cache.stamp = now;
            cache.read_at = Instant::now();
        }
    }

    /// The current settings. An unreadable file is an error rather than a
    /// stale copy, so a revoked token can never keep working because of a typo.
    pub fn snapshot(&self) -> Result<Arc<ServerConfig>, ConfigError> {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        self.refresh(&mut cache);
        cache.config.clone()
    }

    /// The tokens with the freshest known "last used" time.
    pub fn tokens_with_usage(&self) -> Result<Vec<TokenRecord>, ConfigError> {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        self.refresh(&mut cache);
        let config = cache.config.clone()?;
        Ok(config
            .tokens
            .iter()
            .map(|t| {
                let mut t = t.clone();
                if let Some(used) = cache.used.get(&t.id) {
                    t.last_used_at = Some(t.last_used_at.map_or(*used, |old| old.max(*used)));
                }
                t
            })
            .collect())
    }

    pub fn touch(&self, id: &str) {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        cache.used.insert(id.to_string(), now_secs());
    }

    /// Change the settings: re-read the file, apply `change`, write it back.
    /// Reading first keeps edits made by the command line.
    pub fn mutate<R>(&self, change: impl FnOnce(&mut ServerConfig) -> Result<R, ConfigError>) -> Result<R, ConfigError> {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let mut config = ServerConfig::load(&self.path)?;
        let result = change(&mut config)?;
        config.save(&self.path)?;
        cache.config = Ok(Arc::new(config));
        cache.stamp = stamp(&self.path);
        cache.read_at = Instant::now();
        Ok(result)
    }

    /// Write "last used" times to disk if any changed since the last write.
    pub fn flush_usage(&self) {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        if cache.used == cache.flushed {
            return;
        }
        let Ok(mut config) = ServerConfig::load(&self.path) else { return };
        for token in &mut config.tokens {
            if let Some(used) = cache.used.get(&token.id) {
                token.last_used_at = Some(token.last_used_at.map_or(*used, |old| old.max(*used)));
            }
        }
        if config.save(&self.path).is_ok() {
            cache.flushed = cache.used.clone();
            cache.config = Ok(Arc::new(config));
            cache.stamp = stamp(&self.path);
            cache.read_at = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("toybox-config-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("server.json")
    }

    #[test]
    fn roles_are_ordered() {
        assert!(Role::Read < Role::Write && Role::Write < Role::Admin);
        assert_eq!(Role::parse(" Admin "), Some(Role::Admin));
        assert_eq!(Role::parse("root"), None);
    }

    #[test]
    fn tokens_have_the_documented_shape_and_only_the_hash_is_stored() {
        let mut config = ServerConfig::new("Test");
        let (record, secret) = config.add_token("alice", Role::Read).unwrap();
        assert!(secret.starts_with("tbx_") && secret.len() == 4 + 43, "{secret}");
        assert!(secret[4..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(record.sha256, hash_token(&secret));
        let stored = serde_json::to_string(&config).unwrap();
        assert!(!stored.contains(&secret));
        assert_eq!(config.authenticate(&secret).unwrap().name, "alice");
        assert!(config.authenticate("tbx_nope").is_none());
        assert!(config.authenticate("").is_none());
    }

    #[test]
    fn names_are_validated_and_unique() {
        let mut config = ServerConfig::new("Test");
        config.add_token("alice", Role::Read).unwrap();
        assert!(matches!(config.add_token("ALICE", Role::Write), Err(ConfigError::DuplicateName(_))));
        assert!(matches!(config.add_token("  ", Role::Write), Err(ConfigError::InvalidName(_))));
        assert!(matches!(config.add_token("bad\nname", Role::Write), Err(ConfigError::InvalidName(_))));
        assert!(matches!(config.add_token(&"n".repeat(65), Role::Write), Err(ConfigError::InvalidName(_))));
    }

    #[test]
    fn the_last_admin_cannot_be_removed() {
        let mut config = ServerConfig::new("Test");
        let (admin, _) = config.add_token("admin", Role::Admin).unwrap();
        config.add_token("reader", Role::Read).unwrap();
        assert!(matches!(config.remove_token("admin"), Err(ConfigError::LastAdmin)));
        let (second, _) = config.add_token("second", Role::Admin).unwrap();
        assert_eq!(config.remove_token(&admin.id).unwrap().name, "admin");
        assert!(matches!(config.remove_token(&second.id), Err(ConfigError::LastAdmin)));
        assert_eq!(config.remove_token("READER").unwrap().name, "reader");
        assert!(matches!(config.remove_token("ghost"), Err(ConfigError::NotFound(_))));
    }

    #[test]
    fn saves_atomically_and_a_store_notices_outside_edits() {
        let path = temp_file("store");
        let mut config = ServerConfig::new("Test");
        let (_, admin_secret) = config.add_token("admin", Role::Admin).unwrap();
        config.save(&path).unwrap();
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "no temporary file left behind");

        let store = ConfigStore::open(&path).unwrap();
        assert!(store.snapshot().unwrap().authenticate(&admin_secret).is_some());

        // Another process (the command line) adds, then removes, a token.
        let mut outside = ServerConfig::load(&path).unwrap();
        let (_, extra) = outside.add_token("extra", Role::Write).unwrap();
        outside.save(&path).unwrap();
        assert!(store.snapshot().unwrap().authenticate(&extra).is_some());
        let mut outside = ServerConfig::load(&path).unwrap();
        outside.remove_token("extra").unwrap();
        outside.save(&path).unwrap();
        assert!(store.snapshot().unwrap().authenticate(&extra).is_none());

        // A broken file fails closed instead of keeping the old tokens.
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(store.snapshot().is_err());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn usage_is_kept_in_memory_and_flushed_without_losing_outside_edits() {
        let path = temp_file("usage");
        let mut config = ServerConfig::new("Test");
        let (admin, _) = config.add_token("admin", Role::Admin).unwrap();
        config.save(&path).unwrap();
        let store = ConfigStore::open(&path).unwrap();
        store.touch(&admin.id);
        assert!(store.tokens_with_usage().unwrap()[0].last_used_at.is_some());
        assert!(ServerConfig::load(&path).unwrap().tokens[0].last_used_at.is_none());
        let mut outside = ServerConfig::load(&path).unwrap();
        outside.add_token("friend", Role::Read).unwrap();
        outside.save(&path).unwrap();
        store.flush_usage();
        let on_disk = ServerConfig::load(&path).unwrap();
        assert_eq!(on_disk.tokens.len(), 2, "the outside edit survived");
        assert!(on_disk.tokens[0].last_used_at.is_some());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
