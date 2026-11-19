//! `toybox-server update`: fetch a signed release, check it, install it, restart, and go back if it
//! does not come up.
//!
//! The steps, in order, each of which can stop the update with nothing changed:
//! 1. read the manifest (`toybox-server.json`: `version`, `notes`, and per target a `url` and a
//!    `signature`), compare versions (no downgrade unless asked);
//! 2. pick the asset for this build's target;
//! 3. download it next to the installed binary and verify its minisign signature against the public key
//!    compiled into this program (the one the app's updater trusts);
//! 4. ask the new binary its version (it must be the manifest's) and run its `migrate --check` against
//!    the data folder, showing the report;
//! 5. keep the installed binary as `<name>.previous` and put the new one in its place (a rename);
//! 6. restart the systemd service if there is one, wait for `/api/v1/health`, and if it does not answer in
//!    time put `.previous` back, restart again, and say so.
//!
//! The side effects that cannot run in a test (a subprocess, systemd, a person typing) are behind the
//! [`Host`] trait; everything else (manifest, versions, files, signatures, HTTP) is real in the tests.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use semver::Version;
use serde::{Deserialize, Serialize};

/// The target triple this program was built for: which asset of a release is its own.
pub const TARGET: &str = env!("TOYBOX_TARGET");
/// The release page's manifest address the app is built from.
pub const DEFAULT_MANIFEST_URL: &str = env!("TOYBOX_UPDATE_URL");
/// What `--check` exits with when a newer release exists.
pub const EXIT_UPDATE_AVAILABLE: i32 = 10;
const MANIFEST_NAME: &str = "toybox-server.json";
const MANIFEST_LIMIT: u64 = 1024 * 1024;
const BINARY_LIMIT: u64 = 1024 * 1024 * 1024;

/// The public key releases are verified with (the base64 text Tauri uses). A release build has exactly
/// the compiled-in key; only a debug build can be pointed at another, for tests.
pub fn release_public_key() -> String {
    #[cfg(debug_assertions)]
    if let Ok(key) = std::env::var("TOYBOX_TEST_UPDATE_PUBKEY") {
        return key;
    }
    env!("TOYBOX_UPDATE_PUBKEY").to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Asset {
    pub url: String,
    /// The `.sig` file's content as Tauri writes it (base64 of the minisign signature).
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Target triple -> asset.
    #[serde(default)]
    pub targets: BTreeMap<String, Asset>,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Manifest, UpdateError> {
        let manifest: Manifest = serde_json::from_slice(bytes).map_err(|e| UpdateError::Manifest(format!("it is not a release manifest: {e}")))?;
        manifest.version()?;
        Ok(manifest)
    }
    pub fn version(&self) -> Result<Version, UpdateError> {
        Version::parse(self.version.trim().trim_start_matches('v')).map_err(|e| UpdateError::Manifest(format!("\"{}\" is not a version: {e}", self.version)))
    }
    pub fn asset_for(&self, target: &str) -> Option<&Asset> {
        self.targets.get(target)
    }
}

#[derive(Debug)]
pub enum UpdateError {
    Manifest(String),
    NoRelease { target: String, version: String, available: Vec<String> },
    Download(String),
    BadSignature(String),
    WrongVersion { expected: String, found: String },
    NoRights(PathBuf),
    Preflight(String),
    NeedsConfirmation,
    Declined,
    Io(String),
    /// The new version did not come up and the previous one was put back (and answers).
    RolledBack(String),
    /// The new version did not come up and putting the previous one back did not work either.
    RollbackFailed(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::Manifest(why) => write!(f, "Could not read the release information: {why}"),
            UpdateError::NoRelease { target, version, available } => write!(
                f,
                "Release {version} has no toybox-server for {target}{}. Nothing was changed.",
                if available.is_empty() { String::new() } else { format!(" (it has: {})", available.join(", ")) }
            ),
            UpdateError::Download(why) => write!(f, "Could not download the release: {why}"),
            UpdateError::BadSignature(why) => write!(
                f,
                "The downloaded file is NOT signed by the Toybox release key ({why}). It was discarded and nothing was changed."
            ),
            UpdateError::WrongVersion { expected, found } => write!(
                f,
                "The release says it is version {expected} but the program in it reports {found}. It was discarded and nothing was changed."
            ),
            UpdateError::NoRights(path) => write!(
                f,
                "No permission to replace {}. Run the update with enough rights, for example: sudo {} update",
                path.display(),
                path.display()
            ),
            UpdateError::Preflight(why) => write!(f, "The new version cannot open this data folder: {why}\nNothing was changed."),
            UpdateError::NeedsConfirmation => f.write_str("Not asked: there is no terminal to answer on. Run it again with --yes to update."),
            UpdateError::Declined => f.write_str("Not updated."),
            UpdateError::Io(why) => write!(f, "{why}"),
            UpdateError::RolledBack(why) => f.write_str(why),
            UpdateError::RollbackFailed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for UpdateError {}

fn io_error(what: &str, path: &Path, error: std::io::Error) -> UpdateError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        return UpdateError::NoRights(path.to_path_buf());
    }
    UpdateError::Io(format!("{what} {}: {error}", path.display()))
}

// ----------------------------------------------------------------- where releases are

/// Where a manifest or an asset is: a web address or a path.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Url(String),
    Path(PathBuf),
}

