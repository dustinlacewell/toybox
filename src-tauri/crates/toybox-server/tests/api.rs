//! End-to-end tests of the HTTP API: the real router on a temporary data
//! directory, driven in process with tower `oneshot`. The command line is
//! exercised through the built binary where a test needs it.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use toybox_server::config::Role;
use toybox_server::data::{self, DataDir};
use toybox_server::state::{AppState, Options};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Harness {
    dir: PathBuf,
    state: AppState,
    app: Router,
    admin: String,
}

impl Harness {
    fn new(tweak: impl FnOnce(&mut Options)) -> Harness {
        let dir = std::env::temp_dir().join(format!("toybox-server-test-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        let data = DataDir::new(&dir);
        let outcome = data::init(&data, "Test server").expect("init");
        let mut options = Options::default();
        options.fail_delay = Duration::from_millis(1);
        options.log_requests = false;
        options.lockout.max_failures = 10_000;
        tweak(&mut options);
        let state = AppState::open(data, options).expect("open");
        let app = toybox_server::api::router(state.clone());
        Harness { dir, state, app, admin: outcome.secret }
    }

    fn token(&self, name: &str, role: Role) -> String {
        self.state.config.mutate(|config| config.add_token(name, role)).unwrap().1
    }

    /// Reopen the same data directory with a fresh server (a restart).
    fn restarted(&self) -> Router {
        let state = AppState::open(DataDir::new(&self.dir), Options { log_requests: false, ..Options::default() }).unwrap();
        toybox_server::api::router(state)
    }

    fn library(&self) -> PathBuf {
        self.dir.join("library")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|e| panic!("not json ({e}): {}", String::from_utf8_lossy(&self.body)))
    }
    fn code(&self) -> String {
        self.json()["error"]["code"].as_str().unwrap_or("").to_string()
    }
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).map(|v| v.to_str().unwrap()).unwrap_or("")
    }
}

fn peer(address: &str) -> ConnectInfo<SocketAddr> {
    ConnectInfo(format!("{address}:50000").parse().unwrap())
}

fn build(method: &str, path: &str, token: Option<&str>, body: Body, extra: &[(&str, &str)], from: &str) -> Request<Body> {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    let mut request = request.body(body).unwrap();
    request.extensions_mut().insert(peer(from));
    request
}

async fn send(app: &Router, request: Request<Body>) -> Reply {
    let response = app.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    Reply { status: parts.status, headers: parts.headers, body: body.collect().await.unwrap().to_bytes().to_vec() }
}

async fn call(h: &Harness, method: &str, path: &str, token: Option<&str>) -> Reply {
    send(&h.app, build(method, path, token, Body::empty(), &[], "127.0.0.1")).await
}

async fn call_with(h: &Harness, method: &str, path: &str, token: Option<&str>, extra: &[(&str, &str)]) -> Reply {
    send(&h.app, build(method, path, token, Body::empty(), extra, "127.0.0.1")).await
}

async fn call_json(h: &Harness, method: &str, path: &str, token: Option<&str>, body: Value) -> Reply {
    let request = build(method, path, token, Body::from(serde_json::to_vec(&body).unwrap()), &[("content-type", "application/json")], "127.0.0.1");
    send(&h.app, request).await
}

async fn put_bytes(h: &Harness, path: &str, token: &str, bytes: Vec<u8>) -> Reply {
    send(&h.app, build("PUT", path, Some(token), Body::from(bytes), &[], "127.0.0.1")).await
}

const SKY: &[u8] = b"radiance-data-0123456789";

fn sky_entry(id: &str, pack: &str, name: &str) -> Value {
    json!({"id": id, "kind": "plugin", "type": "skybox", "pack": pack, "category": "day", "name": name, "files": ["noon.hdr"]})
}

async fn new_upload(h: &Harness, token: &str) -> String {
    let reply = call(h, "POST", "/api/v1/uploads", Some(token)).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&reply.body));
    reply.json()["id"].as_str().unwrap().to_string()
}

/// Upload one sky and commit it. Returns the commit reply.
async fn import_sky(h: &Harness, token: &str, id: &str, name: &str) -> Reply {
    let pack = format!("_import_{}", id.replace(|c: char| !c.is_ascii_alphanumeric(), ""));
    let upload = new_upload(h, token).await;
    let path = format!("/api/v1/uploads/{upload}/files/library/{pack}/skybox/{name}/noon.hdr");
    let put = put_bytes(h, &path, token, SKY.to_vec()).await;
    assert_eq!(put.status, StatusCode::OK, "{}", String::from_utf8_lossy(&put.body));
    call_json(h, "POST", &format!("/api/v1/uploads/{upload}/commit"), Some(token), json!({"entries": [sky_entry(id, &pack, name)]})).await
}

#[tokio::test]
async fn every_route_enforces_its_role() {
    let h = Harness::new(|_| {});
    let read = h.token("reader", Role::Read);
    let write = h.token("writer", Role::Write);
    let admin = h.admin.clone();
    let upload = new_upload(&h, &write).await;
    let stage = format!("/api/v1/uploads/{upload}/files/library/_import_matrix/skybox/A/noon.hdr");
    let commit = format!("/api/v1/uploads/{upload}/commit");

    // (method, path, minimum role, JSON body)
    let routes: Vec<(&str, String, Role, Option<Value>)> = vec![
        ("GET", "/api/v1/info".into(), Role::Read, None),
        ("GET", "/api/v1/catalog".into(), Role::Read, None),
        ("GET", "/api/v1/changes?since=0".into(), Role::Read, None),
        ("GET", "/api/v1/files/library/audio/0123456789abcdef/none.wav".into(), Role::Read, None),
        ("GET", "/api/v1/thumbs".into(), Role::Read, None),
        ("GET", "/api/v1/thumbs/uid%3A%2F%2Fnone".into(), Role::Read, None),
        ("PUT", "/api/v1/thumbs/uid%3A%2F%2Fnone".into(), Role::Write, None),
        ("POST", "/api/v1/uploads".into(), Role::Write, None),
        ("DELETE", "/api/v1/uploads/00000000000000000000000000000000".into(), Role::Write, None),
        ("PUT", stage, Role::Write, None),
        ("POST", commit, Role::Write, Some(json!({"entries": []}))),
        ("POST", "/api/v1/assets/delete".into(), Role::Write, Some(json!({"ids": ["uid://none"]}))),
        ("POST", "/api/v1/assets/metadata".into(), Role::Write, Some(json!({"edits": [{"id": "uid://none", "tags": ["x"]}]}))),
        ("GET", "/api/v1/tokens".into(), Role::Admin, None),
        ("POST", "/api/v1/tokens".into(), Role::Admin, Some(json!({"name": "matrix", "role": "read"}))),
        ("DELETE", "/api/v1/tokens/ffffffff".into(), Role::Admin, None),
    ];
    for (method, path, needed, body) in routes {
        let attempt = |token: Option<String>| {
            let h = &h;
            let (method, path, body) = (method, path.clone(), body.clone());
            async move {
                let (payload, extra): (Body, Vec<(&str, &str)>) = match body {
                    Some(v) => (Body::from(serde_json::to_vec(&v).unwrap()), vec![("content-type", "application/json")]),
                    None => (Body::empty(), vec![]),
                };
                send(&h.app, build(method, &path, token.as_deref(), payload, &extra, "127.0.0.1")).await
            }
        };
        let none = attempt(None).await;
        assert_eq!(none.status, StatusCode::UNAUTHORIZED, "{method} {path} without a token");
        assert_eq!(none.code(), "unauthorized");
        let bad = attempt(Some("tbx_not-a-real-token".into())).await;
        assert_eq!(bad.status, StatusCode::UNAUTHORIZED, "{method} {path} with a bad token");
        for (role, token) in [(Role::Read, &read), (Role::Write, &write), (Role::Admin, &admin)] {
            let reply = attempt(Some(token.clone())).await;
            if role >= needed {
                assert!(
                    reply.status != StatusCode::UNAUTHORIZED && reply.status != StatusCode::FORBIDDEN,
                    "{method} {path} should be open to {role:?}, got {}",
                    reply.status
                );
            } else {
                assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {path} should refuse {role:?}");
                assert_eq!(reply.code(), "forbidden");
                assert!(reply.json()["error"]["message"].as_str().unwrap().contains(needed.as_str()));
            }
        }
    }
    // Unknown routes do not reveal themselves to strangers either.
    assert_eq!(call(&h, "GET", "/api/v1/nothing", None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(call(&h, "GET", "/api/v1/nothing", Some(&read)).await.status, StatusCode::NOT_FOUND);
    // Health is the one open route.
    let health = call(&h, "GET", "/api/v1/health", None).await;
    assert_eq!((health.status, health.json()["status"].clone()), (StatusCode::OK, json!("ok")));
}

#[tokio::test]
async fn info_reports_name_version_role_and_revision() {
    let h = Harness::new(|_| {});
    let read = h.token("reader", Role::Read);
    let info = call(&h, "GET", "/api/v1/info", Some(&read)).await.json();
    assert_eq!(info["name"], "Test server");
    assert_eq!(info["apiVersion"], 2);
    assert_eq!(info["role"], "read");
    assert_eq!(info["serverVersion"], env!("CARGO_PKG_VERSION"));
    assert!(info["revision"].as_str().unwrap().len() >= 8);
    let admin_info = call(&h, "GET", "/api/v1/info", Some(&h.admin)).await.json();
    assert_eq!(admin_info["role"], "admin");
    assert_eq!(admin_info["revision"], info["revision"]);
}

#[tokio::test]
async fn token_add_use_revoke_through_the_api() {
    let h = Harness::new(|_| {});
    let created = call_json(&h, "POST", "/api/v1/tokens", Some(&h.admin), json!({"name": "friend", "role": "read"})).await;
    assert_eq!(created.status, StatusCode::CREATED);
    let body = created.json();
    let secret = body["token"].as_str().unwrap().to_string();
    assert!(secret.starts_with("tbx_"));
    assert_eq!(body["role"], "read");
    let id = body["id"].as_str().unwrap().to_string();

    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&secret)).await.json()["role"], "read");
    let listed = call(&h, "GET", "/api/v1/tokens", Some(&h.admin)).await;
    let text = String::from_utf8_lossy(&listed.body).to_string();
    assert!(!text.contains(&secret) && !text.contains("sha256"), "secrets and hashes are never listed");
    let friend = listed.json()["tokens"].as_array().unwrap().iter().find(|t| t["name"] == "friend").cloned().unwrap();
    assert_eq!(friend["id"], id.as_str());
    assert!(friend["lastUsedAt"].is_number(), "use is visible");

    // Bad requests get plain answers.
    assert_eq!(call_json(&h, "POST", "/api/v1/tokens", Some(&h.admin), json!({"name": "friend", "role": "read"})).await.code(), "name_taken");
    assert_eq!(call_json(&h, "POST", "/api/v1/tokens", Some(&h.admin), json!({"name": "  ", "role": "read"})).await.code(), "invalid_name");
    assert_eq!(call_json(&h, "POST", "/api/v1/tokens", Some(&h.admin), json!({"name": "x", "role": "root"})).await.code(), "bad_request");
    assert_eq!(call(&h, "DELETE", "/api/v1/tokens/nosuchid", Some(&h.admin)).await.status, StatusCode::NOT_FOUND);

    assert_eq!(call(&h, "DELETE", &format!("/api/v1/tokens/{id}"), Some(&h.admin)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&secret)).await.status, StatusCode::UNAUTHORIZED);
}

