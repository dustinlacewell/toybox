//! Shared test world: a temporary `--home` (never a real app-data folder), temporary
//! library folders, the built `tbx` binary, and an in-process host over the same home to
//! put items into libraries. Tokens never touch the system credential store: `tbx` is run
//! with `TOYBOX_SECRETS=memory:<file>` and the in-process host uses a memory store.

#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde_json::{json, Value};
use toybox_host::{Host, HostPaths};
use toybox_remote::secrets::MemoryStore;

pub struct Run {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Run {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.out).unwrap_or_else(|error| panic!("stdout is not JSON ({error}):\n{}\nstderr:\n{}", self.out, self.err))
    }

    pub fn lines(&self) -> Vec<String> {
        self.out.lines().map(str::to_string).collect()
    }

    #[track_caller]
    pub fn ok(self) -> Run {
        assert_eq!(self.code, 0, "expected success\nstdout:\n{}\nstderr:\n{}", self.out, self.err);
        self
    }

    #[track_caller]
    pub fn fails_with(self, code: i32) -> Run {
        assert_eq!(self.code, code, "expected exit {code}\nstdout:\n{}\nstderr:\n{}", self.out, self.err);
        self
    }
}

pub struct World {
    pub dir: tempfile::TempDir,
}

pub struct Seed {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    pub tags: Vec<&'static str>,
    pub source: Option<&'static str>,
}

#[derive(Clone, Copy)]
pub enum Kind {
    Decal,
    Model,
    Audio,
}

pub fn seed(id: &'static str, name: &'static str, kind: Kind, tags: &[&'static str], source: Option<&'static str>) -> Seed {
    Seed { id, name, kind, tags: tags.to_vec(), source }
}

impl World {
    pub fn new() -> World {
        World { dir: tempfile::tempdir().expect("temp dir") }
    }

    pub fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub fn secrets_file(&self) -> PathBuf {
        self.dir.path().join("secrets.json")
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tbx"));
        command
            .arg("--home")
            .arg(self.home())
            .args(args)
            .env("TOYBOX_SECRETS", format!("memory:{}", self.secrets_file().display()))
            .env_remove("TOYBOX_LIBRARY")
            .env_remove("TOYBOX_HOME")
            .env_remove("TOYBOX_APP_ID")
            .env_remove("TOYBOX_TOKEN")
            .env("NO_COLOR", "1");
        command
    }

    pub fn tbx(&self, args: &[&str]) -> Run {
        self.run(self.command(args), None)
    }

    pub fn tbx_stdin(&self, args: &[&str], stdin: &str) -> Run {
        self.run(self.command(args), Some(stdin))
    }

    pub fn run(&self, mut command: Command, stdin: Option<&str>) -> Run {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
        let mut child = command.spawn().expect("run tbx");
        if let Some(text) = stdin {
            child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
        }
        let output = child.wait_with_output().expect("wait for tbx");
        Run { code: output.status.code().unwrap_or(-1), out: String::from_utf8_lossy(&output.stdout).into_owned(), err: String::from_utf8_lossy(&output.stderr).into_owned() }
    }

    /// An in-process host over this world's home.
    pub fn host(&self) -> Host {
        Host::open_with_secrets(HostPaths::for_home(self.home()), Arc::new(MemoryStore::new()), None).expect("open host")
    }

    /// Make a library with `tbx` and return its id and folder.
    pub fn library(&self, name: &str) -> (String, PathBuf) {
        let root = self.path(&format!("lib-{name}"));
        let run = self.tbx(&["--json", "library", "create", root.to_str().unwrap(), "--name", name]).ok();
        (run.json()["library"]["id"].as_str().unwrap().to_string(), root)
    }

    /// Put items into a library the way an importer's commit does.
    pub fn add_items(&self, library: &str, root: &Path, items: &[Seed]) {
        let host = self.host();
        let mut entries = Vec::new();
        for (n, item) in items.iter().enumerate() {
            let n = item.name.bytes().fold(1usize, |acc, byte| acc.wrapping_mul(31).wrapping_add(byte as usize)) % 100_000 + n;
            let provenance = item.source.map(|source| json!({ "source": source })).unwrap_or(Value::Null);
            let mut entry = match item.kind {
                Kind::Decal => {
                    put(root, &format!("library/_import_t/decal/{}/a.png", item.name), &png(n + 1));
                    json!({ "id": item.id, "kind": "plugin", "type": "decal", "pack": "_import_t", "category": "decals", "name": item.name, "files": ["a.png"], "meta": {}, "format": "png" })
                }
                Kind::Audio => {
                    put(root, &format!("library/_import_t/sfx/{}.wav", item.name), &wav(n + 1));
                    json!({ "id": item.id, "kind": "audio", "pack": "_import_t", "category": "sfx", "file": format!("{}.wav", item.name), "format": "wav" })
                }
                Kind::Model => {
                    let scene = json!({
                        "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0]}],
                        "nodes": [{"name": "Root", "mesh": 0}],
                        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
                        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]}],
                        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}],
                        "buffers": [{"uri": format!("{}.bin", item.name), "byteLength": 36}]
                    });
                    put(root, &format!("library/_import_t/props/{}.gltf", item.name), &serde_json::to_vec(&scene).unwrap());
                    put(root, &format!("library/_import_t/props/{}.bin", item.name), &[n as u8; 36]);
                    json!({ "id": item.id, "pack": "_import_t", "category": "props", "file": format!("{}.gltf", item.name), "format": "gltf" })
                }
            };
            entry["tags"] = json!(item.tags);
            if !provenance.is_null() {
                entry["provenance"] = provenance;
            }
            entries.push(entry);
        }
        host.merge_seed_entries(library, entries).unwrap_or_else(|error| panic!("could not add items: {error}"));
    }

    /// A library with a few items: returns its id and folder.
    pub fn stocked_library(&self, name: &str) -> (String, PathBuf) {
        let (id, root) = self.library(name);
        self.add_items(
            &id,
            &root,
            &[
                seed("uid://crate", "Crate", Kind::Decal, &["wood", "prop"], Some("Pack A")),
                seed("uid://barrel", "Barrel", Kind::Decal, &["wood"], Some("Pack B")),
                seed("uid://lamp", "Lamp", Kind::Decal, &["metal"], None),
                seed("uid://hut", "Hut", Kind::Model, &["building"], Some("Pack A")),
                seed("uid://beep", "Beep", Kind::Audio, &["ui"], None),
            ],
        );
        (id, root)
    }

    /// A plugin folder that declares the `decal` type (no code).
    pub fn plugin_fixture(&self, id: &str) -> PathBuf {
        let dir = self.path(&format!("fixture-{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "id": id, "name": "Decals", "version": "1.2.3", "description": "Decals for tests",
                "assetTypes": [{ "id": "decal", "label": "Decals" }],
                "settings": [{ "key": "engine", "type": "text", "label": "Engine", "default": "none" }]
            }))
            .unwrap(),
        )
        .unwrap();
        dir
    }
}

