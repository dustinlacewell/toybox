//! Shared test world: a real `toybox-server` running in process on a loopback
//! port with a temporary data folder, and a client side (app data, file cache,
//! registry, in-memory credential store) in another temporary folder.
//! Every folder is unique to its test (process id + counter) and only that
//! folder is ever removed.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use toybox_core::domain::catalog_model::Catalog;
use toybox_core::infra::library::RemoteRole;
use toybox_core::infra::registry::LibraryHandle;
use toybox_core::infra::settings::AppSettings;
use toybox_remote::hub::{prepare, Hub};
use toybox_remote::progress::Progress;
use toybox_remote::secrets::MemoryStore;
use toybox_remote::RemoteLibrary;
use toybox_server::config::Role;
use toybox_server::data::{self, DataDir};
use toybox_server::state::{AppState, Options};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub fn unique_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("toybox-remote-test-{tag}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub struct TestServer {
    pub dir: PathBuf,
    pub url: String,
    pub addr: SocketAddr,
    pub admin: String,
    pub state: AppState,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Leave the data folder alone when this is dropped (a benchmark's prepared library).
    keep: bool,
}

impl TestServer {
    pub fn start() -> TestServer {
        Self::boot(unique_dir("server"), true, "127.0.0.1:0", String::new())
    }

    /// Serve a data folder that is already there (a benchmark's generated library),
    /// setting up its `server.json` if it has none. The folder is left alone afterwards.
    pub fn at(dir: PathBuf) -> TestServer {
        let data = DataDir::new(&dir);
        let admin = if data.is_initialised() {
            String::new()
        } else {
            let mut config = toybox_server::config::ServerConfig::new("Bench server");
            let (_, secret) = config.add_token("admin", Role::Admin).unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            config.save(&data.server_json()).unwrap();
            secret
        };
        let mut server = Self::boot(dir, false, "127.0.0.1:0", admin);
        server.keep = true;
        server
    }

    /// Start again on the data folder and port of a server (which is stopped first).
    pub fn restart(mut self) -> TestServer {
        let dir = std::mem::take(&mut self.dir);
        let admin = self.admin.clone();
        let addr = self.addr.to_string();
        self.shutdown();
        drop(self);
        // The port may take a moment to come free.
        for _ in 0..100 {
            if std::net::TcpListener::bind(&addr).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Self::boot(dir, false, &addr, admin)
    }

    fn boot(dir: PathBuf, init: bool, bind: &str, admin: String) -> TestServer {
        let data = DataDir::new(&dir);
        let admin = if init { data::init(&data, "Test server").expect("init").secret } else { admin };
        let mut options = Options::default();
        options.fail_delay = Duration::from_millis(1);
        options.log_requests = false;
        options.lockout.max_failures = 10_000;
        let state = AppState::open(DataDir::new(&dir), options).expect("open");
        let app = toybox_server::api::router(state.clone());
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let bind = bind.to_string();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind(&bind).await.expect("bind");
                addr_tx.send(listener.local_addr().unwrap()).unwrap();
                tokio::select! {
                    _ = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()) => {}
                    _ = stop_rx => {}
                }
            });
            runtime.shutdown_background();
        });
        let addr = addr_rx.recv().expect("server address");
        TestServer { dir, url: format!("http://{addr}"), addr, admin, state, stop: Some(stop_tx), thread: Some(thread), keep: false }
    }

    /// Stop answering (open connections are dropped too). The data stays.
    pub fn shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    pub fn token(&self, name: &str, role: Role) -> String {
        self.state.config.mutate(|config| config.add_token(name, role)).unwrap().1
    }

    pub fn handle(&self) -> LibraryHandle {
        DataDir::new(&self.dir).handle()
    }

    pub fn library_root(&self) -> PathBuf {
        self.dir.join("library")
    }

    /// The catalog document exactly as a client would get it.
    pub fn catalog(&self) -> Catalog {
        toybox_core::ops::scan::scan_library_catalog(&self.handle()).unwrap()
    }

    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.catalog().assets.iter().map(|a| a.id.clone()).collect();
        ids.sort();
        ids
    }

    /// Put files and entries into the served library the way an import does.
    pub fn import(&self, files: &[(&str, Vec<u8>)], entries: Vec<Value>) {
        let handle = self.handle();
        for (rel, bytes) in files {
            let path = handle.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        toybox_core::ops::import::merge_entries(&handle, entries).expect("seed the server library");
    }

    /// A small library: two models (one with a variant and kept originals) sharing a
    /// texture, a material, a sound and a plugin-defined skybox.
    pub fn seed(&self) {
        let (files, entries) = standard_content("s");
        self.import(&files, entries);
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown();
        if !self.keep && !self.dir.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

pub fn wav() -> Vec<u8> {
    let data = 200u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(8000u32.to_le_bytes());
    bytes.extend(16000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data.to_le_bytes());
    bytes.extend(vec![0u8; data as usize]);
    bytes
}

pub fn png() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend(b"not really an image, but the signature is right");
    bytes
}

pub fn scene(bin: &str, images: &[&str]) -> Vec<u8> {
    let images: Vec<Value> = images.iter().map(|uri| json!({"uri": uri})).collect();
    serde_json::to_vec(&json!({"asset":{"version":"2.0"},"buffers":[{"uri":bin,"byteLength":4}],"images":images})).unwrap()
}

/// Files and manifest entries for the standard fixture, staged under `_import_<tag>`.
pub fn standard_content(tag: &str) -> (Vec<(&'static str, Vec<u8>)>, Vec<Value>) {
    let pack = format!("_import_{tag}");
    let leak = |rel: String| -> &'static str { Box::leak(rel.into_boxed_str()) };
    let mut files: Vec<(&'static str, Vec<u8>)> = Vec::new();
    for (stem, original) in [("Crate", "full original"), ("Crate_LOD1", "low original")] {
        files.push((leak(format!("library/{pack}/props/{stem}.gltf")), scene(&format!("{stem}.bin"), &["wood.png", "metal.png"])));
        files.push((leak(format!("library/{pack}/props/{stem}.bin")), stem.as_bytes().to_vec()));
        files.push((leak(format!("library/{pack}/props/{stem}.obj")), original.as_bytes().to_vec()));
    }
    files.push((leak(format!("library/{pack}/props/Barrel.gltf")), scene("Barrel.bin", &["wood.png"])));
    files.push((leak(format!("library/{pack}/props/Barrel.bin")), b"barrel".to_vec()));
    files.push((leak(format!("library/{pack}/props/wood.png")), png()));
    files.push((leak(format!("library/{pack}/props/metal.png")), [png(), b"metal".to_vec()].concat()));
    for name in ["Stone", "Stone_1K"] {
        files.push((leak(format!("library/{pack}/materials/{name}/color.png")), [png(), name.as_bytes().to_vec()].concat()));
    }
    files.push((leak(format!("library/{pack}/materials/Stone/stone.sbsar")), b"archive".to_vec()));
    files.push((leak(format!("library/{pack}/sounds/click.wav")), wav()));
    files.push((leak(format!("library/{pack}/skybox/Night/px.png")), png()));
    files.push((leak(format!("library/{pack}/skybox/Night/nx.png")), png()));
    let origin = |stem: &str| json!({"plugin":"com.example.obj","files":[format!("{stem}.obj")],"meta":{"format":"obj"}});
    let entries = vec![
        json!({"id":"uid://crate","pack":pack,"category":"props","file":"Crate.gltf","format":"obj","origin":origin("Crate"),
            "provenance":{"source":"Crate Pack","license":"CC0"},"tags":["Wooden Crate","prop"],
            "variants":[{"id":"uid://crate_lod","pack":pack,"category":"props","file":"Crate_LOD1.gltf","format":"obj","origin":origin("Crate_LOD1"),"tags":["low detail"]}]}),
        json!({"id":"uid://barrel","pack":pack,"category":"props","file":"Barrel.gltf"}),
        json!({"id":"uid://stone","kind":"material","pack":pack,"category":"stone","name":"Stone","maps":[{"role":"albedo","file":"color.png"}],
            "origin":{"plugin":"com.example.sbsar","files":["stone.sbsar"],"meta":{"preset":"Green"}},
            "variants":[{"id":"uid://stone_small","kind":"material","pack":pack,"category":"stone","name":"Stone_1K","maps":[{"role":"albedo","file":"color.png"}]}]}),
        json!({"id":"uid://click","kind":"audio","pack":pack,"category":"sounds","file":"click.wav","tags":["click","ui"],"provenance":{"publisher":"Sound Co","url":"https://example.test/clicks"}}),
        json!({"id":"uid://night","kind":"plugin","type":"skybox","pack":pack,"category":"night","name":"Night","files":["px.png","nx.png"],"meta":{"layout":"cubemap"}}),
    ];
    (files, entries)
}

/// The client side: app data, file cache root, a registry and an in-memory
/// credential store.
pub struct World {
    pub dir: PathBuf,
    pub app_data: PathBuf,
    pub hub: Hub,
    pub store: Arc<MemoryStore>,
    pub settings: AppSettings,
    pub events: Arc<Mutex<Vec<Progress>>>,
}

impl World {
    pub fn new(tag: &str) -> World {
        let dir = unique_dir(tag);
        let app_data = dir.join("app-data");
        let cache = dir.join("app-cache").join("libraries");
        std::fs::create_dir_all(&app_data).unwrap();
        let store = Arc::new(MemoryStore::new());
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        let sink: toybox_remote::progress::Sink = Arc::new(move |progress: &Progress| sink_events.lock().unwrap().push(progress.clone()));
        let hub = Hub::new(store.clone(), app_data.clone(), cache, Some(sink));
        World { dir, app_data, hub, store, settings: AppSettings::default(), events }
    }

    pub fn settings_path(&self) -> PathBuf {
        self.app_data.join("settings.json")
    }

    /// Add the server as a remote library, as the app does, and open it.
    pub fn add(&mut self, server: &TestServer, token: &str, name: &str) -> Arc<RemoteLibrary> {
        let prepared = prepare(&server.url, token, Some(name)).expect("connect");
        let entry = self.hub.register(&mut self.settings, &prepared, token).expect("register");
        toybox_core::infra::settings::save_to(&self.settings_path(), &self.settings).unwrap();
        self.hub.get(&entry).expect("open")
    }

    /// Add and read the catalog once, so the library is as it is after start-up.
    pub fn add_loaded(&mut self, server: &TestServer, token: &str, name: &str) -> Arc<RemoteLibrary> {
        let library = self.add(server, token, name);
        library.connect();
        library.refresh_catalog(false).expect("first catalog");
        library
    }

    /// A local library in the client's own folder, with a handle in this world's app data.
    pub fn local(&self, id: &str) -> LibraryHandle {
        let root = self.dir.join(format!("local-{id}"));
        toybox_core::infra::library::ensure_library_folder(&root).unwrap();
        let root = root.canonicalize().unwrap();
        LibraryHandle::local(&self.app_data, &toybox_core::infra::registry::LibraryEntry::local(id, id, &root.to_string_lossy()))
    }

    pub fn progress(&self) -> Vec<Progress> {
        self.events.lock().unwrap().clone()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn role_of(library: &RemoteLibrary) -> RemoteRole {
    library.role()
}

pub fn exists(path: &Path) -> bool {
    path.exists()
}