fn cli(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_toybox-server"))
        .args(args)
        .arg("--data")
        .arg(dir)
        .env_remove("TOYBOX_DATA")
        .output()
        .expect("run toybox-server");
    (output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stdout).to_string(), String::from_utf8_lossy(&output.stderr).to_string())
}

fn secret_in(output: &str) -> String {
    output.lines().map(str::trim).find(|l| l.starts_with("tbx_")).expect("a token in the output").to_string()
}

#[tokio::test]
async fn tokens_edited_by_the_command_line_reach_a_running_server() {
    let h = Harness::new(|_| {});
    // The server is already running (the router above); the CLI edits server.json behind its back.
    let (code, out, _) = cli(&h.dir, &["token", "add", "--name", "cli-writer", "--role", "write"]);
    assert_eq!(code, 0, "{out}");
    let secret = secret_in(&out);
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&secret)).await.json()["role"], "write");

    let (_, listing, _) = cli(&h.dir, &["token", "list"]);
    assert!(listing.contains("cli-writer") && !listing.contains(&secret), "{listing}");

    let (code, out, _) = cli(&h.dir, &["token", "revoke", "cli-writer"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&secret)).await.status, StatusCode::UNAUTHORIZED);

    // A hand-broken file fails closed: nobody gets in until it is fixed.
    let path = h.dir.join("server.json");
    let good = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"{ nonsense").unwrap();
    let broken = call(&h, "GET", "/api/v1/info", Some(&h.admin)).await;
    assert_eq!((broken.status, broken.code().as_str()), (StatusCode::SERVICE_UNAVAILABLE, "settings_unreadable"));
    std::fs::write(&path, good).unwrap();
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&h.admin)).await.status, StatusCode::OK);
}