pub fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

pub fn png(n: usize) -> Vec<u8> {
    // A 1x1 PNG whose pixel depends on n, so no two items have the same content.
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc = crc32(&[kind.as_slice(), data].concat());
        crc ^= 0;
        out.extend_from_slice(&crc.to_be_bytes());
        out
    };
    let mut header = Vec::new();
    header.extend_from_slice(&1u32.to_be_bytes());
    header.extend_from_slice(&1u32.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    bytes.extend(chunk(b"IHDR", &header));
    // zlib stream with one stored block: filter byte 0 and an RGB pixel.
    let raw = [0u8, (n % 251) as u8, ((n / 251) % 251) as u8, 7];
    let mut z = vec![0x78, 0x01, 0x01, raw.len() as u8, 0, !(raw.len() as u8), 0xff];
    z.extend_from_slice(&raw);
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    bytes.extend(chunk(b"IDAT", &z));
    bytes.extend(chunk(b"IEND", &[]));
    bytes
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

pub fn wav(n: usize) -> Vec<u8> {
    let samples: Vec<i16> = (0..128).map(|i| (((i * (n % 50 + 1)) % 200) as i16 - 100) * 100).collect();
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// A bundled plugin of this repository that has been compiled (`dist` exists), or `None` with a message.
pub fn repo_plugin(id: &str) -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins").join(id);
    if dir.join("dist").is_dir() && dir.join("manifest.json").is_file() {
        Some(dir)
    } else {
        eprintln!("skipped: {id} is not built ({} has no dist folder)", dir.display());
        None
    }
}

impl World {
    /// Install a compiled bundled plugin by linking its folder (the repository is not touched).
    pub fn link_repo_plugin(&self, id: &str) -> bool {
        let Some(dir) = repo_plugin(id) else { return false };
        self.tbx(&["plugin", "install", dir.to_str().unwrap(), "--link"]).ok();
        true
    }
}
