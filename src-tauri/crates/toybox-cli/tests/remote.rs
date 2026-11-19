//! Remote libraries through `tbx`, against a real `toybox-server` running in this process on a
//! loopback port. Tokens are kept in a file named by `TOYBOX_SECRETS`, never in the system
//! credential store.

mod common;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use common::*;
use serde_json::{json, Value};
use toybox_server::config::Role;
use toybox_server::data::{self, DataDir};
use toybox_server::state::{AppState, Options};

struct Server {
    dir: PathBuf,
    url: String,
    admin: String,
    state: AppState,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn start(parent: &std::path::Path) -> Server {
        let dir = parent.join("server");
        std::fs::create_dir_all(&dir).unwrap();
        let data = DataDir::new(&dir);
        let admin = data::init(&data, "Test server").expect("init").secret;
        let mut options = Options::default();
        options.fail_delay = Duration::from_millis(1);
        options.log_requests = false;
        options.lockout.max_failures = 10_000;
        let state = AppState::open(DataDir::new(&dir), options).expect("open");
        let app = toybox_server::api::router(state.clone());
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
                addr_tx.send(listener.local_addr().unwrap()).unwrap();
                tokio::select! {
                    _ = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()) => {}
                    _ = stop_rx => {}
                }
            });
            runtime.shutdown_background();
        });
        let addr = addr_rx.recv().expect("server address");
        let server = Server { dir, url: format!("http://{addr}"), admin, state, stop: Some(stop_tx), thread: Some(thread) };
        server.seed();
        server
    }

    fn token(&self, name: &str, role: Role) -> String {
        self.state.config.mutate(|config| config.add_token(name, role)).unwrap().1
    }

    /// Two decals and a model on the server.
    fn seed(&self) {
        let handle = DataDir::new(&self.dir).handle();
        let mut entries = Vec::new();
        for (n, (id, name)) in [("uid://srv-crate", "ServerCrate"), ("uid://srv-lamp", "ServerLamp")].into_iter().enumerate() {
            put(&handle.root, &format!("library/_import_s/decal/{name}/a.png"), &png(n + 100));
            entries.push(json!({ "id": id, "kind": "plugin", "type": "decal", "pack": "_import_s", "category": "decals", "name": name, "files": ["a.png"], "meta": {}, "format": "png", "tags": ["server"] }));
        }
        toybox_core::ops::import::merge_entries(&handle, entries).expect("seed the server library");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn with_token(world: &World, token: &str, args: &[&str]) -> Run {
    let mut command = world.command(args);
    command.env("TOYBOX_TOKEN", token);
    world.run(command, None)
}

#[test]
fn a_remote_library_can_be_connected_read_and_forgotten() {
    let world = World::new();
    let server = Server::start(world.dir.path());
    // The token is never an argument: without one (and no terminal) there is nothing to connect with.
    world.tbx(&["library", "connect", &server.url]).fails_with(2);
    // A wrong token is refused, and the message does not repeat it.
    let wrong = with_token(&world, "tbx_wrong_token_value", &["library", "connect", &server.url]).fails_with(1);
    assert!(!wrong.err.contains("tbx_wrong_token_value"), "{}", wrong.err);
    assert_eq!(world.tbx(&["--json", "library", "list"]).ok().json()["libraries"], json!([]));

    let connected = with_token(&world, &server.admin, &["--json", "library", "connect", &server.url, "--name", "Studio"]).ok().json();
    assert_eq!(connected["library"]["kind"], "remote");
    assert_eq!(connected["library"]["name"], "Studio");
    assert_eq!(connected["library"]["location"], server.url.as_str());
    assert!(std::fs::read_to_string(world.secrets_file()).unwrap().contains(&server.admin), "the token is in the credential store");
    assert!(!std::fs::read_to_string(world.home().join("settings.json")).unwrap().contains(&server.admin), "and never in settings");

    let info = world.tbx(&["--json", "library", "info"]).ok().json();
    assert_eq!(info["items"], 2);
    assert_eq!(info["remote"]["role"], "admin");
    assert_eq!(info["remote"]["serverName"], "Test server");
    assert!(world.tbx(&["library", "info"]).ok().out.contains("server    Test server"));

    assert_eq!(world.tbx(&["search", "--ids"]).ok().lines(), vec!["uid://srv-crate", "uid://srv-lamp"]);
    let shown = world.tbx(&["--json", "show", "uid://srv-crate"]).ok().json();
    assert_eq!(shown["items"][0]["asset"]["name"], "ServerCrate");
    let list = world.tbx(&["library", "list"]).ok();
    assert!(list.out.contains("remote") && list.out.contains(&server.url), "{}", list.out);

    // Files of a remote library are downloaded when a path is asked for.
    let path = world.tbx(&["path", "uid://srv-crate"]).ok();
    assert!(PathBuf::from(path.out.trim()).is_file(), "{}", path.out);
    let size = world.tbx(&["--json", "library", "cache", "size"]).ok().json()["bytes"].as_u64().unwrap();
    assert!(size > 0);
    let cleared = world.tbx(&["--json", "library", "cache", "clear"]).ok().json();
    assert_eq!(cleared["freedBytes"].as_u64().unwrap(), size);
    assert_eq!(world.tbx(&["--json", "library", "cache", "size"]).ok().json()["bytes"], 0);

    // The scan fetches what changed; a remote library has no folder to check files in.
    assert_eq!(world.tbx(&["--json", "library", "scan"]).ok().json()["items"], 2);
    assert_eq!(world.tbx(&["--json", "library", "check"]).ok().json()["ok"], true);

    // Forgetting it removes the token from the credential store; the server is untouched.
    world.tbx(&["--yes", "library", "remove", "Studio"]).ok();
    assert!(!std::fs::read_to_string(world.secrets_file()).unwrap().contains(&server.admin));
    assert_eq!(world.tbx(&["--json", "library", "list"]).ok().json()["libraries"], json!([]));
    assert_eq!(server.state.config.snapshot().unwrap().tokens.len(), 1);
}

#[test]
fn tokens_are_listed_created_revoked_and_replaced() {
    let world = World::new();
    let server = Server::start(world.dir.path());
    with_token(&world, &server.admin, &["library", "connect", &server.url, "--name", "Studio"]).ok();
    let created = world.tbx(&["--json", "library", "token", "create", "--name", "ci", "--role", "read"]).ok().json();
    let secret = created["token"].as_str().unwrap().to_string();
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["role"], "read");
    // The secret is printed once on standard output, alone.
    let text = world.tbx(&["library", "token", "create", "--name", "ci2", "--role", "write"]).ok();
    assert_eq!(text.lines().len(), 1);
    assert!(text.lines()[0].len() > 16);
    let list = world.tbx(&["--json", "library", "token", "list"]).ok().json();
    assert!(list["tokens"].as_array().unwrap().iter().any(|token| token["name"] == "ci"));
    assert!(!list.to_string().contains(&secret), "a list never shows secrets");
    world.tbx(&["library", "token", "create", "--name", "x", "--role", "owner"]).fails_with(2);

    // `login` replaces the token with a read-only one, which cannot write.
    let stdin = world.tbx_stdin(&["library", "login", "Studio", "--token-stdin"], &format!("{secret}\n")).ok();
    assert!(stdin.out.contains("access token") || stdin.out.is_empty(), "{}", stdin.out);
    assert_eq!(world.tbx(&["--json", "library", "info"]).ok().json()["remote"]["role"], "read");
    let refused = world.tbx(&["--yes", "delete", "uid://srv-lamp"]).fails_with(1);
    assert!(refused.err.contains("cannot be changed"), "{}", refused.err);
    world.tbx(&["library", "token", "list"]).fails_with(1);
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "2");
    // Back to the admin token: revoke the read-only one.
    world.tbx_stdin(&["library", "login", "Studio", "--token-stdin"], &format!("{}\n", server.admin)).ok();
    world.tbx(&["library", "token", "revoke", &id]).ok();
    let list = world.tbx(&["--json", "library", "token", "list"]).ok().json();
    assert!(!list["tokens"].as_array().unwrap().iter().any(|token| token["id"] == id.as_str()));
    let _ = server.token("unused", Role::Read);
}