#[tokio::test]
async fn the_last_admin_cannot_be_deleted() {
    let h = Harness::new(|_| {});
    let me = call(&h, "GET", "/api/v1/tokens", Some(&h.admin)).await.json()["tokens"][0]["id"].as_str().unwrap().to_string();
    let refused = call(&h, "DELETE", &format!("/api/v1/tokens/{me}"), Some(&h.admin)).await;
    assert_eq!((refused.status, refused.code().as_str()), (StatusCode::CONFLICT, "last_admin"));
    // The command line enforces the same rule.
    let (code, _, err) = cli(&h.dir, &["token", "revoke", "admin"]);
    assert_eq!(code, 1);
    assert!(err.contains("last admin"), "{err}");

    let second = call_json(&h, "POST", "/api/v1/tokens", Some(&h.admin), json!({"name": "second", "role": "admin"})).await.json();
    let second_secret = second["token"].as_str().unwrap().to_string();
    assert_eq!(call(&h, "DELETE", &format!("/api/v1/tokens/{me}"), Some(&h.admin)).await.status, StatusCode::NO_CONTENT);
    let second_id = second["id"].as_str().unwrap();
    assert_eq!(call(&h, "DELETE", &format!("/api/v1/tokens/{second_id}"), Some(&second_secret)).await.code(), "last_admin");
    // Demoting by deleting a non-admin is always fine.
    let reader = call_json(&h, "POST", "/api/v1/tokens", Some(&second_secret), json!({"name": "r", "role": "read"})).await.json();
    let reader_id = reader["id"].as_str().unwrap();
    assert_eq!(call(&h, "DELETE", &format!("/api/v1/tokens/{reader_id}"), Some(&second_secret)).await.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn repeated_wrong_tokens_lock_an_address_out() {
    let h = Harness::new(|o| {
        o.lockout.max_failures = 3;
        o.lockout.lockout = Duration::from_secs(300);
    });
    let attempt = |token: &str, from: &str, forwarded: Option<&str>| {
        let mut extra = vec![];
        if let Some(f) = forwarded {
            extra.push(("x-forwarded-for", f));
        }
        send_owned(&h, token.to_string(), from.to_string(), extra.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect())
    };
    // Behind the proxy (peer is loopback) the forwarded address is the client.
    for _ in 0..3 {
        assert_eq!(attempt("tbx_wrong", "127.0.0.1", Some("203.0.113.9")).await.status, StatusCode::UNAUTHORIZED);
    }
    let locked = attempt(&h.admin, "127.0.0.1", Some("203.0.113.9")).await;
    assert_eq!((locked.status, locked.code().as_str()), (StatusCode::TOO_MANY_REQUESTS, "locked_out"));
    assert!(locked.header("retry-after").parse::<u64>().unwrap() > 0);
    // Someone else behind the same proxy is fine.
    assert_eq!(attempt(&h.admin, "127.0.0.1", Some("203.0.113.10")).await.status, StatusCode::OK);

    // A direct caller cannot dodge the lockout, or frame another address, with a header.
    for i in 0..3 {
        attempt("tbx_wrong", "198.51.100.7", Some(&format!("192.0.2.{i}"))).await;
    }
    assert_eq!(attempt(&h.admin, "198.51.100.7", Some("192.0.2.200")).await.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(attempt(&h.admin, "198.51.100.8", None).await.status, StatusCode::OK);
    // A success clears earlier failures.
    attempt("tbx_wrong", "198.51.100.9", None).await;
    attempt("tbx_wrong", "198.51.100.9", None).await;
    assert_eq!(attempt(&h.admin, "198.51.100.9", None).await.status, StatusCode::OK);
    attempt("tbx_wrong", "198.51.100.9", None).await;
    attempt("tbx_wrong", "198.51.100.9", None).await;
    assert_eq!(attempt(&h.admin, "198.51.100.9", None).await.status, StatusCode::OK);
}

async fn send_owned(h: &Harness, token: String, from: String, extra: Vec<(String, String)>) -> Reply {
    let extra: Vec<(&str, &str)> = extra.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    send(&h.app, build("GET", "/api/v1/info", Some(&token), Body::empty(), &extra, &from)).await
}

fn write_library_file(h: &Harness, rel: &str, bytes: &[u8]) {
    let path = h.library().join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[tokio::test]
async fn files_cannot_be_read_outside_the_library_folder() {
    let h = Harness::new(|_| {});
    let read = h.token("reader", Role::Read);
    std::fs::write(h.dir.join("secret.txt"), b"top secret").unwrap();
    write_library_file(&h, "library/audio/0123456789abcdef/ok.bin", b"fine");
    assert_eq!(call(&h, "GET", "/api/v1/files/library/audio/0123456789abcdef/ok.bin", Some(&read)).await.status, StatusCode::OK);

    let attempts = [
        "/api/v1/files/../server.json",
        "/api/v1/files/library/../../server.json",
        "/api/v1/files/library/audio/../../../server.json",
        "/api/v1/files/..%2fserver.json",
        "/api/v1/files/library%2f..%2f..%2fserver.json",
        "/api/v1/files/%2e%2e/%2e%2e/server.json",
        "/api/v1/files/..%5cserver.json",
        "/api/v1/files/library%5c..%5c..%5cserver.json",
        "/api/v1/files/%2Fetc%2Fpasswd",
        "/api/v1/files//etc/passwd",
        "/api/v1/files/C:/Windows/win.ini",
        "/api/v1/files/C%3A%5CWindows%5Cwin.ini",
        "/api/v1/files/library/audio/0123456789abcdef/ok.bin%00.png",
        "/api/v1/files/library/audio/0123456789abcdef/",
        "/api/v1/files/library/audio/0123456789abcdef",
        "/api/v1/files/server.json",
        "/api/v1/files/_library_config/catalog.json",
        "/api/v1/files/library/../_library_config/catalog.json",
        "/api/v1/files/_library_config/library.db",
        "/api/v1/files/_library_config/library.db-wal",
        "/api/v1/files/cache/catalog.json",
        "/api/v1/files/cache/user.db",
        "/api/v1/files/uploads",
        "/api/v1/files/",
    ];
    for path in attempts {
        let reply = call(&h, "GET", path, Some(&read)).await;
        assert!(
            reply.status == StatusCode::BAD_REQUEST || reply.status == StatusCode::NOT_FOUND,
            "{path} answered {}: {}",
            reply.status,
            String::from_utf8_lossy(&reply.body)
        );
        assert!(!String::from_utf8_lossy(&reply.body).contains("top secret"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_inside_the_library_cannot_lead_out_or_to_internals() {
    let h = Harness::new(|_| {});
    let read = h.token("reader", Role::Read);
    std::fs::write(h.dir.join("secret.txt"), b"top secret").unwrap();
    std::fs::create_dir_all(h.library().join("library/audio")).unwrap();
    std::os::unix::fs::symlink(h.dir.join("secret.txt"), h.library().join("library/audio/out.txt")).unwrap();
    std::os::unix::fs::symlink(h.library().join("_library_config/catalog.json"), h.library().join("library/audio/manifest.json")).unwrap();
    for path in ["/api/v1/files/library/audio/out.txt", "/api/v1/files/library/audio/manifest.json"] {
        let reply = call(&h, "GET", path, Some(&read)).await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn files_support_ranges_validators_and_long_caching() {
    let h = Harness::new(|_| {});
    let read = h.token("reader", Role::Read);
    let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    write_library_file(&h, "library/audio/0123456789abcdef/blob.bin", &data);
    write_library_file(&h, "library/audio/legacy_pack/blob.bin", &data);
    let path = "/api/v1/files/library/audio/0123456789abcdef/blob.bin";

    let full = call(&h, "GET", path, Some(&read)).await;
    assert_eq!(full.status, StatusCode::OK);
    assert_eq!(full.body, data);
    assert_eq!(full.header("accept-ranges"), "bytes");
    assert_eq!(full.header("content-length"), "1000");
    assert_eq!(full.header("cache-control"), "private, max-age=31536000, immutable");
    assert_eq!(full.header("x-content-type-options"), "nosniff");
    let etag = full.header("etag").to_string();
    assert!(etag.starts_with('"'));

    let part = call_with(&h, "GET", path, Some(&read), &[("range", "bytes=10-19")]).await;
    assert_eq!(part.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.body, data[10..20]);
    assert_eq!(part.header("content-range"), "bytes 10-19/1000");
    assert_eq!(part.header("content-length"), "10");

    let tail = call_with(&h, "GET", path, Some(&read), &[("range", "bytes=-5")]).await;
    assert_eq!((tail.status, tail.body.clone()), (StatusCode::PARTIAL_CONTENT, data[995..].to_vec()));
    assert_eq!(tail.header("content-range"), "bytes 995-999/1000");
    let open = call_with(&h, "GET", path, Some(&read), &[("range", "bytes=990-")]).await;
    assert_eq!((open.status, open.body.clone()), (StatusCode::PARTIAL_CONTENT, data[990..].to_vec()));
    let clipped = call_with(&h, "GET", path, Some(&read), &[("range", "bytes=900-5000")]).await;
    assert_eq!(clipped.header("content-range"), "bytes 900-999/1000");

    let beyond = call_with(&h, "GET", path, Some(&read), &[("range", "bytes=1000-")]).await;
    assert_eq!(beyond.status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(beyond.header("content-range"), "bytes */1000");

    let revalidated = call_with(&h, "GET", path, Some(&read), &[("if-none-match", etag.as_str())]).await;
    assert_eq!(revalidated.status, StatusCode::NOT_MODIFIED);
    assert!(revalidated.body.is_empty());
    let stale_range = call_with(&h, "GET", path, Some(&read), &[("range", "bytes=0-9"), ("if-range", "\"old\"")]).await;
    assert_eq!((stale_range.status, stale_range.body.len()), (StatusCode::OK, 1000), "a changed file is sent whole");
    let fresh_range = call_with(&h, "GET", path, Some(&read), &[("range", "bytes=0-9"), ("if-range", etag.as_str())]).await;
    assert_eq!(fresh_range.status, StatusCode::PARTIAL_CONTENT);

    let legacy = call(&h, "GET", "/api/v1/files/library/audio/legacy_pack/blob.bin", Some(&read)).await;
    assert_eq!(legacy.header("cache-control"), "private, no-cache");
    let head = send(&h.app, build("HEAD", path, Some(&read), Body::empty(), &[], "127.0.0.1")).await;
    assert_eq!((head.status, head.body.len()), (StatusCode::OK, 0));
}

#[tokio::test]
async fn import_download_thumbnail_and_delete_round_trip() {
    let h = Harness::new(|_| {});
    let read = h.token("reader", Role::Read);
    let write = h.token("writer", Role::Write);

    let before = call(&h, "GET", "/api/v1/catalog", Some(&read)).await;
    assert_eq!(before.status, StatusCode::OK);
    let revision_before = before.header("etag").to_string();
    assert_eq!(before.json()["assets"].as_array().unwrap().len(), 0);
    assert_eq!(before.json()["libraryRoot"], "", "the server's folder is not revealed");

    // Import.
    let committed = import_sky(&h, &write, "uid://sky1", "Noon").await;
    assert_eq!(committed.status, StatusCode::OK, "{}", String::from_utf8_lossy(&committed.body));
    let result = committed.json();
    assert_eq!(result["imported"], json!(["uid://sky1"]));
    assert_eq!(result["errors"], json!([]));
    assert_eq!(result["uploadKept"], false);
    let revision = result["revision"].as_str().unwrap().to_string();
    assert_ne!(format!("\"{revision}\""), revision_before);

    // The catalog shows it, with the new ETag, and 304 works.
    let after = call(&h, "GET", "/api/v1/catalog", Some(&read)).await;
    assert_eq!(after.header("etag"), format!("\"{revision}\""));
    let catalog = after.json();
    let asset = &catalog["assets"][0];
    assert_eq!((asset["id"].as_str(), asset["kind"].as_str(), asset["type"].as_str()), (Some("uid://sky1"), Some("plugin"), Some("skybox")));
    assert_eq!(asset["user"]["favorite"], false);
    let not_modified = call_with(&h, "GET", "/api/v1/catalog", Some(&read), &[("if-none-match", &format!("\"{revision}\""))]).await;
    assert_eq!(not_modified.status, StatusCode::NOT_MODIFIED);
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&read)).await.json()["revision"], revision.as_str());

    // The file downloads from the path the catalog names, and the staging area is gone.
    let file = asset["files"][0].as_str().unwrap().to_string();
    assert!(file.starts_with("library/skybox/") && file.ends_with("/noon.hdr"), "{file}");
    let download = call(&h, "GET", &format!("/api/v1/files/{file}"), Some(&read)).await;
    assert_eq!((download.status, download.body.as_slice()), (StatusCode::OK, SKY));
    assert!(!h.library().join("library/_import_sky1").exists());
    assert_eq!(std::fs::read_dir(h.dir.join("uploads")).unwrap().count(), 0, "a finished import leaves nothing in staging");

    // Shared thumbnail.
    let thumb_path = "/api/v1/thumbs/uid%3A%2F%2Fsky1";
    assert_eq!(call(&h, "GET", thumb_path, Some(&read)).await.status, StatusCode::NOT_FOUND);
    let png: Vec<u8> = [&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..], b"fake png body"].concat();
    assert_eq!(put_bytes(&h, thumb_path, &read, png.clone()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(put_bytes(&h, thumb_path, &write, b"GIF89a".to_vec()).await.code(), "not_png");
    assert_eq!(put_bytes(&h, thumb_path, &write, vec![0x89; 3 * 1024 * 1024]).await.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(put_bytes(&h, "/api/v1/thumbs/uid%3A%2F%2Funknown", &write, png.clone()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(put_bytes(&h, thumb_path, &write, png.clone()).await.status, StatusCode::NO_CONTENT);
    let thumb = call(&h, "GET", thumb_path, Some(&read)).await;
    assert_eq!((thumb.status, thumb.body.clone(), thumb.header("content-type")), (StatusCode::OK, png.clone(), "image/png"));
    let thumb_etag = thumb.header("etag").to_string();
    assert_eq!(call_with(&h, "GET", thumb_path, Some(&read), &[("if-none-match", thumb_etag.as_str())]).await.status, StatusCode::NOT_MODIFIED);
    let list = call(&h, "GET", "/api/v1/thumbs", Some(&read)).await;
    assert_eq!(list.json()["thumbs"][0]["id"], "uid://sky1");
    assert_eq!(list.json()["revision"], revision.as_str());
    let list_etag = list.header("etag").to_string();
    assert_eq!(call_with(&h, "GET", "/api/v1/thumbs", Some(&read), &[("if-none-match", list_etag.as_str())]).await.status, StatusCode::NOT_MODIFIED);
    // A thumbnail does not change the library revision.
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&read)).await.json()["revision"], revision.as_str());

    // A restart sees the same library without rebuilding anything different.
    let restarted = h.restarted();
    let again = send(&restarted, build("GET", "/api/v1/catalog", Some(&h.admin), Body::empty(), &[], "127.0.0.1")).await;
    assert_eq!(again.header("etag"), format!("\"{revision}\""));

    // Delete: read may not; write may; unknown ids are refused as a whole.
    let body = json!({"ids": ["uid://sky1"]});
    assert_eq!(call_json(&h, "POST", "/api/v1/assets/delete", Some(&read), body.clone()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call_json(&h, "POST", "/api/v1/assets/delete", Some(&write), json!({"ids": ["uid://sky1", "uid://nope"]})).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call_json(&h, "POST", "/api/v1/assets/delete", Some(&write), json!({"ids": []})).await.code(), "no_ids");
    assert!(h.library().join(&file).exists(), "a refused delete removes nothing");
    let deleted = call_json(&h, "POST", "/api/v1/assets/delete", Some(&write), body).await;
    assert_eq!(deleted.status, StatusCode::OK, "{}", String::from_utf8_lossy(&deleted.body));
    let deleted_json = deleted.json();
    assert_eq!(deleted_json["deleted"], json!(["uid://sky1"]));
    assert_eq!(deleted_json["catalog"]["assets"].as_array().unwrap().len(), 0);
    let new_revision = deleted_json["revision"].as_str().unwrap();
    assert_ne!(new_revision, revision);
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&read)).await.json()["revision"], new_revision);
    assert_eq!(call(&h, "GET", &format!("/api/v1/files/{file}"), Some(&read)).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&h, "GET", thumb_path, Some(&read)).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&h, "GET", "/api/v1/catalog", Some(&read)).await.json()["assets"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn uploads_stay_in_their_staging_area() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    let upload = new_upload(&h, &write).await;
    let base = format!("/api/v1/uploads/{upload}/files");
    let attempts = [
        "library/_import_x1/../../../escape.bin",
        "library/_import_x1/skybox/..%2f..%2f..%2f..%2fescape.bin",
        "library/_import_x1/skybox/..%5c..%5cescape.bin",
        "../escape.bin",
        "%2Fescape.bin",
        "/escape.bin",
        "escape.bin",
        "library/audio/0123456789abcdef/escape.bin",
        "library/_import_/x/y.bin",
        "library/_import_x1/y.bin",
        "_library_config/catalog.json",
        "_library_config/library.db",
        "library/_import_x1/skybox/a.tbx-part",
        "C:/escape.bin",
    ];
    for rel in attempts {
        let reply = put_bytes(&h, &format!("{base}/{rel}"), &write, b"x".to_vec()).await;
        assert!(reply.status == StatusCode::BAD_REQUEST || reply.status == StatusCode::NOT_FOUND, "{rel} answered {}", reply.status);
    }
    assert!(!h.dir.join("escape.bin").exists() && !h.dir.join("uploads/escape.bin").exists());
    assert!(!h.library().join("library/audio/0123456789abcdef/escape.bin").exists());
    // An unknown or malformed upload id is a clean 404.
    let missing = put_bytes(&h, "/api/v1/uploads/00000000000000000000000000000000/files/library/_import_x1/a/b.bin", &write, b"x".to_vec()).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    let odd = put_bytes(&h, "/api/v1/uploads/..%2f..%2fcache/files/library/_import_x1/a/b.bin", &write, b"x".to_vec()).await;
    assert_eq!(odd.status, StatusCode::NOT_FOUND);
    // A file and a folder cannot share a name.
    assert_eq!(put_bytes(&h, &format!("{base}/library/_import_x1/a/b"), &write, b"x".to_vec()).await.status, StatusCode::OK);
    assert_eq!(put_bytes(&h, &format!("{base}/library/_import_x1/a/b/c"), &write, b"x".to_vec()).await.status, StatusCode::CONFLICT);
    // Cancelling removes the staged files.
    assert_eq!(call(&h, "DELETE", &format!("/api/v1/uploads/{upload}"), Some(&write)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(std::fs::read_dir(h.dir.join("uploads")).unwrap().count(), 0);
    assert_eq!(call(&h, "DELETE", &format!("/api/v1/uploads/{upload}"), Some(&write)).await.status, StatusCode::NOT_FOUND);
}


#[tokio::test]
async fn names_that_windows_cannot_store_or_would_merge_are_refused() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    let upload = new_upload(&h, &write).await;
    let base = format!("/api/v1/uploads/{upload}/files/library/_import_w1/skybox");
    // Legal on Linux, but the library is copied to Windows machines.
    for name in ["CON.png", "nul", "com1.txt", "a%7Cb.png", "what%3F.png", "star*.png", "q%22.png", "lt%3C.png"] {
        let reply = put_bytes(&h, &format!("{base}/{name}"), &write, b"x".to_vec()).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{name}");
    }
    assert_eq!(put_bytes(&h, &format!("{base}/console.png"), &write, b"x".to_vec()).await.status, StatusCode::OK);
    // Names that differ only in case are one file on Windows.
    assert_eq!(put_bytes(&h, &format!("{base}/Tex.png"), &write, b"x".to_vec()).await.status, StatusCode::OK);
    assert_eq!(put_bytes(&h, &format!("{base}/tex.PNG"), &write, b"x".to_vec()).await.status, StatusCode::CONFLICT);
    assert_eq!(put_bytes(&h, &format!("{}/SkyBox/other.png", base.trim_end_matches("/skybox")), &write, b"x".to_vec()).await.status, StatusCode::CONFLICT);
    // The very same name again is a replacement, as before.
    assert_eq!(put_bytes(&h, &format!("{base}/Tex.png"), &write, b"y".to_vec()).await.status, StatusCode::OK);
    assert_eq!(put_bytes(&h, &format!("{base}/other.png"), &write, b"y".to_vec()).await.status, StatusCode::OK);
}
#[tokio::test]
async fn size_and_count_limits_are_enforced() {
    let h = Harness::new(|o| {
        o.uploads.max_file_bytes = 1000;
        o.uploads.max_upload_bytes = 1500;
        o.uploads.max_files = 3;
        o.uploads.max_uploads = 2;
    });
    let write = h.token("writer", Role::Write);
    let upload = new_upload(&h, &write).await;
    let url = |name: &str| format!("/api/v1/uploads/{upload}/files/library/_import_lim/skybox/S/{name}");

    assert_eq!(put_bytes(&h, &url("big.bin"), &write, vec![0; 1001]).await.status, StatusCode::PAYLOAD_TOO_LARGE);
    // A stream with no declared length is cut off when it grows past the limit.
    let chunks: Vec<Result<Vec<u8>, std::io::Error>> = (0..5).map(|_| Ok(vec![0u8; 300])).collect();
    let streamed = send(&h.app, build("PUT", &url("stream.bin"), Some(&write), Body::from_stream(futures_util::stream::iter(chunks)), &[], "127.0.0.1")).await;
    assert_eq!(streamed.status, StatusCode::PAYLOAD_TOO_LARGE);
    let staged = h.dir.join("uploads").join(&upload).join("files/library/_import_lim/skybox/S");
    let leftovers = std::fs::read_dir(&staged).map(|d| d.count()).unwrap_or(0);
    assert_eq!(leftovers, 0, "a refused file leaves nothing behind");

    assert_eq!(put_bytes(&h, &url("a.bin"), &write, vec![0; 800]).await.status, StatusCode::OK);
    assert_eq!(put_bytes(&h, &url("b.bin"), &write, vec![0; 800]).await.status, StatusCode::PAYLOAD_TOO_LARGE, "per-import total");
    assert_eq!(put_bytes(&h, &url("a.bin"), &write, vec![0; 900]).await.status, StatusCode::OK, "replacing re-counts");
    assert_eq!(put_bytes(&h, &url("c.bin"), &write, vec![0; 100]).await.status, StatusCode::OK);
    assert_eq!(put_bytes(&h, &url("d.bin"), &write, vec![0; 10]).await.status, StatusCode::OK);
    assert_eq!(put_bytes(&h, &url("e.bin"), &write, vec![0; 10]).await.status, StatusCode::PAYLOAD_TOO_LARGE, "file count");

    assert_eq!(call(&h, "POST", "/api/v1/uploads", Some(&write)).await.status, StatusCode::CREATED);
    let third = call(&h, "POST", "/api/v1/uploads", Some(&write)).await;
    assert_eq!((third.status, third.code().as_str()), (StatusCode::TOO_MANY_REQUESTS, "too_many_uploads"));

    // Bodies of other routes are capped too.
    let huge = format!("{{\"ids\": [\"{}\"]}}", "x".repeat(3 * 1024 * 1024));
    let reply = send(&h.app, build("POST", "/api/v1/assets/delete", Some(&write), Body::from(huge), &[("content-type", "application/json")], "127.0.0.1")).await;
    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
}


#[tokio::test]
async fn a_bad_entry_is_reported_without_sinking_the_good_ones() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    let upload = new_upload(&h, &write).await;
    let put = |pack: &'static str, name: &'static str| {
        put_bytes_owned(&h, format!("/api/v1/uploads/{upload}/files/library/{pack}/skybox/{name}/noon.hdr"), write.clone(), SKY.to_vec())
    };
    assert_eq!(put("_import_good", "Good").await.status, StatusCode::OK);
    let revision_before = call(&h, "GET", "/api/v1/info", Some(&write)).await.json()["revision"].clone();

    let commit_url = format!("/api/v1/uploads/{upload}/commit");
    // Nothing was uploaded for "Missing", and four entries break the server rules.
    let entries = json!([
        sky_entry("uid://good", "_import_good", "Good"),
        sky_entry("uid://missing", "_import_good", "Missing"),
        sky_entry("uid://traversal", "_import_good", "../../x"),
        sky_entry("uid://elsewhere", "library", "Elsewhere"),
        {"id": "uid://stored", "kind": "plugin", "type": "skybox", "pack": "_import_good", "category": "day", "name": "Noon", "files": ["noon.hdr"], "storagePath": "library/skybox/0123456789abcdef"},
        sky_entry("uid://good", "_import_good", "Good"),
    ]);
    let reply = call_json(&h, "POST", &commit_url, Some(&write), json!({"entries": entries})).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", String::from_utf8_lossy(&reply.body));
    let body = reply.json();
    assert_eq!(body["imported"], json!(["uid://good"]));
    assert_eq!(body["uploadKept"], true, "the upload stays so the client can fix and retry");
    let errors = body["errors"].as_array().unwrap();
    let failed: Vec<&str> = errors.iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(failed, ["uid://traversal", "uid://elsewhere", "uid://stored", "uid://good", "uid://missing"]);
    for error in errors {
        let message = error["message"].as_str().unwrap();
        assert!(!message.is_empty() && !message.contains(h.dir.to_str().unwrap()), "{message}");
    }
    assert_ne!(body["revision"], revision_before);
    assert!(h.state.uploads.exists(&upload));
    assert!(!h.library().join("library/_import_good").exists(), "no staging debris in the library");

    // Retrying with the missing file uploaded succeeds, and the upload is then consumed.
    assert_eq!(put("_import_good", "Missing").await.status, StatusCode::OK);
    let retry = call_json(&h, "POST", &commit_url, Some(&write), json!({"entries": [sky_entry("uid://missing", "_import_good", "Missing")]})).await;
    assert_eq!(retry.status, StatusCode::OK, "{}", String::from_utf8_lossy(&retry.body));
    assert_eq!(retry.json()["uploadKept"], false);
    let catalog = call(&h, "GET", "/api/v1/catalog", Some(&write)).await.json();
    assert_eq!(catalog["assets"].as_array().unwrap().len(), 2);

    // Nothing importable at all is an error status with the reasons attached.
    let upload = new_upload(&h, &write).await;
    let none = call_json(&h, "POST", &format!("/api/v1/uploads/{upload}/commit"), Some(&write), json!({"entries": [sky_entry("uid://ghost", "_import_ghost", "Ghost")]})).await;
    assert_eq!((none.status, none.code().as_str()), (StatusCode::UNPROCESSABLE_ENTITY, "import_failed"));
    assert_eq!(none.json()["errors"][0]["id"], "uid://ghost");
    assert_eq!(call_json(&h, "POST", &format!("/api/v1/uploads/{upload}/commit"), Some(&write), json!({"entries": []})).await.code(), "no_entries");
    assert_eq!(call_json(&h, "POST", "/api/v1/uploads/00000000000000000000000000000000/commit", Some(&write), json!({"entries": [{}]})).await.status, StatusCode::NOT_FOUND);
    let malformed = send(&h.app, build("POST", &format!("/api/v1/uploads/{upload}/commit"), Some(&write), Body::from("not json"), &[("content-type", "application/json")], "127.0.0.1")).await;
    assert_eq!((malformed.status, malformed.code().as_str()), (StatusCode::BAD_REQUEST, "bad_request"));
}

async fn put_bytes_owned(h: &Harness, path: String, token: String, bytes: Vec<u8>) -> Reply {
    put_bytes(h, &path, &token, bytes).await
}

#[tokio::test]
async fn a_library_changed_on_disk_is_noticed() {
    // Someone copies a finished library over the data folder while the server runs.
    let h = Harness::new(|_| {});
    let read = h.token("reader", Role::Read);
    let first = call(&h, "GET", "/api/v1/info", Some(&read)).await.json()["revision"].clone();
    let other = Harness::new(|_| {});
    let write = other.token("writer", Role::Write);
    assert_eq!(import_sky(&other, &write, "uid://copied", "Copied").await.status, StatusCode::OK);
    std::fs::remove_dir_all(h.library().join("library")).unwrap();
    copy_dir(&other.library().join("library"), &h.library().join("library"));
    std::fs::copy(other.library().join(toybox_core::config::LIBRARY_DB_REL), h.library().join(toybox_core::config::LIBRARY_DB_REL)).unwrap();
    let catalog = call(&h, "GET", "/api/v1/catalog", Some(&read)).await.json();
    assert_eq!(catalog["assets"][0]["id"], "uid://copied");
    assert_ne!(call(&h, "GET", "/api/v1/info", Some(&read)).await.json()["revision"], first);
}

#[tokio::test]
async fn a_library_copied_up_from_windows_with_a_half_finished_change_is_served() {
    // A library folder made on Windows: its storage journal names a Windows
    // cache, and a stale cache with a Windows root may sit beside it.
    let source = Harness::new(|_| {});
    let write = source.token("writer", Role::Write);
    assert_eq!(import_sky(&source, &write, "uid://winlib", "WinLib").await.status, StatusCode::OK);
    let h = Harness::new(|_| {});
    let read = h.token("reader", Role::Read);
    std::fs::remove_dir_all(h.library().join("library")).unwrap();
    copy_dir(&source.library().join("library"), &h.library().join("library"));
    std::fs::copy(source.library().join(toybox_core::config::LIBRARY_DB_REL), h.library().join(toybox_core::config::LIBRARY_DB_REL)).unwrap();
    let after: Value = toybox_core::infra::store::read_manifest(&source.library()).unwrap();
    let back = '\u{5c}';
    let foreign_cache = if cfg!(windows) {
        "/home/ry/.local/share/com.toybox.app/libraries/abc/catalog.json".to_string()
    } else {
        format!("C:{back}Users{back}ry{back}AppData{back}Roaming{back}com.toybox.app{back}libraries{back}abc{back}catalog.json")
    };
    let journal = json!({
        "manifest_before": {"assets": []}, "manifest_after": after,
        "cache_before": null, "cache_after": null, "cache_path": foreign_cache,
        "files": [], "legacy_dirs": [], "archive": "_library_config/migration-backups/storage-1/unindexed",
    });
    std::fs::write(h.library().join("_library_config/storage-pending.json"), serde_json::to_vec(&journal).unwrap()).unwrap();
    // The server's own cache says it was built for a folder this machine does not have.
    let stale = json!({"schemaVersion": 5, "libraryRoot": format!("Z:{back}nonexistent{back}Lib"), "assets": []});
    std::fs::create_dir_all(h.dir.join("cache")).unwrap();
    std::fs::write(h.state.library.handle.cache_path.clone(), serde_json::to_vec(&stale).unwrap()).unwrap();

    let catalog = call(&h, "GET", "/api/v1/catalog", Some(&read)).await;
    assert_eq!(catalog.status, StatusCode::OK, "{}", String::from_utf8_lossy(&catalog.body));
    let catalog = catalog.json();
    assert_eq!(catalog["assets"][0]["id"], "uid://winlib");
    assert!(!h.library().join("_library_config/storage-pending.json").exists(), "the journal was finished");
    let file = catalog["assets"][0]["files"][0].as_str().unwrap().to_string();
    let reply = call(&h, "GET", &format!("/api/v1/files/{file}"), Some(&read)).await;
    assert_eq!((reply.status, reply.body), (StatusCode::OK, SKY.to_vec()));
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn the_command_line_sets_up_and_reports() {
    let dir = std::env::temp_dir().join(format!("toybox-server-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let (code, _, err) = cli(&dir, &["status"]);
    assert_eq!(code, 1, "status before init fails");
    assert!(err.contains("init"), "{err}");

    let (code, out, err) = cli(&dir, &["init", "--name", "Friends"]);
    assert_eq!(code, 0, "{out}{err}");
    let admin = secret_in(&out);
    assert!(admin.starts_with("tbx_") && out.contains("ONLY THIS ONCE"));
    assert!(dir.join("library").join(toybox_core::config::LIBRARY_DB_REL).is_file());
    assert!(dir.join("cache").is_dir() && dir.join("uploads").is_dir());
    let stored = std::fs::read_to_string(dir.join("server.json")).unwrap();
    assert!(!stored.contains(&admin), "only the hash is stored");
    assert!(stored.contains("Friends"));

    let (code, _, err) = cli(&dir, &["init"]);
    assert_eq!(code, 1, "init twice is refused");
    assert!(err.contains("already set up"), "{err}");

    let (code, out, _) = cli(&dir, &["status", "--listen", "127.0.0.1:1"]);
    assert_eq!(code, 0);
    assert!(out.contains("Friends") && out.contains("1 admin") && out.contains("0 items") && out.contains("not running"), "{out}");

    assert_eq!(cli(&dir, &["token", "add", "--name", "x", "--role", "superuser"]).0, 2, "a wrong role is a usage error");
    let (code, _, err) = cli(&dir, &["token", "add", "--name", "admin", "--role", "read"]);
    assert_eq!(code, 1, "duplicate names are refused");
    assert!(err.contains("already exists"), "{err}");
    assert_eq!(cli(&dir, &["token", "revoke", "ghost"]).0, 1);
    assert_eq!(cli(&dir, &["serve", "--listen", "not-an-address"]).0, 1);

    // --data also comes from the environment.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_toybox-server")).args(["token", "list"]).env("TOYBOX_DATA", &dir).output().unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("admin"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_catalog_carries_content_fingerprints_from_the_moment_an_item_is_uploaded() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    // The same file under another id, name and pack is the same content; other bytes are not.
    assert_eq!(import_sky(&h, &write, "uid://sky_a", "Noon").await.status, StatusCode::OK);
    assert_eq!(import_sky(&h, &write, "uid://sky_b", "Midday_copy").await.status, StatusCode::OK);
    let upload = new_upload(&h, &write).await;
    let path = format!("/api/v1/uploads/{upload}/files/library/_import_skyc/skybox/Dusk/noon.hdr");
    assert_eq!(put_bytes(&h, &path, &write, b"different-radiance".to_vec()).await.status, StatusCode::OK);
    let commit = call_json(&h, "POST", &format!("/api/v1/uploads/{upload}/commit"), Some(&write), json!({"entries": [sky_entry("uid://sky_c", "_import_skyc", "Dusk")]})).await;
    assert_eq!(commit.status, StatusCode::OK);

    let catalog = call(&h, "GET", "/api/v1/catalog", Some(&write)).await.json();
    let hash_of = |id: &str| -> String {
        let asset = catalog["assets"].as_array().unwrap().iter().find(|a| a["id"] == id).unwrap_or_else(|| panic!("no {id}"));
        asset["contentHash"].as_str().unwrap_or_else(|| panic!("{id} has no contentHash: {asset}")).to_string()
    };
    let (a, b, c) = (hash_of("uid://sky_a"), hash_of("uid://sky_b"), hash_of("uid://sky_c"));
    assert!(a.len() == 64 && a.bytes().all(|x| x.is_ascii_hexdigit()));
    assert_eq!(a, b, "same bytes, other id and name");
    assert_ne!(a, c);
    // The side cache is the hashes table of the library database.
    let cache = toybox_core::infra::store::hashes::load(&h.library().join(toybox_core::config::LIBRARY_DB_REL));
    assert_eq!(cache.len(), 3);
    assert!(cache.keys().all(|key| key.starts_with("skybox/")));
    // Nothing of the manifest changed for it.
    let manifest = toybox_core::infra::store::read_manifest(&h.library()).unwrap().to_string();
    assert!(!manifest.contains("contentHash") && !manifest.contains(&a));
    // A restart serves them straight from the side cache.
    let restarted = h.restarted();
    let again = send(&restarted, build("GET", "/api/v1/catalog", Some(&h.admin), Body::empty(), &[], "127.0.0.1")).await.json();
    assert_eq!(again["assets"].as_array().unwrap().iter().filter(|a| a["contentHash"].is_string()).count(), 3);
}

#[tokio::test]
async fn fingerprints_added_in_the_background_change_the_catalog_etag_and_revision() {
    let mut h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    assert_eq!(import_sky(&h, &write, "uid://sky_a", "Noon").await.status, StatusCode::OK);
    assert_eq!(import_sky(&h, &write, "uid://sky_b", "Dusk").await.status, StatusCode::OK);
    // A library that was never fingerprinted: no side cache, a restart.
    toybox_core::infra::store::hashes::update(&h.library().join(toybox_core::config::LIBRARY_DB_REL), |map| map.clear()).unwrap();
    h.state = AppState::open(DataDir::new(&h.dir), Options { log_requests: false, ..Options::default() }).unwrap();
    h.app = toybox_server::api::router(h.state.clone());

    let first = call(&h, "GET", "/api/v1/catalog", Some(&write)).await;
    assert!(first.json()["assets"].as_array().unwrap().iter().all(|a| a.get("contentHash").is_none()), "nothing known yet: the old rule applies");
    let (etag, info) = (first.header("etag").to_string(), call(&h, "GET", "/api/v1/info", Some(&write)).await.json());
    assert_eq!(format!("\"{}\"", info["revision"].as_str().unwrap()), etag, "the revision is the ETag");

    let report = h.state.library.fingerprint_pass().await.unwrap();
    assert_eq!((report.hashed, report.failed), (2, 0));

    let stale = call_with(&h, "GET", "/api/v1/catalog", Some(&write), &[("if-none-match", &etag)]).await;
    assert_eq!(stale.status, StatusCode::OK, "a client holding the old ETag gets the new catalog");
    assert_ne!(stale.header("etag"), etag);
    assert!(stale.json()["assets"].as_array().unwrap().iter().all(|a| a["contentHash"].is_string()));
    let new_info = call(&h, "GET", "/api/v1/info", Some(&write)).await.json();
    assert_ne!(new_info["revision"], info["revision"]);
    assert_eq!(format!("\"{}\"", new_info["revision"].as_str().unwrap()), stale.header("etag"));
    // Holding the new one is a 304, and a second pass finds nothing to do.
    let fresh = call_with(&h, "GET", "/api/v1/catalog", Some(&write), &[("if-none-match", stale.header("etag"))]).await;
    assert_eq!(fresh.status, StatusCode::NOT_MODIFIED);
    assert_eq!(h.state.library.fingerprint_pass().await.unwrap().hashed, 0);
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&write)).await.json()["revision"], new_info["revision"]);
}

#[tokio::test]
async fn an_item_whose_files_are_gone_is_skipped_by_the_background_pass() {
    let mut h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    assert_eq!(import_sky(&h, &write, "uid://sky_a", "Noon").await.status, StatusCode::OK);
    assert_eq!(import_sky(&h, &write, "uid://sky_b", "Dusk").await.status, StatusCode::OK);
    let catalog = call(&h, "GET", "/api/v1/catalog", Some(&write)).await.json();
    let gone = catalog["assets"][0]["files"][0].as_str().unwrap().to_string();
    std::fs::remove_file(h.library().join(&gone)).unwrap();
    toybox_core::infra::store::hashes::update(&h.library().join(toybox_core::config::LIBRARY_DB_REL), |map| map.clear()).unwrap();
    h.state = AppState::open(DataDir::new(&h.dir), Options { log_requests: false, ..Options::default() }).unwrap();
    h.app = toybox_server::api::router(h.state.clone());
    let report = h.state.library.fingerprint_pass().await.unwrap();
    assert_eq!((report.hashed, report.failed), (1, 1));
    // It is not retried until the next start.
    assert_eq!(h.state.library.fingerprint_pass().await.unwrap(), Default::default());
    let served = call(&h, "GET", "/api/v1/catalog", Some(&write)).await.json();
    assert_eq!(served["assets"].as_array().unwrap().iter().filter(|a| a["contentHash"].is_string()).count(), 1);
}

// ------------------------------------------------------------- library tags

async fn import_tagged_sky(h: &Harness, token: &str) -> String {
    let upload = new_upload(h, token).await;
    let pack = "_import_tagged";
    let path = format!("/api/v1/uploads/{upload}/files/library/{pack}/skybox/Noon/noon.hdr");
    assert_eq!(put_bytes(h, &path, token, SKY.to_vec()).await.status, StatusCode::OK);
    let mut entry = sky_entry("uid://tagged", pack, "Noon");
    entry["tags"] = json!(["  Sky ", "NOON", "sky", "source: forged"]);
    entry["provenance"] = json!({"source": "Sky Pack", "license": "CC0", "filesystemPath": "P:/private/sky"});
    let reply = call_json(h, "POST", &format!("/api/v1/uploads/{upload}/commit"), Some(token), json!({"entries": [entry]})).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", String::from_utf8_lossy(&reply.body));
    reply.json()["revision"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn library_tags_are_served_to_everyone_and_edited_by_writers_only() {
    let h = Harness::new(|_| {});
    let read = h.token("reader", Role::Read);
    let write = h.token("writer", Role::Write);
    let admin = h.admin.clone();
    let revision = import_tagged_sky(&h, &write).await;

    // The catalog carries the normalised library tags at the item's top level;
    // `user.tags` still holds only the provenance-derived ones.
    let catalog = call(&h, "GET", "/api/v1/catalog", Some(&read)).await.json();
    let asset = &catalog["assets"][0];
    assert_eq!(asset["tags"], json!(["sky", "noon"]));
    assert_eq!(asset["user"]["tags"], json!(["source: Sky Pack", "license: CC0"]));
    assert!(asset["user"]["provenance"].get("filesystemPath").is_none() || asset["user"]["provenance"]["filesystemPath"].is_null());
    let storage_path = asset["files"][0].as_str().unwrap().to_string();

    let body = json!({"edits": [{"id": "uid://tagged", "tags": ["Footstep", "carpet", "loop"],
        "provenance": {"source": "Better Sky Pack", "url": "https://example.test/sky", "license": null}}]});

    // A read token cannot edit; nothing changes.
    let refused = call_json(&h, "POST", "/api/v1/assets/metadata", Some(&read), body.clone()).await;
    assert_eq!((refused.status, refused.code().as_str()), (StatusCode::FORBIDDEN, "forbidden"));
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&read)).await.json()["revision"], revision.as_str());
    // Bad requests are refused whole.
    for (edits, status, code) in [
        (json!({"edits": []}), StatusCode::BAD_REQUEST, "no_edits"),
        (json!({"edits": [{"id": "uid://tagged", "tags": ["fine"]}, {"id": "uid://nope", "tags": ["x"]}]}), StatusCode::NOT_FOUND, "not_found"),
        (json!({"edits": [{"id": "uid://tagged", "provenance": {"url": "ftp://example.test"}}]}), StatusCode::BAD_REQUEST, "invalid_metadata"),
        (json!({"edits": [{"id": "uid://tagged", "tag": ["misspelt"]}]}), StatusCode::BAD_REQUEST, "bad_request"),
        (json!({"edits": [{"id": "uid://tagged", "provenance": {"filesystemPath": "x"}}]}), StatusCode::BAD_REQUEST, "bad_request"),
    ] {
        let reply = call_json(&h, "POST", "/api/v1/assets/metadata", Some(&write), edits).await;
        assert_eq!((reply.status, reply.code().as_str()), (status, code), "{}", String::from_utf8_lossy(&reply.body));
    }
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&read)).await.json()["revision"], revision.as_str(), "refused edits change nothing");

    // Write and admin may.
    let edited = call_json(&h, "POST", "/api/v1/assets/metadata", Some(&write), body).await;
    assert_eq!(edited.status, StatusCode::OK, "{}", String::from_utf8_lossy(&edited.body));
    let reply = edited.json();
    assert_ne!(reply["revision"], revision.as_str());
    assert_eq!(reply["updated"], json!(["uid://tagged"]));
    assert_eq!(reply["assets"][0]["id"], "uid://tagged");
    assert_eq!(reply["assets"][0]["tags"], json!(["footstep", "carpet", "loop"]));
    assert_eq!(reply["assets"][0]["user"]["provenance"]["source"], "Better Sky Pack");
    assert_eq!(reply["assets"][0]["user"]["tags"], json!(["source: Better Sky Pack"]));
    assert_eq!(reply["assets"][0]["user"]["favorite"], false, "what is served has no personal metadata");

    // The new revision is what the catalog and info say; the ETag moved with it.
    let new_revision = reply["revision"].as_str().unwrap();
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&read)).await.json()["revision"], new_revision);
    let again = call(&h, "GET", "/api/v1/catalog", Some(&read)).await;
    assert_eq!(again.header("etag"), format!("\"{new_revision}\""));
    assert_eq!(again.json()["assets"][0]["tags"], json!(["footstep", "carpet", "loop"]));
    assert_eq!(call_with(&h, "GET", "/api/v1/catalog", Some(&read), &[("if-none-match", &format!("\"{revision}\""))]).await.status, StatusCode::OK);
    // The files did not move, and a restart serves the same thing.
    assert_eq!(again.json()["assets"][0]["files"][0].as_str().unwrap(), storage_path);
    assert_eq!(call(&h, "GET", &format!("/api/v1/files/{storage_path}"), Some(&read)).await.body, SKY);
    let restarted = send(&h.restarted(), build("GET", "/api/v1/catalog", Some(&admin), Body::empty(), &[], "127.0.0.1")).await;
    assert_eq!(restarted.json()["assets"][0]["tags"], json!(["footstep", "carpet", "loop"]));
    assert_eq!(restarted.header("etag"), format!("\"{new_revision}\""));

    let by_admin = call_json(&h, "POST", "/api/v1/assets/metadata", Some(&admin), json!({"edits": [{"id": "uid://tagged", "tags": []}]})).await;
    assert_eq!(by_admin.status, StatusCode::OK);
    assert!(call(&h, "GET", "/api/v1/catalog", Some(&read)).await.json()["assets"][0].get("tags").is_none());
}