impl Place {
    pub fn parse(text: &str) -> Place {
        let text = text.trim();
        if text.starts_with("http://") || text.starts_with("https://") {
            Place::Url(text.to_string())
        } else {
            Place::Path(PathBuf::from(text))
        }
    }

    /// The manifest of a release source: a folder holds `toybox-server.json`.
    pub fn manifest(&self) -> Place {
        match self {
            Place::Path(path) if path.is_dir() => Place::Path(path.join(MANIFEST_NAME)),
            other => other.clone(),
        }
    }

    /// An asset address from the manifest, which may be relative to the manifest.
    pub fn resolve(&self, reference: &str) -> Place {
        let reference = reference.trim();
        if reference.starts_with("http://") || reference.starts_with("https://") {
            return Place::Url(reference.to_string());
        }
        match self {
            Place::Url(base) => {
                let folder = base.rsplit_once('/').map(|(folder, _)| folder).unwrap_or(base);
                Place::Url(format!("{folder}/{}", reference.trim_start_matches("./")))
            }
            Place::Path(base) => {
                let given = PathBuf::from(reference);
                if given.is_absolute() {
                    Place::Path(given)
                } else {
                    Place::Path(base.parent().unwrap_or(Path::new(".")).join(given))
                }
            }
        }
    }

    fn describe(&self) -> String {
        match self {
            Place::Url(url) => url.clone(),
            Place::Path(path) => path.display().to_string(),
        }
    }

    fn read(&self, limit: u64) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        self.open(limit)?.read_to_end(&mut bytes).map_err(|e| format!("reading {}: {e}", self.describe()))?;
        Ok(bytes)
    }

    fn open(&self, limit: u64) -> Result<Box<dyn Read>, String> {
        match self {
            Place::Path(path) => Ok(Box::new(std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?)),
            Place::Url(url) => {
                let agent: ureq::Agent = ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(900)))
                    .user_agent(format!("toybox-server/{}", crate::SERVER_VERSION))
                    .build()
                    .into();
                let response = agent.get(url).call().map_err(|e| match e {
                    ureq::Error::StatusCode(code) => format!("{url} answered {code}"),
                    other => format!("{url}: {other}"),
                })?;
                let body = response.into_body();
                Ok(Box::new(body.into_with_config().limit(limit).reader()))
            }
        }
    }
}

// ----------------------------------------------------------------- signatures

/// Check `data` against a minisign signature. Both `public_key` and `signature` are the base64 texts
/// Tauri writes (the key as in `tauri.conf.json`, the signature as in the `.sig` file).
pub fn verify(public_key: &str, signature: &str, data: &[u8]) -> Result<(), String> {
    let text = |encoded: &str| -> Result<String, String> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).map_err(|e| format!("not base64: {e}"))?;
        String::from_utf8(bytes).map_err(|_| "not text".to_string())
    };
    let key = minisign_verify::PublicKey::decode(&text(public_key).map_err(|e| format!("public key: {e}"))?).map_err(|e| format!("public key: {e}"))?;
    let signature = minisign_verify::Signature::decode(&text(signature).map_err(|e| format!("signature: {e}"))?).map_err(|e| format!("signature: {e}"))?;
    key.verify(data, &signature, true).map_err(|e| e.to_string())
}

// ----------------------------------------------------------------- side effects