#[test]
fn items_are_copied_between_a_local_and_a_remote_library() {
    let world = World::new();
    let server = Server::start(world.dir.path());
    let (_, _) = world.stocked_library("Main");
    with_token(&world, &server.admin, &["library", "connect", &server.url, "--name", "Studio"]).ok();
    // Down: remote to local.
    let down = world.tbx(&["--json", "-L", "Studio", "copy", "uid://srv-crate", "--to", "Main"]).ok().json();
    assert_eq!(down["copied"], 1);
    assert_eq!(world.tbx(&["-L", "Main", "count"]).ok().out.trim(), "6");
    // Up: local to remote.
    let up = world.tbx(&["--json", "-L", "Main", "copy", "uid://lamp", "--to", "Studio"]).ok().json();
    assert_eq!(up["copied"], 1, "{up}");
    assert_eq!(world.tbx(&["-L", "Studio", "count"]).ok().out.trim(), "3");
    // The server's own library has it too.
    let handle = DataDir::new(&server.dir).handle();
    let ids: Vec<String> = toybox_core::infra::store::Store::for_handle(&handle).asset_id_page("", 100).unwrap().0;
    assert!(ids.contains(&"uid://lamp".to_string()), "{ids:?}");
    // Edits go through the server.
    world.tbx(&["-L", "Studio", "tag", "add", "remote-tag", "uid://srv-crate"]).ok();
    let shown: Value = world.tbx(&["--json", "-L", "Studio", "show", "uid://srv-crate"]).ok().json();
    assert!(shown["items"][0]["asset"]["tags"].as_array().unwrap().iter().any(|tag| tag == "remote-tag"), "{shown}");
    // And a delete on the remote library is the server's delete.
    world.tbx(&["--yes", "-L", "Studio", "delete", "uid://srv-lamp"]).ok();
    assert_eq!(world.tbx(&["-L", "Studio", "count"]).ok().out.trim(), "2");
}