fn count_named(dir: &Path, prefix: &str, suffix: &str) -> usize {
    std::fs::read_dir(dir).map(|read| read.flatten().filter(|e| { let n = e.file_name().to_string_lossy().into_owned(); n.starts_with(prefix) && n.ends_with(suffix) }).count()).unwrap_or(0)
}

#[tokio::test]
async fn safety_copies_are_bounded_and_deleting_leaves_no_empty_set_folders() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    let keep = toybox_core::infra::retention::KEEP_STORAGE_SNAPSHOTS;
    let ids: Vec<String> = (0..keep + 3).map(|n| format!("uid://bounded{n}")).collect();
    for (n, id) in ids.iter().enumerate() {
        let reply = import_sky(&h, &write, id, &format!("Sky{n}")).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", String::from_utf8_lossy(&reply.body));
    }
    let backups = h.library().join("_library_config/migration-backups");
    assert_eq!(count_named(&backups, "storage-", ""), keep, "N+3 uploads leave only N publication snapshots");

    for id in &ids {
        let deleted = call_json(&h, "POST", "/api/v1/assets/delete", Some(&write), json!({"ids": [id]})).await;
        assert_eq!(deleted.status, StatusCode::OK, "{}", String::from_utf8_lossy(&deleted.body));
    }
    let copies = toybox_core::infra::retention::KEEP_DELETE_CATALOGS;
    assert_eq!(count_named(&h.library().join("_library_config"), "catalog-before-delete-", ".json"), copies);
    assert_eq!(count_named(&h.dir.join("cache"), "catalog-before-delete-", ".json"), copies);
    let skyboxes = h.library().join("library/skybox");
    assert_eq!(std::fs::read_dir(&skyboxes).map(|r| r.count()).unwrap_or(0), 0, "every emptied set folder is gone");
}

