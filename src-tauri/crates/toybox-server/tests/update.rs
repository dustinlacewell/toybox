//! `toybox-server update` against a local HTTP fixture, with signatures from keys generated here.
//! Nothing touches the network or a real service: the side effects (restart, health, running the new
//! program) are a fake `Host` whose health depends on which program is installed.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};
use toybox_server::update::{self, Asset, Host, Manifest, Options, Outcome, Restart, UpdateError};

static COUNTER: AtomicU32 = AtomicU32::new(0);
const TARGET: &str = "x86_64-unknown-linux-gnu";

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A minisign key pair (the legacy "Ed" scheme, which the verifier accepts) in the base64 texts Tauri uses.
struct Keys {
    pair: Ed25519KeyPair,
    id: [u8; 8],
    public: String,
}

impl Keys {
    fn new() -> Keys {
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let mut id = [0u8; 8];
        ring::rand::SecureRandom::fill(&rng, &mut id).unwrap();
        let mut key = b"Ed".to_vec();
        key.extend_from_slice(&id);
        key.extend_from_slice(pair.public_key().as_ref());
        let public = b64(format!("untrusted comment: test key\n{}\n", b64(&key)).as_bytes());
        Keys { pair, id, public }
    }

    fn sign(&self, data: &[u8]) -> String {
        let signature = self.pair.sign(data);
        let mut body = b"Ed".to_vec();
        body.extend_from_slice(&self.id);
        body.extend_from_slice(signature.as_ref());
        let trusted = "test";
        let mut global = signature.as_ref().to_vec();
        global.extend_from_slice(trusted.as_bytes());
        let global = self.pair.sign(&global);
        b64(format!("untrusted comment: signature\n{}\ntrusted comment: {trusted}\n{}\n", b64(&body), b64(global.as_ref())).as_bytes())
    }
}

/// Serves fixed files over HTTP on a free local port.
fn serve(files: HashMap<String, Vec<u8>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0u8; 4096];
            let n = stream.read(&mut request).unwrap_or(0);
            let text = String::from_utf8_lossy(&request[..n]).to_string();
            let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
            match files.get(&path) {
                Some(body) => {
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = stream.write_all(body);
                }
                None => {
                    let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
            }
        }
    });
    format!("http://{address}")
}

struct Dirs {
    root: PathBuf,
}

