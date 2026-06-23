//! Shelling the user-supplied FBX2glTF executable to convert a `.fbx` source into
//! loose glTF. We ship no binary and trust the path the user configured: this is
//! a local single-user tool. The only contract pinned here is FBX2glTF's CLI —
//! `-i <input> -o <output-base>` writes loose `.gltf` + `.bin` + sibling textures
//! (no `-b`, the default). Build variants differ on whether the output lands as
//! `<base>.gltf` siblings or inside a `<base>/` subdir, so we *discover* the
//! produced `.gltf` rather than assume its exact path.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{AppError, AppResult};

/// Convert `src_fbx` with the user's `exe`, writing output under `out_dir` with
/// base name `stem`. Returns the path to the produced `.gltf`. Errors if the
/// process fails to spawn, exits non-zero, or emits no `.gltf`.
pub fn convert_fbx(exe: &Path, src_fbx: &Path, out_dir: &Path, stem: &str) -> AppResult<PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    let out_base = out_dir.join(stem);

    let output = Command::new(exe)
        .arg("-i")
        .arg(src_fbx)
        .arg("-o")
        .arg(&out_base)
        .output()
        .map_err(|e| AppError::msg(format!("failed to run FBX2glTF: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::msg(format!(
            "FBX2glTF failed ({}): {}",
            output.status,
            stderr.trim()
        )));
    }

    find_gltf(out_dir)
        .ok_or_else(|| AppError::msg("FBX2glTF produced no .gltf output".to_string()))
}

/// Find the first `.gltf` anywhere under `dir` (siblings or a `<base>/` subdir).
fn find_gltf(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("gltf") {
            return Some(path);
        }
        if path.is_dir() {
            subdirs.push(path);
        }
    }
    subdirs.iter().find_map(|d| find_gltf(d))
}