#[tokio::test]
async fn opening_a_library_shrinks_old_piles_and_removes_existing_empty_set_folders() {
    // Piles left by earlier versions. The library is copied to a new folder first, so
    // that this process has not opened it yet (it tidies each library once per run).
    let made = Harness::new(|_| {});
    let dir = std::env::temp_dir().join(format!("toybox-server-test-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&made.dir, &dir);
    let library = dir.join("library");
    let backups = library.join("_library_config/migration-backups");
    for stamp in 1..=20u32 {
        std::fs::create_dir_all(backups.join(format!("storage-{stamp}"))).unwrap();
        std::fs::write(backups.join(format!("storage-{stamp}/manifest.before.json")), b"{}").unwrap();
    }
    let config = library.join("_library_config");
    for stamp in 1..=9u32 {
        std::fs::write(config.join(format!("catalog-before-delete-{stamp}.json")), b"{}").unwrap();
    }
    let empty = library.join("library/skybox").join("c".repeat(64));
    std::fs::create_dir_all(&empty).unwrap();
    let busy = library.join("library/skybox").join("d".repeat(64));
    std::fs::create_dir_all(&busy).unwrap();
    std::fs::write(busy.join("kept.hdr"), b"x").unwrap();

    let mut options = Options::default();
    options.log_requests = false;
    let state = AppState::open(DataDir::new(&dir), options).expect("open");
    let app = toybox_server::api::router(state.clone());
    let h = Harness { dir, state, app, admin: made.admin.clone() };
    let catalog = call(&h, "GET", "/api/v1/catalog", Some(&h.admin.clone())).await;
    assert_eq!(catalog.status, StatusCode::OK, "{}", String::from_utf8_lossy(&catalog.body));
    assert_eq!(count_named(&backups, "storage-", ""), toybox_core::infra::retention::KEEP_STORAGE_SNAPSHOTS);
    assert!(backups.join("storage-20").is_dir() && !backups.join("storage-1").exists() && !backups.join("storage-15").exists());
    assert_eq!(count_named(&config, "catalog-before-delete-", ".json"), toybox_core::infra::retention::KEEP_DELETE_CATALOGS);
    assert!(config.join("catalog-before-delete-9.json").exists() && !config.join("catalog-before-delete-4.json").exists());
    assert!(!empty.exists(), "the empty set folder was swept");
    assert!(busy.join("kept.hdr").exists(), "a folder with a file stays");
}

#[tokio::test]
async fn concurrent_writes_into_one_upload_keep_the_case_rule_and_every_file() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    let upload = new_upload(&h, &write).await;
    let base = format!("/api/v1/uploads/{upload}/files/library/_import_cc/skybox");

    // Many different files at once all arrive.
    let urls: Vec<String> = (0..60).map(|i| format!("{base}/many/f{i}.png")).collect();
    let puts: Vec<_> = urls.iter().enumerate().map(|(i, url)| put_bytes(&h, url, &write, vec![i as u8; 20])).collect();
    for reply in futures_util::future::join_all(puts).await {
        assert_eq!(reply.status, StatusCode::OK, "{}", String::from_utf8_lossy(&reply.body));
    }
    let staged = h.dir.join("uploads").join(&upload).join("files/library/_import_cc/skybox/many");
    assert_eq!(std::fs::read_dir(&staged).unwrap().count(), 60);

    // Two names that differ only in case, sent at the same moment: exactly one wins, in every round.
    for round in 0..20 {
        let (first, second) = (format!("{base}/race{round}/Tex.png"), format!("{base}/race{round}/tEX.png"));
        let (a, b) = tokio::join!(put_bytes(&h, &first, &write, b"a".to_vec()), put_bytes(&h, &second, &write, b"b".to_vec()));
        let mut statuses = [a.status, b.status];
        statuses.sort();
        assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT], "round {round}");
    }
    // The same name sent twice at once is two writes of one file.
    let same = format!("{base}/same/Tex.png");
    let (a, b) = tokio::join!(put_bytes(&h, &same, &write, b"a".to_vec()), put_bytes(&h, &same, &write, b"bb".to_vec()));
    assert_eq!((a.status, b.status), (StatusCode::OK, StatusCode::OK));
    // A refused write does not leave its name claimed: the other spelling is refused while the first stands.
    assert_eq!(put_bytes(&h, &format!("{base}/same/tex.png"), &write, b"c".to_vec()).await.status, StatusCode::CONFLICT);
    assert_eq!(put_bytes(&h, &format!("{base}/same/Tex.png"), &write, b"d".to_vec()).await.status, StatusCode::OK);
    // The upload is still in a state to commit (no write is left counted as running).
    assert_eq!(h.state.uploads.begin_commit(&upload).map_err(|e| e.code), Ok(()));
    h.state.uploads.end_commit(&upload);
}

