//! Shared by `tests/plugin_*.rs`: a temporary home with a local library, the real compiled
//! bundled plugins installed into it, and generators for tiny valid source files.
#![allow(dead_code)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use toybox_core::ops::browse::ItemRef;
use toybox_host::{Host, HostPaths};
use toybox_remote::secrets::MemoryStore;

#[path = "../../src/plugin_runtime/mod.rs"]
pub mod plugin_runtime;

use plugin_runtime::{Event, PluginRuntime};

pub const REPO: &str = r"D:\dev\toybox";

/// A folder under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(label: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("tbx-plugin-test-{label}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub struct Env {
    pub tmp: TempDir,
    pub host: Arc<Host>,
    pub library: String,
    pub library_root: PathBuf,
    pub events: Rc<RefCell<Vec<String>>>,
}

impl Env {
    pub fn work(&self, name: &str) -> PathBuf {
        let dir = self.tmp.path().join("work").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Load the runtime; every event is recorded in `events` (`print: ...`, `log: ...`, ...).
    pub fn runtime(&self) -> PluginRuntime {
        let log = self.events.clone();
        PluginRuntime::load(
            self.host.clone(),
            Box::new(move |event| {
                log.borrow_mut().push(match event {
                    Event::Print(t) => format!("print: {t}"),
                    Event::Log(t) => format!("log: {t}"),
                    Event::Warn(t) => format!("warn: {t}"),
                    Event::Console(t) => format!("console: {t}"),
                })
            }),
        )
        .expect("the runtime loads")
    }

    /// Add permissions to the installed copy of a plugin's manifest (the repository is not touched).
    pub fn grant(&self, plugin: &str, permissions: &[&str]) {
        let path = self.tmp.path().join("home").join("plugins").join(plugin).join("manifest.json");
        let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for permission in permissions {
            manifest["permissions"][*permission] = serde_json::json!(true);
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    }

    /// Install a small plugin written for a test: its manifest and files (relative path, text).
    pub fn install(&self, id: &str, manifest: serde_json::Value, files: &[(&str, &str)]) {
        let dir = self.tmp.path().join("home").join("plugins").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let mut manifest = manifest;
        manifest["id"] = serde_json::json!(id);
        manifest["name"] = serde_json::json!(id);
        manifest["version"] = serde_json::json!("1.0.0");
        std::fs::write(dir.join("manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
        for (rel, text) in files {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        self.no_real_programs(id);
    }

    /// Point every `file` setting of an installed plugin (the programs it may run) at a program that does
    /// not exist, so no test can ever start a real external program.
    pub fn no_real_programs(&self, id: &str) {
        let manifest = self.host.plugin_manifest(id).expect("the plugin is installed");
        for setting in manifest["settings"].as_array().into_iter().flatten() {
            if let (Some("file"), Some(key)) = (setting["type"].as_str(), setting["key"].as_str()) {
                let missing = self.tmp.path().join("no-such-program").join("program.exe");
                self.host.set_plugin_setting(id, key, serde_json::json!(missing.to_string_lossy())).unwrap();
            }
        }
    }

    /// The library's items by raw id (what the host answers a query for them with).
    pub fn items(&self, ids: &[String]) -> Vec<serde_json::Value> {
        let refs: Vec<ItemRef> = ids.iter().map(|id| ItemRef { library: self.library.clone(), id: id.clone() }).collect();
        self.host.get_assets(&refs).unwrap().into_iter().map(|row| serde_json::to_value(row.asset).unwrap()).collect()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        // Programs a plugin launched (`services.tools.launch`) are the caller's to stop.
        self.host.tools.stop_all();
    }
}

/// A fresh home with a local library and these bundled plugins installed (copied from
/// `plugins/<id>`: the manifest and the compiled folder). `None`, with a message, when a plugin is not built.
pub fn setup(plugins: &[&str]) -> Option<Env> {
    let tmp = TempDir::new("home");
    let home = tmp.path().join("home");
    for id in plugins {
        let source = Path::new(REPO).join("plugins").join(id);
        if !source.join("dist").is_dir() || !source.join("manifest.json").is_file() {
            eprintln!("skipped: {} is not built ({} has no dist folder)", id, source.display());
            return None;
        }
        let target = home.join("plugins").join(id);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::copy(source.join("manifest.json"), target.join("manifest.json")).unwrap();
        copy_dir(&source.join("dist"), &target.join("dist"));
        if source.join("icons").is_dir() {
            copy_dir(&source.join("icons"), &target.join("icons"));
        }
    }
    let host = Arc::new(Host::open_with_secrets(HostPaths::for_home(&home), Arc::new(MemoryStore::new()), None).unwrap());
    let library_root = tmp.path().join("library");
    let info = host.create_library(&library_root, Some("Test library")).unwrap();
    let env = Env { tmp, host, library: info.id, library_root, events: Rc::default() };
    for id in plugins {
        env.no_real_programs(id);
    }
    Some(env)
}

// --- generated sources ----------------------------------------------------------------------------------

/// A mono 16-bit PCM WAV of `frames` frames of a quiet ramp.
pub fn wav(frames: u32, rate: u32) -> Vec<u8> {
    let data_len = frames * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        out.extend_from_slice(&(((i % 100) as i16 - 50) * 100).to_le_bytes());
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

/// An RGB8 PNG (stored, uncompressed deflate) of `width` x `height` filled with `color`.
pub fn png(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
    let mut raw = Vec::new();
    for _ in 0..height {
        raw.push(0);
        for _ in 0..width {
            raw.extend_from_slice(&color);
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    while let Some(block) = blocks.next() {
        z.push(if blocks.peek().is_none() { 1 } else { 0 });
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &z));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// A one-triangle glTF (`<stem>.gltf` + `<stem>.bin`) into `dir`; with `animated`, a node animation
/// ("Move") that translates the triangle.
pub fn write_gltf(dir: &Path, stem: &str, animated: bool) {
    let mut bin: Vec<u8> = Vec::new();
    for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let mut doc = serde_json::json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "name": "Tri", "mesh": 0 }],
        "meshes": [{ "name": "Tri", "primitives": [{ "attributes": { "POSITION": 0 } }] }],
        "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0] }],
        "bufferViews": [{ "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 }],
        "buffers": [{ "uri": format!("{stem}.bin"), "byteLength": 36 }]
    });
    if animated {
        for v in [0.0f32, 1.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        for v in [0.0f32, 0.0, 0.0, 0.0, 2.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        doc["accessors"].as_array_mut().unwrap().extend([
            serde_json::json!({ "bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0], "max": [1] }),
            serde_json::json!({ "bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC3" }),
        ]);
        doc["bufferViews"].as_array_mut().unwrap().extend([
            serde_json::json!({ "buffer": 0, "byteOffset": 36, "byteLength": 8 }),
            serde_json::json!({ "buffer": 0, "byteOffset": 44, "byteLength": 24 }),
        ]);
        doc["buffers"][0]["byteLength"] = serde_json::json!(bin.len());
        doc["animations"] = serde_json::json!([{
            "name": "Move",
            "samplers": [{ "input": 1, "output": 2, "interpolation": "LINEAR" }],
            "channels": [{ "sampler": 0, "target": { "node": 0, "path": "translation" } }]
        }]);
    }
    std::fs::write(dir.join(format!("{stem}.gltf")), serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
    std::fs::write(dir.join(format!("{stem}.bin")), bin).unwrap();
}
