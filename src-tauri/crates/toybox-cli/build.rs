//! Embeds the compiled plugin SDK (`packages/toybox-sdk/dist/**/*.js`) so that
//! `@ldlework/toybox-sdk` resolves inside the binary, with no files on disk at run time.
//! `TOYBOX_SDK_DIST` overrides the folder.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn collect(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, base, out);
        } else if path.extension().is_some_and(|e| e == "js") {
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, path));
        }
    }
}

pub fn main() {
    println!("cargo:rerun-if-env-changed=TOYBOX_SDK_DIST");
    println!("cargo:rerun-if-changed=build.rs");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let repo = manifest.join("../../..");
    let dist = std::env::var_os("TOYBOX_SDK_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("packages/toybox-sdk/dist"));
    println!("cargo:rerun-if-changed={}", dist.display());
    if !dist.join("index.js").is_file() {
        eprintln!(
            "error: the plugin SDK is not built ({} has no index.js); run `node_modules/.bin/tsc -p packages/toybox-sdk/tsconfig.json` from the repo root (or set TOYBOX_SDK_DIST)",
            dist.display()
        );
        std::process::exit(1);
    }
    let dist = dist.canonicalize().unwrap_or(dist);
    let mut files = Vec::new();
    collect(&dist, &dist, &mut files);
    let mut code = String::from("/// `(path relative to the SDK dist folder, source)` of every compiled SDK module.\npub const SDK_FILES: &[(&str, &str)] = &[\n");
    for (rel, path) in files {
        let abs = path.to_string_lossy().replace('\\', "/");
        let abs = abs.strip_prefix("//?/").unwrap_or(&abs);
        writeln!(code, "    ({rel:?}, include_str!({abs:?})),").unwrap();
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("sdk_files.rs");
    std::fs::write(out, code).unwrap();
}