#[tokio::test]
async fn a_client_that_hangs_up_during_a_commit_does_not_strand_the_upload_or_lose_the_import() {
    // One upload at a time, so a stuck upload would show as a refused POST /uploads.
    let h = Harness::new(|o| o.uploads.max_uploads = 1);
    let write = h.token("writer", Role::Write);
    let read = h.token("reader", Role::Read);
    let upload = new_upload(&h, &write).await;
    let count = 150;
    let mut entries = Vec::new();
    for i in 0..count {
        let name = format!("S{i}");
        let path = format!("/api/v1/uploads/{upload}/files/library/_import_gone/skybox/{name}/noon.hdr");
        assert_eq!(put_bytes(&h, &path, &write, SKY.to_vec()).await.status, StatusCode::OK);
        entries.push(sky_entry(&format!("uid://gone{i}"), "_import_gone", &name));
    }
    let request = build(
        "POST",
        &format!("/api/v1/uploads/{upload}/commit"),
        Some(&write),
        Body::from(serde_json::to_vec(&json!({ "entries": entries })).unwrap()),
        &[("content-type", "application/json")],
        "127.0.0.1",
    );
    use std::future::Future;
    let mut response = Box::pin(send(&h.app, request));
    // Drive the request until the commit is running, then hang up.
    let mut hung_up_mid_commit = false;
    for _ in 0..100_000 {
        let ready = std::future::poll_fn(|cx| std::task::Poll::Ready(response.as_mut().poll(cx).is_ready())).await;
        assert!(!ready, "the commit finished before the client could hang up; use a bigger batch");
        if matches!(h.state.uploads.begin_file(&upload, "library/_import_probe/x/y"), Err(e) if e.code == "upload_busy") {
            hung_up_mid_commit = true;
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(hung_up_mid_commit, "the commit never started");
    drop(response);

    // The work still runs to its end: the upload is released and removed, the items are served.
    let mut waited = 0;
    while h.state.uploads.count() > 0 {
        assert!(waited < 600, "the upload is stuck after the client hung up");
        tokio::time::sleep(Duration::from_millis(50)).await;
        waited += 1;
    }
    // The folder goes right after the upload leaves the list.
    for _ in 0..100 {
        if !h.dir.join("uploads").join(&upload).exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!h.dir.join("uploads").join(&upload).exists(), "its folder is gone");
    let catalog = call(&h, "GET", "/api/v1/catalog", Some(&read)).await.json();
    assert_eq!(catalog["assets"].as_array().unwrap().len(), count);
    // A new upload can be made (the only slot is free again) and another write is not blocked.
    let again = new_upload(&h, &write).await;
    assert_eq!(call(&h, "DELETE", &format!("/api/v1/uploads/{again}"), Some(&write)).await.status, StatusCode::NO_CONTENT);
    let one_more = import_sky(&h, &write, "uid://after", "After").await;
    assert_eq!(one_more.status, StatusCode::OK, "{}", String::from_utf8_lossy(&one_more.body));
}

// ------------------------------------------------------- the revision feed

const V2: (&str, &str) = ("x-toybox-api", "2");

async fn v2_json(h: &Harness, path: &str, token: &str, body: Value) -> Reply {
    let request = build("POST", path, Some(token), Body::from(serde_json::to_vec(&body).unwrap()), &[("content-type", "application/json"), V2], "127.0.0.1");
    send(&h.app, request).await
}

async fn v2_get(h: &Harness, path: &str, token: &str) -> Reply {
    call_with(h, "GET", path, Some(token), &[V2]).await
}

fn ids_of(assets: &Value) -> Vec<String> {
    let mut ids: Vec<String> = assets.as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap().to_string()).collect();
    ids.sort();
    ids
}

/// Commit one more sky and return the (version 2) reply.
async fn v2_import_sky(h: &Harness, token: &str, id: &str, name: &str) -> Value {
    let pack = format!("_import_{}", id.replace(|c: char| !c.is_ascii_alphanumeric(), ""));
    let upload = new_upload(h, token).await;
    let path = format!("/api/v1/uploads/{upload}/files/library/{pack}/skybox/{name}/noon.hdr");
    assert_eq!(put_bytes(h, &path, token, format!("radiance-{name}").into_bytes()).await.status, StatusCode::OK);
    let reply = v2_json(h, &format!("/api/v1/uploads/{upload}/commit"), token, json!({"entries": [sky_entry(id, &pack, name)]})).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", String::from_utf8_lossy(&reply.body));
    reply.json()
}

#[tokio::test]
async fn info_names_the_database_and_a_number_for_clients_that_follow_the_feed() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    let old = call(&h, "GET", "/api/v1/info", Some(&write)).await.json();
    let new = v2_get(&h, "/api/v1/info", &write).await.json();
    assert!(old["revision"].is_string(), "a client that sends no version gets the string it always had");
    let start = new["revision"].as_i64().unwrap();
    assert_eq!(old["dbId"], new["dbId"]);
    assert!(!new["dbId"].as_str().unwrap().is_empty());
    assert_eq!((new["apiVersion"].clone(), new["thumbRevision"].clone()), (json!(2), json!(0)));
    v2_import_sky(&h, &write, "uid://sky1", "Noon").await;
    assert_eq!(v2_get(&h, "/api/v1/info", &write).await.json()["revision"], start + 1);
}

