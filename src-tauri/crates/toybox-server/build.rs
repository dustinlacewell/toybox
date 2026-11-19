//! Compiles the release signing key and the default update address into the server.
//!
//! Both come from `src-tauri/tauri.conf.json` (`plugins.updater`), the same key the app's updater
//! trusts: the server verifies its own updates against it and there is no run-time way to replace it
//! in a release build.

fn main() {
    let conf = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tauri.conf.json");
    println!("cargo:rerun-if-changed={}", conf.display());
    println!("cargo:rerun-if-changed=build.rs");
    let text = std::fs::read_to_string(&conf).unwrap_or_else(|e| panic!("cannot read {}: {e}", conf.display()));
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", conf.display()));
    let updater = &doc["plugins"]["updater"];
    let pubkey = updater["pubkey"].as_str().unwrap_or("").trim().to_string();
    // https://github.com/<owner>/<repo>/releases/latest/download/latest.json -> .../toybox-server.json
    let endpoint = updater["endpoints"][0].as_str().unwrap_or("");
    let url = match endpoint.rfind('/') {
        Some(at) if endpoint.contains("/releases/") => format!("{}/toybox-server.json", &endpoint[..at]),
        _ => String::new(),
    };
    println!("cargo:rustc-env=TOYBOX_UPDATE_PUBKEY={pubkey}");
    println!("cargo:rustc-env=TOYBOX_UPDATE_URL={url}");
    println!("cargo:rustc-env=TOYBOX_TARGET={}", std::env::var("TARGET").unwrap_or_default());
}