/// How the service came back (or did not).
#[derive(Debug, PartialEq)]
pub enum Restart {
    Restarted,
    /// Nothing manages the server here; the person restarts it.
    NotManaged(String),
    Failed(String),
}

/// What an update does outside the files: ask, run programs, restart, probe.
pub trait Host {
    fn say(&self, line: &str);
    /// Ask a yes/no question; `None` when there is nobody to ask.
    fn confirm(&self, question: &str) -> Option<bool>;
    /// The version the downloaded program reports.
    fn program_version(&self, program: &Path) -> Result<String, String>;
    /// The downloaded program's `migrate --check` against the data folder: the report to show.
    fn migrate_check(&self, program: &Path, data: &Path) -> Result<String, String>;
    fn restart(&self, service: &str) -> Restart;
    /// Whether the server answers its health check.
    fn healthy(&self, listen: &str) -> bool;
}

/// The real thing.
pub struct System;

impl Host for System {
    fn say(&self, line: &str) {
        println!("{line}");
    }

    fn confirm(&self, question: &str) -> Option<bool> {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            return None;
        }
        print!("{question} [y/N] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok()?;
        Some(matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
    }

    fn program_version(&self, program: &Path) -> Result<String, String> {
        let output = std::process::Command::new(program).arg("--version").output().map_err(|e| format!("could not run it: {e}"))?;
        if !output.status.success() {
            return Err(format!("it exited with {}", output.status));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        text.split_whitespace().last().map(str::to_string).ok_or_else(|| "it printed nothing".to_string())
    }

    fn migrate_check(&self, program: &Path, data: &Path) -> Result<String, String> {
        let output = std::process::Command::new(program)
            .args(["migrate", "--check", "--data"])
            .arg(data)
            .output()
            .map_err(|e| format!("could not run it: {e}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("{}{}", stdout, stderr).trim().to_string());
        }
        Ok(stdout)
    }

    fn restart(&self, service: &str) -> Restart {
        let active = std::process::Command::new("systemctl").args(["is-active", "--quiet", service]).status();
        match active {
            Err(_) => return Restart::NotManaged("systemd is not available here".into()),
            Ok(status) if !status.success() => return Restart::NotManaged(format!("the service \"{service}\" is not running under systemd")),
            Ok(_) => {}
        }
        match std::process::Command::new("systemctl").args(["restart", service]).output() {
            Ok(output) if output.status.success() => Restart::Restarted,
            Ok(output) => Restart::Failed(format!("systemctl restart {service}: {}", String::from_utf8_lossy(&output.stderr).trim())),
            Err(e) => Restart::Failed(format!("systemctl: {e}")),
        }
    }

    fn healthy(&self, listen: &str) -> bool {
        let address = listen.replace("0.0.0.0", "127.0.0.1").replace("[::]", "[::1]");
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(3))).build().into();
        match agent.get(format!("http://{address}/api/v1/health")).call() {
            Ok(mut response) => response.body_mut().read_to_string().map(|body| body.contains("\"ok\"")).unwrap_or(false),
            Err(_) => false,
        }
    }
}

// ----------------------------------------------------------------- the update

pub struct Options {
    /// Manifest address or folder; unset: `server.json`'s `updateUrl`, then the built-in release page.
    pub from: Option<String>,
    pub check: bool,
    pub yes: bool,
    pub allow_downgrade: bool,
    pub data: PathBuf,
    /// The installed program (what is replaced).
    pub binary: PathBuf,
    pub service: String,
    pub listen: String,
    /// How long the restarted server has to answer.
    pub wait: Duration,
    pub public_key: String,
    pub target: String,
    pub current_version: String,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    UpToDate { current: String, latest: String },
    /// `--check`: a newer release exists.
    Available { current: String, latest: String, notes: Option<String> },
    Updated { from: String, to: String, restarted: bool },
}

/// Download, check and install. See the module notes for the order.
pub fn run(options: &Options, host: &dyn Host) -> Result<Outcome, UpdateError> {
    let source = Place::parse(options.from.as_deref().unwrap_or(DEFAULT_MANIFEST_URL));
    if options.from.is_none() && DEFAULT_MANIFEST_URL.is_empty() {
        return Err(UpdateError::Manifest("this build knows no release page; pass --from <manifest address or folder>".into()));
    }
    let manifest_place = source.manifest();
    let bytes = manifest_place.read(MANIFEST_LIMIT).map_err(|e| UpdateError::Manifest(format!("{e} (looked at {})", manifest_place.describe())))?;
    let manifest = Manifest::parse(&bytes)?;
    let latest = manifest.version()?;
    let current = Version::parse(&options.current_version).map_err(|e| UpdateError::Io(format!("this program's version is not valid: {e}")))?;
    let (current_text, latest_text) = (current.to_string(), latest.to_string());
    if latest == current {
        host.say(&format!("toybox-server {current_text} is up to date."));
        return Ok(Outcome::UpToDate { current: current_text, latest: latest_text });
    }
    if latest < current && !options.allow_downgrade {
        host.say(&format!("toybox-server {current_text} is newer than the latest release ({latest_text}); nothing to do. (--allow-downgrade installs the older release.)"));
        return Ok(Outcome::UpToDate { current: current_text, latest: latest_text });
    }
    let downgrade = latest < current;
    host.say(&format!("{} {latest_text} is available (this is {current_text}).", if downgrade { "Older release" } else { "Release" }));
    if let Some(notes) = manifest.notes.as_deref().filter(|n| !n.trim().is_empty()) {
        host.say(notes.trim());
    }
    if options.check {
        return Ok(Outcome::Available { current: current_text, latest: latest_text, notes: manifest.notes });
    }
    let asset = manifest.asset_for(&options.target).ok_or_else(|| UpdateError::NoRelease {
        target: options.target.clone(),
        version: latest_text.clone(),
        available: manifest.targets.keys().cloned().collect(),
    })?;
    if !options.yes {
        match host.confirm(&format!("Update toybox-server {current_text} -> {latest_text} and restart it?")) {
            Some(true) => {}
            Some(false) => return Err(UpdateError::Declined),
            None => return Err(UpdateError::NeedsConfirmation),
        }
    }

    let folder = options.binary.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let name = options.binary.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "toybox-server".into());
    let staged = folder.join(format!(".{name}.new"));
    let previous = folder.join(format!("{name}.previous"));
    let _ = std::fs::remove_file(&staged);
    let outcome = install(options, host, asset, &source.manifest(), &staged, &previous, &latest_text, &current_text);
    if outcome.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    outcome
}