#[tokio::test]
async fn paging_through_the_feed_from_zero_gives_the_catalog() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    for (id, name) in [("uid://s1", "A"), ("uid://s2", "B"), ("uid://s3", "C"), ("uid://s4", "D"), ("uid://s5", "E")] {
        v2_import_sky(&h, &write, id, name).await;
    }
    let catalog = call(&h, "GET", "/api/v1/catalog", Some(&write)).await.json();
    let mut served: Vec<Value> = catalog["assets"].as_array().unwrap().clone();
    served.sort_by_key(|a| a["id"].as_str().unwrap().to_string());

    let (mut since, mut after) = (0, None::<String>);
    let (mut assets, mut pages): (Vec<Value>, i64) = (Vec::new(), 0);
    let db_id = v2_get(&h, "/api/v1/info", &write).await.json()["dbId"].as_str().unwrap().to_string();
    loop {
        let mut path = format!("/api/v1/changes?since={since}&limit=2&dbId={db_id}");
        if let Some(cursor) = &after {
            path.push_str(&format!("&after={}", cursor.replace(':', "%3A").replace('/', "%2F")));
        }
        let page = call(&h, "GET", &path, Some(&write)).await.json();
        pages += 1;
        assert_eq!(page["reset"], false);
        assets.extend(page["assets"].as_array().unwrap().iter().cloned());
        assert!(page["deleted"].as_array().unwrap().is_empty());
        since = page["next"].as_i64().unwrap();
        after = page["after"].as_str().map(String::from);
        if !page["more"].as_bool().unwrap() {
            assert_eq!(since, page["revision"].as_i64().unwrap());
            break;
        }
    }
    assert_eq!(pages, 3, "five rows in pages of two");
    assets.sort_by_key(|a| a["id"].as_str().unwrap().to_string());
    assert_eq!(assets, served, "the feed from 0 is the catalog, asset for asset (fingerprints included)");
    assert!(assets.iter().all(|a| a["contentHash"].is_string()));
}