impl Dirs {
    fn new(name: &str) -> Dirs {
        let root = std::env::temp_dir().join(format!("toybox-update-{name}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(root.join("bin/toybox-server"), b"old program").unwrap();
        Dirs { root }
    }
    fn binary(&self) -> PathBuf {
        self.root.join("bin/toybox-server")
    }
    fn installed(&self) -> Vec<u8> {
        std::fs::read(self.binary()).unwrap()
    }
    fn files_in_bin(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.root.join("bin")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        names
    }
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// What the new program is, what the world does when it is installed.
struct Fake {
    binary: PathBuf,
    /// The program content for which the health check answers.
    healthy_when: Vec<u8>,
    reports_version: String,
    check: Result<String, String>,
    answer: Option<bool>,
    managed: bool,
    said: Mutex<Vec<String>>,
    restarts: AtomicUsize,
}

impl Fake {
    fn new(dirs: &Dirs, healthy_when: &[u8], reports: &str) -> Fake {
        Fake {
            binary: dirs.binary(),
            healthy_when: healthy_when.to_vec(),
            reports_version: reports.into(),
            check: Ok("Pending step 5 (shared-sources): store each kept source file once".into()),
            answer: Some(true),
            managed: true,
            said: Mutex::new(Vec::new()),
            restarts: AtomicUsize::new(0),
        }
    }
    fn said(&self) -> String {
        self.said.lock().unwrap().join("\n")
    }
}

impl Host for Fake {
    fn say(&self, line: &str) {
        self.said.lock().unwrap().push(line.to_string());
    }
    fn confirm(&self, _: &str) -> Option<bool> {
        self.answer
    }
    fn program_version(&self, _: &Path) -> Result<String, String> {
        Ok(self.reports_version.clone())
    }
    fn migrate_check(&self, _: &Path, _: &Path) -> Result<String, String> {
        self.check.clone()
    }
    fn restart(&self, _: &str) -> Restart {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        if self.managed {
            Restart::Restarted
        } else {
            Restart::NotManaged("no systemd".into())
        }
    }
    fn healthy(&self, _: &str) -> bool {
        std::fs::read(&self.binary).map(|content| content == self.healthy_when).unwrap_or(false)
    }
}

fn options(dirs: &Dirs, from: &str, keys: &Keys) -> Options {
    Options {
        from: Some(from.to_string()),
        check: false,
        yes: true,
        allow_downgrade: false,
        data: dirs.root.join("data"),
        binary: dirs.binary(),
        service: "toybox-server".into(),
        listen: "127.0.0.1:8787".into(),
        wait: Duration::from_millis(600),
        public_key: keys.public.clone(),
        target: TARGET.into(),
        current_version: "1.0.0".into(),
    }
}

fn manifest(version: &str, targets: &[(&str, &str, &str)]) -> Vec<u8> {
    let manifest = Manifest {
        version: version.into(),
        notes: Some("Faster imports.".into()),
        targets: targets.iter().map(|(target, url, signature)| (target.to_string(), Asset { url: url.to_string(), signature: signature.to_string() })).collect(),
    };
    serde_json::to_vec(&manifest).unwrap()
}

/// A release of `program` signed by `keys`, served over HTTP; returns the manifest address.
fn release(version: &str, program: &[u8], keys: &Keys) -> String {
    let mut files = HashMap::new();
    files.insert(format!("/toybox-server-{TARGET}"), program.to_vec());
    files.insert("/toybox-server.json".to_string(), manifest(version, &[(TARGET, &format!("toybox-server-{TARGET}"), &keys.sign(program))]));
    format!("{}/toybox-server.json", serve(files))
}

#[test]
fn an_update_downloads_verifies_installs_and_restarts() {
    let dirs = Dirs::new("ok");
    let keys = Keys::new();
    let url = release("1.1.0", b"new program", &keys);
    let host = Fake::new(&dirs, b"new program", "1.1.0");
    let outcome = update::run(&options(&dirs, &url, &keys), &host).unwrap();
    assert_eq!(outcome, Outcome::Updated { from: "1.0.0".into(), to: "1.1.0".into(), restarted: true });
    assert_eq!(dirs.installed(), b"new program");
    assert_eq!(std::fs::read(dirs.root.join("bin/toybox-server.previous")).unwrap(), b"old program");
    assert_eq!(dirs.files_in_bin(), vec!["toybox-server", "toybox-server.previous"], "no staging file is left");
    assert_eq!(host.restarts.load(Ordering::SeqCst), 1);
    let said = host.said();
    assert!(said.contains("Signature is valid") && said.contains("Pending step 5") && said.contains("Faster imports."), "{said}");
}

#[test]
fn a_server_that_does_not_come_up_is_rolled_back_and_restarted() {
    let dirs = Dirs::new("rollback");
    let keys = Keys::new();
    let url = release("1.1.0", b"broken program", &keys);
    // Healthy only with the old program.
    let host = Fake::new(&dirs, b"old program", "1.1.0");
    let error = update::run(&options(&dirs, &url, &keys), &host).unwrap_err();
    assert!(matches!(error, UpdateError::RolledBack(_)), "{error}");
    assert!(error.to_string().contains("previous version is back and running"), "{error}");
    assert_eq!(dirs.installed(), b"old program");
    assert_eq!(host.restarts.load(Ordering::SeqCst), 2, "restarted for the update and again for the rollback");
    assert_eq!(dirs.files_in_bin(), vec!["toybox-server"]);
}

#[test]
fn a_rollback_that_does_not_come_up_either_says_where_things_are() {
    let dirs = Dirs::new("rollback-fails");
    let keys = Keys::new();
    let url = release("1.1.0", b"broken program", &keys);
    // Healthy for neither: the library is probably in a state the old program refuses.
    let host = Fake::new(&dirs, b"nothing", "1.1.0");
    let error = update::run(&options(&dirs, &url, &keys), &host).unwrap_err();
    assert!(matches!(error, UpdateError::RollbackFailed(_)));
    let text = error.to_string();
    assert!(text.contains("snapshot restore") && text.contains("journalctl"), "{text}");
    assert_eq!(dirs.installed(), b"old program");
}

#[test]
fn a_file_signed_by_another_key_or_changed_afterwards_is_refused_and_nothing_changes() {
    let keys = Keys::new();
    for (name, signer, served) in [("other-key", Keys::new(), b"new program".to_vec()), ("tampered", Keys::new(), b"tampered".to_vec())] {
        let dirs = Dirs::new(name);
        let signer = if name == "tampered" { &keys } else { &signer };
        // The manifest carries the signature of `new program`; the served file is `served`.
        let mut files = HashMap::new();
        files.insert(format!("/toybox-server-{TARGET}"), served);
        files.insert("/toybox-server.json".to_string(), manifest("1.1.0", &[(TARGET, &format!("toybox-server-{TARGET}"), &signer.sign(b"new program"))]));
        let url = format!("{}/toybox-server.json", serve(files));
        let host = Fake::new(&dirs, b"new program", "1.1.0");
        let error = update::run(&options(&dirs, &url, &keys), &host).unwrap_err();
        assert!(matches!(error, UpdateError::BadSignature(_)), "{name}: {error}");
        assert!(error.to_string().contains("NOT signed by the Toybox release key"));
        assert_eq!(dirs.installed(), b"old program");
        assert_eq!(dirs.files_in_bin(), vec!["toybox-server"], "{name}: the download was discarded");
        assert_eq!(host.restarts.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn garbage_where_a_signature_belongs_is_refused() {
    let dirs = Dirs::new("garbage");
    let keys = Keys::new();
    let mut files = HashMap::new();
    files.insert(format!("/toybox-server-{TARGET}"), b"new program".to_vec());
    files.insert("/toybox-server.json".to_string(), manifest("1.1.0", &[(TARGET, &format!("toybox-server-{TARGET}"), "bm90IGEgc2lnbmF0dXJl")]));
    let url = format!("{}/toybox-server.json", serve(files));
    let error = update::run(&options(&dirs, &url, &keys), &Fake::new(&dirs, b"x", "1.1.0")).unwrap_err();
    assert!(matches!(error, UpdateError::BadSignature(_)), "{error}");
    assert_eq!(dirs.installed(), b"old program");
}

#[test]
fn a_release_without_this_target_says_which_it_has() {
    let dirs = Dirs::new("target");
    let keys = Keys::new();
    let mut files = HashMap::new();
    files.insert("/toybox-server.json".to_string(), manifest("1.1.0", &[("aarch64-apple-darwin", "x", "y"), ("x86_64-pc-windows-msvc", "x", "y")]));
    let url = format!("{}/toybox-server.json", serve(files));
    let error = update::run(&options(&dirs, &url, &keys), &Fake::new(&dirs, b"x", "1.1.0")).unwrap_err();
    let text = error.to_string();
    assert!(matches!(error, UpdateError::NoRelease { .. }));
    assert!(text.contains(TARGET) && text.contains("aarch64-apple-darwin") && text.contains("Nothing was changed"), "{text}");
}

#[test]
fn the_asset_of_this_target_is_picked_among_several() {
    let dirs = Dirs::new("pick");
    let keys = Keys::new();
    let mut files = HashMap::new();
    files.insert("/linux".to_string(), b"new program".to_vec());
    files.insert("/mac".to_string(), b"mac program".to_vec());
    files.insert(
        "/toybox-server.json".to_string(),
        manifest("1.1.0", &[("aarch64-apple-darwin", "mac", &keys.sign(b"mac program")), (TARGET, "linux", &keys.sign(b"new program"))]),
    );
    let url = format!("{}/toybox-server.json", serve(files));
    update::run(&options(&dirs, &url, &keys), &Fake::new(&dirs, b"new program", "1.1.0")).unwrap();
    assert_eq!(dirs.installed(), b"new program");
}

#[test]
fn versions_decide_whether_there_is_anything_to_do() {
    let keys = Keys::new();
    let dirs = Dirs::new("versions");
    let same = release("1.0.0", b"new program", &keys);
    let host = Fake::new(&dirs, b"new program", "1.0.0");
    assert!(matches!(update::run(&options(&dirs, &same, &keys), &host).unwrap(), Outcome::UpToDate { .. }));
    assert_eq!(dirs.installed(), b"old program");
    // An older release is not installed by default ...
    let older = release("0.9.0", b"new program", &keys);
    assert!(matches!(update::run(&options(&dirs, &older, &keys), &Fake::new(&dirs, b"new program", "0.9.0")).unwrap(), Outcome::UpToDate { .. }));
    assert_eq!(dirs.installed(), b"old program");
    // ... unless asked.
    let mut allowed = options(&dirs, &older, &keys);
    allowed.allow_downgrade = true;
    update::run(&allowed, &Fake::new(&dirs, b"new program", "0.9.0")).unwrap();
    assert_eq!(dirs.installed(), b"new program");
    // 1.10.0 is newer than 1.9.0 (not a text comparison).
    let dirs = Dirs::new("versions-numeric");
    let mut ten = options(&dirs, &release("1.10.0", b"new program", &keys), &keys);
    ten.current_version = "1.9.0".into();
    ten.check = true;
    assert!(matches!(update::run(&ten, &Fake::new(&dirs, b"x", "1.10.0")).unwrap(), Outcome::Available { .. }));
}

#[test]
fn check_changes_nothing_and_reports_the_notes() {
    let dirs = Dirs::new("check");
    let keys = Keys::new();
    let url = release("1.1.0", b"new program", &keys);
    let mut check = options(&dirs, &url, &keys);
    check.check = true;
    let host = Fake::new(&dirs, b"new program", "1.1.0");
    match update::run(&check, &host).unwrap() {
        Outcome::Available { current, latest, notes } => assert_eq!((current.as_str(), latest.as_str(), notes.as_deref()), ("1.0.0", "1.1.0", Some("Faster imports."))),
        other => panic!("{other:?}"),
    }
    assert_eq!(dirs.files_in_bin(), vec!["toybox-server"]);
    assert_eq!(host.restarts.load(Ordering::SeqCst), 0);
    assert_eq!(update::EXIT_UPDATE_AVAILABLE, 10);
}

#[test]
fn nothing_is_installed_when_the_new_program_reports_another_version_or_cannot_read_the_library() {
    let keys = Keys::new();
    let dirs = Dirs::new("wrong-version");
    let url = release("1.1.0", b"new program", &keys);
    let error = update::run(&options(&dirs, &url, &keys), &Fake::new(&dirs, b"new program", "0.9.0")).unwrap_err();
    assert!(matches!(error, UpdateError::WrongVersion { .. }), "{error}");
    assert_eq!(dirs.installed(), b"old program");
    assert_eq!(dirs.files_in_bin(), vec!["toybox-server"]);

    let dirs = Dirs::new("preflight");
    let mut host = Fake::new(&dirs, b"new program", "1.1.0");
    host.check = Err("This library was migrated by a newer Toybox.".into());
    let error = update::run(&options(&dirs, &url, &keys), &host).unwrap_err();
    assert!(matches!(error, UpdateError::Preflight(_)) && error.to_string().contains("Nothing was changed"), "{error}");
    assert_eq!(dirs.installed(), b"old program");
}

#[test]
fn it_asks_first_unless_told_not_to_and_says_so_when_nobody_can_be_asked() {
    let keys = Keys::new();
    let dirs = Dirs::new("ask");
    let url = release("1.1.0", b"new program", &keys);
    let mut ask = options(&dirs, &url, &keys);
    ask.yes = false;
    let mut host = Fake::new(&dirs, b"new program", "1.1.0");
    host.answer = Some(false);
    assert!(matches!(update::run(&ask, &host).unwrap_err(), UpdateError::Declined));
    host.answer = None;
    assert!(matches!(update::run(&ask, &host).unwrap_err(), UpdateError::NeedsConfirmation));
    assert_eq!(dirs.installed(), b"old program");
    host.answer = Some(true);
    update::run(&ask, &host).unwrap();
    assert_eq!(dirs.installed(), b"new program");
}

#[test]
fn without_systemd_the_program_is_installed_and_the_person_is_told_to_restart() {
    let keys = Keys::new();
    let dirs = Dirs::new("unmanaged");
    let url = release("1.1.0", b"new program", &keys);
    let mut host = Fake::new(&dirs, b"nothing", "1.1.0");
    host.managed = false;
    let outcome = update::run(&options(&dirs, &url, &keys), &host).unwrap();
    assert_eq!(outcome, Outcome::Updated { from: "1.0.0".into(), to: "1.1.0".into(), restarted: false });
    assert!(host.said().contains("Restart toybox-server yourself"));
    assert_eq!(dirs.installed(), b"new program");
}

#[test]
fn a_local_folder_is_a_release_source() {
    let keys = Keys::new();
    let dirs = Dirs::new("folder");
    let folder = dirs.root.join("release");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(format!("toybox-server-{TARGET}")), b"new program").unwrap();
    std::fs::write(folder.join("toybox-server.json"), manifest("1.1.0", &[(TARGET, &format!("toybox-server-{TARGET}"), &keys.sign(b"new program"))])).unwrap();
    update::run(&options(&dirs, &folder.to_string_lossy(), &keys), &Fake::new(&dirs, b"new program", "1.1.0")).unwrap();
    assert_eq!(dirs.installed(), b"new program");
}

#[test]
fn a_missing_manifest_or_download_is_reported_plainly() {
    let keys = Keys::new();
    let dirs = Dirs::new("missing");
    let base = serve(HashMap::new());
    let error = update::run(&options(&dirs, &format!("{base}/toybox-server.json"), &keys), &Fake::new(&dirs, b"x", "1.1.0")).unwrap_err();
    assert!(matches!(error, UpdateError::Manifest(_)) && error.to_string().contains("404"), "{error}");
    let mut files = HashMap::new();
    files.insert("/toybox-server.json".to_string(), manifest("1.1.0", &[(TARGET, "gone", "x")]));
    let url = format!("{}/toybox-server.json", serve(files));
    let error = update::run(&options(&dirs, &url, &keys), &Fake::new(&dirs, b"x", "1.1.0")).unwrap_err();
    assert!(matches!(error, UpdateError::Download(_)), "{error}");
    assert_eq!(dirs.files_in_bin(), vec!["toybox-server"]);
}

#[test]
fn missing_rights_suggest_sudo() {
    let text = UpdateError::NoRights(PathBuf::from("/usr/local/bin/toybox-server")).to_string();
    assert!(text.contains("sudo") && text.contains("/usr/local/bin/toybox-server"), "{text}");
}

#[test]
fn the_compiled_in_key_is_the_one_the_app_updater_trusts() {
    let conf: serde_json::Value = serde_json::from_slice(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tauri.conf.json")).unwrap()).unwrap();
    assert_eq!(update::release_public_key(), conf["plugins"]["updater"]["pubkey"].as_str().unwrap());
    assert!(update::DEFAULT_MANIFEST_URL.ends_with("/releases/latest/download/toybox-server.json"), "{}", update::DEFAULT_MANIFEST_URL);
    assert!(!update::TARGET.is_empty());
}

// ------------------------------------------------------------- the real binary

fn run_server(args: &[&str], key: Option<&str>) -> (i32, String, String) {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_toybox-server"));
    command.args(args).env_remove("TOYBOX_DATA").env_remove("TOYBOX_LISTEN");
    if let Some(key) = key {
        command.env("TOYBOX_TEST_UPDATE_PUBKEY", key);
    }
    let output = command.output().unwrap();
    (output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stdout).to_string(), String::from_utf8_lossy(&output.stderr).to_string())
}

#[test]
fn update_check_exits_0_when_current_and_10_when_a_newer_release_exists() {
    let dirs = Dirs::new("cli-check");
    let keys = Keys::new();
    let data = dirs.root.join("data");
    let data = data.to_string_lossy().to_string();
    let target = env!("TOYBOX_TARGET");
    for (version, code, words) in [(env!("CARGO_PKG_VERSION"), 0, "up to date"), ("999.0.0", 10, "999.0.0 is available"), ("0.0.1", 0, "newer than the latest release")] {
        let folder = dirs.root.join(format!("rel-{version}"));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("toybox-server.json"), manifest(version, &[(target, "file", &keys.sign(b"x"))])).unwrap();
        let (status, out, err) = run_server(&["update", "--check", "--from", &folder.to_string_lossy(), "--data", &data], Some(&keys.public));
        assert_eq!(status, code, "{version}: {out}{err}");
        assert!(out.contains(words), "{version}: {out}");
    }
    // No manifest there: a failure (1), not "up to date".
    let (status, _, err) = run_server(&["update", "--check", "--from", &dirs.root.join("nowhere").to_string_lossy(), "--data", &data], None);
    assert_eq!(status, 1);
    assert!(err.contains("Could not read the release information"), "{err}");
}

#[test]
fn update_without_a_terminal_and_without_yes_asks_for_yes() {
    let dirs = Dirs::new("cli-yes");
    let keys = Keys::new();
    let data = dirs.root.join("data").to_string_lossy().to_string();
    let folder = dirs.root.join("rel");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("toybox-server.json"), manifest("999.0.0", &[(env!("TOYBOX_TARGET"), "file", &keys.sign(b"x"))])).unwrap();
    let (status, _, err) = run_server(&["update", "--from", &folder.to_string_lossy(), "--data", &data], Some(&keys.public));
    assert_eq!(status, 1);
    assert!(err.contains("--yes"), "{err}");
}

#[test]
fn migrate_check_and_snapshot_list_work_on_a_fresh_data_folder_and_change_nothing() {
    let dirs = Dirs::new("cli-migrate");
    let data = dirs.root.join("data");
    let data_text = data.to_string_lossy().to_string();
    let (status, _, err) = run_server(&["init", "--data", &data_text], None);
    assert_eq!(status, 0, "{err}");
    let tree = |root: &Path| -> Vec<(String, Vec<u8>)> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    out.push((path.strip_prefix(root).unwrap().to_string_lossy().to_string(), std::fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    };
    let before = tree(&data);
    let (status, out, err) = run_server(&["migrate", "--check", "--data", &data_text], None);
    assert_eq!(status, 0, "{err}");
    assert!(out.contains("Format version:"), "{out}");
    let (status, json, _) = run_server(&["migrate", "--check", "--json", "--data", &data_text], None);
    assert_eq!(status, 0);
    let report: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(report["latestVersion"], 6);
    let (status, out, _) = run_server(&["snapshot", "list", "--data", &data_text], None);
    assert_eq!(status, 0);
    assert!(out.contains("No snapshots"), "{out}");
    assert_eq!(tree(&data), before, "nothing in the data folder changed");
    let (status, _, err) = run_server(&["snapshot", "restore", "library-v4-nothing.db", "--data", &data_text], None);
    assert_eq!(status, 1);
    assert!(err.contains("no snapshot named"), "{err}");
}

#[test]
fn a_signature_made_by_the_tauri_signer_verifies_and_a_change_does_not() {
    let program = include_bytes!("fixtures/signed-by-tauri.bin");
    let signature = include_str!("fixtures/signed-by-tauri.bin.sig").trim();
    let key = include_str!("fixtures/test-public-key.txt").trim();
    update::verify(key, signature, program).unwrap();
    let mut changed = program.to_vec();
    changed[0] ^= 1;
    assert!(update::verify(key, signature, &changed).is_err());
    // Another key does not vouch for it.
    assert!(update::verify(&Keys::new().public, signature, program).is_err());
    // Nor does the app's real key: this fixture was never signed with it.
    assert!(update::verify(&update::release_public_key(), signature, program).is_err());
}