#[allow(clippy::too_many_arguments)]
fn install(
    options: &Options,
    host: &dyn Host,
    asset: &Asset,
    manifest_place: &Place,
    staged: &Path,
    previous: &Path,
    latest: &str,
    current: &str,
) -> Result<Outcome, UpdateError> {
    let place = manifest_place.resolve(&asset.url);
    // Stage the download beside the installed program so the final step is a rename. This also is where
    // missing rights show, before anything is downloaded.
    let mut file = std::fs::File::create(staged).map_err(|e| io_error("Cannot write", staged, e))?;
    host.say(&format!("Downloading {} ...", place.describe()));
    let mut reader = place.open(BINARY_LIMIT).map_err(UpdateError::Download)?;
    let started = Instant::now();
    let size = std::io::copy(&mut reader, &mut file).map_err(|e| UpdateError::Download(e.to_string()))?;
    file.flush().map_err(|e| UpdateError::Io(format!("Cannot write {}: {e}", staged.display())))?;
    // Closed before the program is run (a program that is open for writing cannot start on Windows).
    drop(file);
    host.say(&format!("Downloaded {:.1} MB in {:.1}s.", size as f64 / 1_048_576.0, started.elapsed().as_secs_f64()));

    let data = std::fs::read(staged).map_err(|e| io_error("Cannot read", staged, e))?;
    verify(&options.public_key, &asset.signature, &data).map_err(UpdateError::BadSignature)?;
    host.say("Signature is valid.");
    drop(data);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| io_error("Cannot change", staged, e))?;
    }
    let found = host.program_version(staged).map_err(|why| UpdateError::Preflight(format!("it does not run: {why}")))?;
    if found.trim_start_matches('v') != latest {
        return Err(UpdateError::WrongVersion { expected: latest.to_string(), found });
    }
    let report = host.migrate_check(staged, &options.data).map_err(UpdateError::Preflight)?;
    host.say("What the new version would do with the library:");
    for line in report.lines() {
        host.say(&format!("  {line}"));
    }

    // Keep what runs now, then put the new program in its place.
    let _ = std::fs::remove_file(previous);
    if std::fs::hard_link(&options.binary, previous).is_err() {
        std::fs::copy(&options.binary, previous).map_err(|e| io_error("Cannot keep a copy as", previous, e))?;
    }
    std::fs::rename(staged, &options.binary).map_err(|e| io_error("Cannot replace", &options.binary, e))?;
    host.say(&format!("Installed {latest}. The previous program is kept as {}.", previous.display()));

    match host.restart(&options.service) {
        Restart::NotManaged(why) => {
            host.say(&format!("Not restarted ({why}). Restart toybox-server yourself to use {latest}."));
            Ok(Outcome::Updated { from: current.to_string(), to: latest.to_string(), restarted: false })
        }
        Restart::Failed(why) => {
            host.say(&format!("The service did not restart: {why}"));
            Err(rollback(options, host, previous, &format!("Restarting the service failed ({why})")))
        }
        Restart::Restarted => {
            host.say(&format!("Restarted {}. Waiting for it to answer on {} ...", options.service, options.listen));
            if wait_healthy(options, host) {
                host.say(&format!("toybox-server {latest} is running."));
                Ok(Outcome::Updated { from: current.to_string(), to: latest.to_string(), restarted: true })
            } else {
                Err(rollback(options, host, previous, &format!("The new version did not answer within {} seconds", options.wait.as_secs())))
            }
        }
    }
}