#[tokio::test]
async fn an_import_an_edit_and_a_delete_appear_as_only_the_rows_they_touch() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    for (id, name) in [("uid://s1", "A"), ("uid://s2", "B"), ("uid://s3", "C")] {
        v2_import_sky(&h, &write, id, name).await;
    }
    let base = v2_get(&h, "/api/v1/info", &write).await.json()["revision"].as_i64().unwrap();

    // Import: the reply is the change.
    let reply = v2_import_sky(&h, &write, "uid://s4", "D").await;
    assert_eq!((reply["base"].as_i64().unwrap(), reply["revision"].as_i64().unwrap()), (base, base + 1));
    assert_eq!(ids_of(&reply["changes"]["assets"]), ["uid://s4"]);
    assert!(reply["changes"]["deleted"].as_array().unwrap().is_empty());
    assert_eq!(reply["changes"]["more"], false);
    assert_eq!(reply["changes"]["next"], reply["revision"]);
    // It is what the feed says too.
    let feed = call(&h, "GET", &format!("/api/v1/changes?since={base}"), Some(&write)).await.json();
    assert_eq!(ids_of(&feed["assets"]), ["uid://s4"]);

    // Edit.
    let edit = v2_json(&h, "/api/v1/assets/metadata", &write, json!({"edits": [{"id": "uid://s2", "tags": ["Footstep"]}]})).await;
    assert_eq!(edit.status, StatusCode::OK, "{}", String::from_utf8_lossy(&edit.body));
    let edit = edit.json();
    assert_eq!(edit["revision"].as_i64().unwrap(), base + 2);
    assert_eq!((edit["base"].clone(), edit["updated"].clone(), edit["touched"].clone()), (json!(base + 1), json!(["uid://s2"]), json!(["uid://s2"])));
    assert_eq!(ids_of(&edit["changes"]["assets"]), ["uid://s2"]);
    assert_eq!(edit["changes"]["assets"][0]["tags"], json!(["footstep"]));
    assert!(edit.get("assets").is_none(), "the rows are in changes");

    // Delete.
    let deleted = v2_json(&h, "/api/v1/assets/delete", &write, json!({"ids": ["uid://s1"]})).await;
    assert_eq!(deleted.status, StatusCode::OK);
    let deleted = deleted.json();
    assert_eq!((deleted["deleted"].clone(), deleted["revision"].as_i64().unwrap()), (json!(["uid://s1"]), base + 3));
    assert!(deleted.get("catalog").is_none(), "no catalog in a version 2 reply");
    assert_eq!(deleted["changes"]["deleted"], json!(["uid://s1"]));
    assert!(deleted["changes"]["assets"].as_array().unwrap().is_empty());

    // Everything since the first import, from the feed: the latest state of each row touched since.
    let feed = call(&h, "GET", &format!("/api/v1/changes?since={base}"), Some(&write)).await.json();
    assert_eq!(ids_of(&feed["assets"]), ["uid://s2", "uid://s4"]);
    assert_eq!(feed["deleted"], json!(["uid://s1"]));
}

#[tokio::test]
async fn a_cursor_the_server_cannot_answer_for_is_reset() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    v2_import_sky(&h, &write, "uid://s1", "A").await;
    v2_import_sky(&h, &write, "uid://s2", "B").await;
    let info = v2_get(&h, "/api/v1/info", &write).await.json();
    let (db_id, before) = (info["dbId"].as_str().unwrap().to_string(), info["revision"].as_i64().unwrap());
    let deleted = v2_json(&h, "/api/v1/assets/delete", &write, json!({"ids": ["uid://s1"]})).await.json();
    let at = before + 1;
    assert_eq!(deleted["revision"], at);

    let ok = call(&h, "GET", &format!("/api/v1/changes?since={before}&dbId={db_id}"), Some(&write)).await.json();
    assert_eq!((ok["reset"].clone(), ok["deleted"].clone()), (json!(false), json!(["uid://s1"])));
    // Unknown database: the replica it describes is not this library.
    let foreign = call(&h, "GET", &format!("/api/v1/changes?since={before}&dbId=not-this-one"), Some(&write)).await.json();
    assert_eq!(foreign["reset"], true);
    assert_eq!(foreign["dbId"].as_str().unwrap(), db_id);
    assert!(foreign["assets"].as_array().unwrap().is_empty() && foreign["deleted"].as_array().unwrap().is_empty());
    // Ahead of the server.
    assert_eq!(call(&h, "GET", &format!("/api/v1/changes?since={}", at + 50), Some(&write)).await.json()["reset"], true);
    // Older than the deletions the server still remembers.
    toybox_core::infra::store::Store::for_handle(&h.state.library.handle).prune_changes(0).unwrap();
    let old = call(&h, "GET", &format!("/api/v1/changes?since={before}&dbId={db_id}"), Some(&write)).await.json();
    assert_eq!(old["reset"], true, "the deletion is no longer remembered");
    assert_eq!(call(&h, "GET", &format!("/api/v1/changes?since={at}"), Some(&write)).await.json()["reset"], false);
    // A full download never needs a reset.
    let full = call(&h, "GET", "/api/v1/changes?since=0", Some(&write)).await.json();
    assert_eq!((full["reset"].clone(), ids_of(&full["assets"])), (json!(false), vec!["uid://s2".to_string()]));
    // Bad queries are refused plainly.
    assert_eq!(call(&h, "GET", "/api/v1/changes?since=-1", Some(&write)).await.code(), "bad_query");
    assert_eq!(call(&h, "GET", "/api/v1/changes?since=x", Some(&write)).await.code(), "bad_query");
    assert_eq!(call(&h, "GET", "/api/v1/changes?since=1&after=garbage", Some(&write)).await.code(), "bad_cursor");
}

#[tokio::test]
async fn pictures_are_listed_since_a_thumbnail_revision() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    v2_import_sky(&h, &write, "uid://s1", "A").await;
    v2_import_sky(&h, &write, "uid://s2", "B").await;
    let png: Vec<u8> = [&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a][..], b"body"].concat();
    assert_eq!(put_bytes(&h, "/api/v1/thumbs/uid%3A%2F%2Fs1", &write, png.clone()).await.status, StatusCode::NO_CONTENT);
    let library_revision = v2_get(&h, "/api/v1/info", &write).await.json()["revision"].clone();
    let list = call(&h, "GET", "/api/v1/thumbs?since=0", Some(&write)).await.json();
    assert_eq!((list["thumbRevision"].clone(), list["thumbs"][0]["id"].clone()), (json!(1), json!("uid://s1")));
    assert_eq!(put_bytes(&h, "/api/v1/thumbs/uid%3A%2F%2Fs2", &write, png.clone()).await.status, StatusCode::NO_CONTENT);
    let list = call(&h, "GET", "/api/v1/thumbs?since=1", Some(&write)).await.json();
    assert_eq!((list["thumbRevision"].clone(), list["thumbs"].as_array().unwrap().len(), list["thumbs"][0]["id"].clone()), (json!(2), 1, json!("uid://s2")));
    assert_eq!(call(&h, "GET", "/api/v1/thumbs?since=2", Some(&write)).await.json()["thumbs"], json!([]));
    // The old listing is still all of them, and the library revision never moved.
    let all = call(&h, "GET", "/api/v1/thumbs", Some(&write)).await.json();
    assert_eq!((all["thumbs"].as_array().unwrap().len(), all["thumbRevision"].clone()), (2, json!(2)));
    assert!(all["revision"].is_string());
    assert_eq!(v2_get(&h, "/api/v1/info", &write).await.json()["revision"], library_revision);
}

#[tokio::test]
async fn an_old_client_reads_and_writes_through_the_catalog_as_before() {
    let h = Harness::new(|_| {});
    let write = h.token("writer", Role::Write);
    // No version header: string revisions, the catalog in the delete answer, items in the edit answer.
    let committed = import_sky(&h, &write, "uid://old1", "Noon").await.json();
    assert!(committed["revision"].is_string() && committed.get("changes").is_none() && committed.get("base").is_none());
    let catalog = call(&h, "GET", "/api/v1/catalog", Some(&write)).await;
    assert_eq!(catalog.header("etag"), format!("\"{}\"", committed["revision"].as_str().unwrap()));
    assert_eq!(catalog.json()["assets"][0]["id"], "uid://old1");
    let edited = call_json(&h, "POST", "/api/v1/assets/metadata", Some(&write), json!({"edits": [{"id": "uid://old1", "tags": ["a"]}]})).await.json();
    assert_eq!((edited["assets"][0]["id"].clone(), edited["assets"][0]["tags"].clone()), (json!("uid://old1"), json!(["a"])));
    assert!(edited["revision"].is_string());
    let deleted = call_json(&h, "POST", "/api/v1/assets/delete", Some(&write), json!({"ids": ["uid://old1"]})).await.json();
    assert_eq!(deleted["catalog"]["assets"].as_array().unwrap().len(), 0);
    assert_eq!(deleted["revision"], call(&h, "GET", "/api/v1/info", Some(&write)).await.json()["revision"]);
}

#[tokio::test]
async fn info_names_the_window_of_clients_it_serves() {
    let h = Harness::new(|_| {});
    let info = call(&h, "GET", "/api/v1/info", Some(&h.admin)).await.json();
    assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(info["apiVersion"], 2);
    assert_eq!(info["minApiVersion"], 1, "a server serves its own level and the one before");
    assert_eq!(info["minClientVersion"], "0.1.0");
}

#[tokio::test]
async fn a_client_below_the_window_gets_426_with_the_versions_and_others_are_served() {
    let h = Harness::new(|options| options.min_api_version = Some(2));
    // Without a token nothing is learned: authentication comes first.
    assert_eq!(call(&h, "GET", "/api/v1/catalog", None).await.status, StatusCode::UNAUTHORIZED);
    // A client that sends no level (level 1) is below a window that starts at 2.
    let old = call_with(&h, "GET", "/api/v1/catalog", Some(&h.admin), &[("x-toybox-client", "toybox/0.3.0")]).await;
    assert_eq!(old.status, StatusCode::UPGRADE_REQUIRED);
    let body = old.json();
    assert_eq!(body["error"]["code"], "client_too_old");
    assert_eq!(body["serverVersion"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["minApiVersion"], 2);
    assert_eq!(body["minClientVersion"], "0.4.1");
    assert!(body["error"]["message"].as_str().unwrap().contains("Update Toybox to 0.4.1"));
    // The first call of an old client, /info, says the same.
    assert_eq!(call(&h, "GET", "/api/v1/info", Some(&h.admin)).await.status, StatusCode::UPGRADE_REQUIRED);
    // A client in the window is served, and the health check stays open to everyone.
    let ok = call_with(&h, "GET", "/api/v1/info", Some(&h.admin), &[("x-toybox-api", "2"), ("x-toybox-client", "toybox/0.4.1")]).await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(call(&h, "GET", "/api/v1/health", None).await.status, StatusCode::OK);
    // Level 2 but a stated version older than the window starts is refused as well.
    let stale = call_with(&h, "GET", "/api/v1/info", Some(&h.admin), &[("x-toybox-api", "2"), ("x-toybox-client", "tbx/0.2.0")]).await;
    assert_eq!(stale.status, StatusCode::UPGRADE_REQUIRED);
}

#[tokio::test]
async fn a_level_one_client_is_still_served_by_this_server() {
    let h = Harness::new(|_| {});
    let reply = call(&h, "GET", "/api/v1/catalog", Some(&h.admin)).await;
    assert_eq!(reply.status, StatusCode::OK);
}