#[test]
fn an_unreachable_remote_library_is_reported_and_not_fatal_elsewhere() {
    let world = World::new();
    let (_, _) = world.stocked_library("Main");
    let server = Server::start(world.dir.path());
    with_token(&world, &server.admin, &["library", "connect", &server.url, "--name", "Studio"]).ok();
    // Fetch it once, so there is something to fall back to.
    let before = world.tbx(&["-L", "Studio", "count"]).ok();
    assert_eq!(before.out.trim(), "2");
    drop(server);
    // The local library is unaffected; the remote one falls back to what was fetched.
    assert_eq!(world.tbx(&["-L", "Main", "count"]).ok().out.trim(), "5");
    let status = world.tbx(&["--json", "status"]).ok().json();
    assert_eq!(status["libraries"].as_array().unwrap().len(), 2);
    assert_eq!(world.tbx(&["-L", "Studio", "count"]).ok().out.trim(), "2");
    assert_eq!(world.tbx(&["--json", "library", "list"]).ok().json()["libraries"][1]["kind"], "remote");
}

#[test]
fn the_real_audio_importer_imports_into_a_remote_library_and_the_item_is_found() {
    let world = World::new();
    let server = Server::start(world.dir.path());
    if !world.link_repo_plugin("com.toybox.audio-core") {
        return;
    }
    with_token(&world, &server.admin, &["library", "connect", &server.url, "--name", "Studio"]).ok();
    let sounds = world.path("sounds");
    std::fs::create_dir_all(&sounds).unwrap();
    std::fs::write(sounds.join("click.wav"), wav(3)).unwrap();
    std::fs::write(sounds.join("whoosh.wav"), wav(9)).unwrap();
    let run = world.tbx(&["-L", "Studio", "import", "audio", sounds.to_str().unwrap(), "--category", "ui", "--tag", "sfx", "--source", "Test Pack"]).ok();
    let imported = run.lines();
    assert_eq!(imported.len(), 2, "{} / {}", run.out, run.err);
    // The item is on the server, and `search` finds it with the ids the import printed.
    let found = world.tbx(&["-L", "Studio", "search", "click", "--type", "audio", "--ids"]).ok();
    assert_eq!(found.lines().len(), 1, "{}", found.out);
    assert!(imported.contains(&found.lines()[0]), "{imported:?} vs {:?}", found.lines());
    let shown = world.tbx(&["--json", "show", &found.lines()[0]]).ok().json();
    assert_eq!(shown["items"][0]["asset"]["category"], "ui");
    assert_eq!(shown["items"][0]["asset"]["origin"]["plugin"], "com.toybox.audio-core");
    let handle = DataDir::new(&server.dir).handle();
    let on_server = toybox_core::infra::store::Store::for_handle(&handle).asset_id_page("", 100).unwrap().0;
    assert_eq!(on_server.len(), 4, "two seeded decals and two sounds: {on_server:?}");
    assert_eq!(world.tbx(&["-L", "Studio", "count", "--type", "audio"]).ok().out.trim(), "2");
}