fn wait_healthy(options: &Options, host: &dyn Host) -> bool {
    let started = Instant::now();
    loop {
        if host.healthy(&options.listen) {
            return true;
        }
        if started.elapsed() >= options.wait {
            return false;
        }
        std::thread::sleep(Duration::from_millis(500).min(options.wait));
    }
}

/// Put the previous program back and start it.
fn rollback(options: &Options, host: &dyn Host, previous: &Path, cause: &str) -> UpdateError {
    host.say(&format!("{cause}. Putting the previous version back ..."));
    if let Err(e) = std::fs::rename(previous, &options.binary) {
        return UpdateError::RollbackFailed(format!(
            "{cause}, and the previous version could not be put back: {e}. It is still at {}; copy it over {} by hand.",
            previous.display(),
            options.binary.display()
        ));
    }
    let hint = "If the older version refuses the library because the new one already migrated it, restore the snapshot: toybox-server snapshot list, then toybox-server snapshot restore <name> (see docs/server.md).";
    match host.restart(&options.service) {
        Restart::Restarted if wait_healthy(options, host) => UpdateError::RolledBack(format!("{cause}. The previous version is back and running; the update did not happen.")),
        Restart::Restarted => UpdateError::RollbackFailed(format!("{cause}. The previous version was put back but does not answer either. Check: journalctl -u {}. {hint}", options.service)),
        other => UpdateError::RollbackFailed(format!("{cause}. The previous version was put back; starting it failed ({other:?}). {hint}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_parses_and_picks_its_target() {
        let manifest = Manifest::parse(
            br#"{"version":"v1.2.3","notes":"Faster.","targets":{"x86_64-unknown-linux-gnu":{"url":"toybox-server-x86_64-unknown-linux-gnu","signature":"c2ln"}}}"#,
        )
        .unwrap();
        assert_eq!(manifest.version().unwrap(), Version::new(1, 2, 3));
        assert_eq!(manifest.asset_for("x86_64-unknown-linux-gnu").unwrap().signature, "c2ln");
        assert!(manifest.asset_for("aarch64-apple-darwin").is_none());
        assert!(Manifest::parse(b"{}").is_err());
        assert!(Manifest::parse(br#"{"version":"soon"}"#).is_err());
        assert!(Manifest::parse(b"<html>").is_err());
    }

    #[test]
    fn addresses_resolve_against_the_manifest() {
        let url = Place::parse("https://example.com/releases/latest/download/toybox-server.json");
        assert_eq!(url.resolve("toybox-server-x"), Place::Url("https://example.com/releases/latest/download/toybox-server-x".into()));
        assert_eq!(url.resolve("https://cdn.example.com/a"), Place::Url("https://cdn.example.com/a".into()));
        let path = Place::parse("/srv/releases/toybox-server.json");
        assert_eq!(path.resolve("a/b"), Place::Path(PathBuf::from("/srv/releases").join("a/b")));
    }
}
