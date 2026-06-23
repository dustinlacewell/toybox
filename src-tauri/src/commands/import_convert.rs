//! `convert_to_gltf`: the importer's FBX bridge. Shells the user-configured
//! FBX2glTF to turn one `.fbx` source into loose glTF, then lands the produced
//! fileset under `library/<pack>/<category>/` so the existing index step ingests
//! it unchanged. Conversion happens in a temp dir first, so a failed run never
//! leaves half-files in the library tree.

use std::path::Path;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::infra::{converter, fsio, library, settings};

/// Convert `src_path` (an `.fbx`) into `library/<pack>/<category>/`, naming the
/// output `<stem>`. Returns the library-relative paths written (the `.gltf`
/// first), for the importer to build a seed entry from. Errors with a clear
/// message if no converter is configured.
#[tauri::command]
pub async fn convert_to_gltf(
    app: AppHandle,
    src_path: String,
    pack: String,
    category: String,
    stem: String,
) -> AppResult<Vec<String>> {
    let library_root = library::resolve(&app)?;
    let exe = settings::load(&app)?
        .fbx2gltf_path
        .ok_or_else(|| AppError::msg("no FBX2glTF configured".to_string()))?;

    // Convert into an isolated temp dir, then promote the output into the library.
    let work = app
        .path()
        .temp_dir()
        .map_err(|e| AppError::msg(format!("temp_dir: {e}")))?
        .join(format!("toybox_convert_{stem}"));
    let _ = std::fs::remove_dir_all(&work);

    let gltf = converter::convert_fbx(Path::new(&exe), Path::new(&src_path), &work, &stem);
    let result = gltf.and_then(|gltf| {
        let dest_rel = format!("library/{pack}/{category}");
        let dest_abs = library_root.join(&dest_rel);
        promote(&gltf, &dest_abs, &dest_rel, &stem)
    });

    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Move FBX2glTF's output into the library, normalizing it to the `<stem>` the
/// catalog assumes. FBX2glTF names its buffer `buffer.bin` (uri `"buffer.bin"`),
/// but `entry_from_parts` derives the catalog's bin as `<stem>.bin` from the gltf
/// basename — so without this rename the bin the catalog records wouldn't exist,
/// breaking the viewer and every exporter. We rewrite `buffers[0].uri` to
/// `<stem>.bin`, write the buffer there, and copy textures verbatim. Returns the
/// library-relative paths written, `.gltf` first.
pub fn promote(gltf: &Path, dest_abs: &Path, dest_rel: &str, stem: &str) -> AppResult<Vec<String>> {
    let produced_dir = gltf
        .parent()
        .ok_or_else(|| AppError::msg("converted gltf has no parent dir".to_string()))?;

    let mut doc: Value = serde_json::from_str(&fsio::read_text(gltf)?)?;
    let buffers = doc
        .get("buffers")
        .and_then(|b| b.as_array())
        .filter(|b| b.len() == 1)
        .ok_or_else(|| AppError::msg("converted gltf is not single-buffer".to_string()))?;
    let src_bin_uri = buffers[0]
        .get("uri")
        .and_then(|u| u.as_str())
        .ok_or_else(|| AppError::msg("converted gltf buffer has no uri".to_string()))?
        .to_string();

    let mut written = Vec::new();

    // The buffer: copy under the canonical <stem>.bin and point the gltf at it.
    let bin_src = produced_dir.join(&src_bin_uri);
    let bin_name = format!("{stem}.bin");
    fsio::write_bytes(&dest_abs.join(&bin_name), &fsio::read_bytes(&bin_src)?)?;
    doc["buffers"][0]["uri"] = Value::String(bin_name.clone());
    written.push(format!("{dest_rel}/{bin_name}"));

    // The normalized gltf as <stem>.gltf.
    let gltf_name = format!("{stem}.gltf");
    fsio::write_text(&dest_abs.join(&gltf_name), &serde_json::to_string_pretty(&doc)?)?;
    written.insert(0, format!("{dest_rel}/{gltf_name}"));

    // Everything else FBX2glTF produced (textures) — verbatim, skipping the two
    // files we already normalized.
    copy_rest(produced_dir, dest_abs, dest_rel, &src_bin_uri, &mut written)?;
    Ok(written)
}

/// Copy every file under `src` into `dst` except the original gltf/bin (already
/// normalized), appending each as a library-relative path.
fn copy_rest(
    src: &Path,
    dst: &Path,
    dst_rel: &str,
    bin_uri: &str,
    written: &mut Vec<String>,
) -> AppResult<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let from = entry.path();
        let name_str = name.to_string_lossy();
        let child_rel = format!("{dst_rel}/{name_str}");
        if from.is_dir() {
            copy_rest(&from, &dst.join(&name), &child_rel, bin_uri, written)?;
        } else if name_str == bin_uri || name_str.ends_with(".gltf") {
            continue; // the buffer and gltf were normalized above
        } else {
            fsio::write_bytes(&dst.join(&name), &fsio::read_bytes(&from)?)?;
            written.push(child_rel);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulate FBX2glTF's output (gltf with uri "buffer.bin" + a texture) and
    /// confirm `promote` normalizes it to <stem>.gltf/<stem>.bin with the uri
    /// rewritten — the invariant `entry_from_parts` assumes.
    #[test]
    fn promote_renames_fbx2gltf_buffer_to_stem() {
        let tmp = std::env::temp_dir().join("toybox_promote_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let produced = tmp.join("Robot_out");
        std::fs::create_dir_all(&produced).unwrap();
        std::fs::write(
            produced.join("Robot.gltf"),
            r#"{"buffers":[{"uri":"buffer.bin","byteLength":3}],"images":[{"uri":"skin.png"}]}"#,
        )
        .unwrap();
        std::fs::write(produced.join("buffer.bin"), [1u8, 2, 3]).unwrap();
        std::fs::write(produced.join("skin.png"), [9u8, 9]).unwrap();

        let dest = tmp.join("lib");
        let written = promote(
            &produced.join("Robot.gltf"),
            &dest,
            "library/imported/characters",
            "Robot",
        )
        .unwrap();

        // gltf first, then bin, then texture — all library-relative.
        assert_eq!(written[0], "library/imported/characters/Robot.gltf");
        assert!(written.contains(&"library/imported/characters/Robot.bin".to_string()));
        assert!(written.contains(&"library/imported/characters/skin.png".to_string()));
        // The buffer is renamed on disk and in the gltf; "buffer.bin" is gone.
        assert!(dest.join("Robot.bin").exists());
        assert!(!dest.join("buffer.bin").exists());
        let doc: Value =
            serde_json::from_str(&fsio::read_text(&dest.join("Robot.gltf")).unwrap()).unwrap();
        assert_eq!(doc["buffers"][0]["uri"], "Robot.bin");
        assert_eq!(fsio::read_bytes(&dest.join("Robot.bin")).unwrap(), vec![1, 2, 3]);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
