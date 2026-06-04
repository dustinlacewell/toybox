//! Pure planning for the self-contained copy export. Given an asset's glTF doc
//! and its library-relative fileset, decide the target paths and the rewritten
//! glTF, without touching the filesystem. The command layer executes the plan.
//!
//! Two layouts:
//! - preserve: `target/<pack>/<category>/Name.{gltf,bin}` + `target/<pack>/textures/*`
//!   with image URIs rewritten to `../textures/Foo.png`.
//! - flatten:  `target/Name.{gltf,bin}` + `target/textures/*` with image URIs
//!   rewritten to `./textures/Foo.png`.
//!
//! Texture basenames are globally unique in this library, so a single flat
//! `textures/` dir never collides. glTF basenames DO collide across packs (31
//! of them); the command layer guards flatten collisions by namespacing.

use serde_json::Value;

use crate::domain::gltf_parse;
use crate::domain::paths;
use crate::error::AppResult;

/// One texture to copy: source (library-relative) -> target basename.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureCopy {
    pub src_rel: String,
    pub dst_basename: String,
}

/// The outcome of planning one asset's copy.
pub struct CopyPlan {
    /// The glTF document with buffer + image URIs rewritten for the target.
    pub gltf: Value,
    /// Target-relative path for the rewritten glTF (e.g. `Name.gltf` or
    /// `<pack>/<category>/Name.gltf`).
    pub gltf_dst: String,
    /// Target-relative path for the bin.
    pub bin_dst: String,
    /// Source-relative bin path (to copy bytes from).
    pub bin_src_rel: String,
    /// Textures to copy, with their target-relative directory.
    pub textures: Vec<TextureCopy>,
    /// Target-relative directory textures land in (e.g. `textures` or
    /// `<pack>/textures`).
    pub textures_dst_dir: String,
}

/// Build a copy plan. `gltf_rel` and `bin_rel` are library-relative; `pack`/
/// `category` drive the preserve layout. `stem` is the output base name without
/// extension (`Name`, or a namespaced `polygon_city_Name` when guarding a
/// flatten collision) — it names BOTH the gltf and bin so a renamed pair stays
/// consistent.
pub fn plan_copy(
    gltf_doc: &Value,
    gltf_rel: &str,
    bin_rel: &str,
    pack: &str,
    category: &str,
    stem: &str,
    preserve_structure: bool,
) -> AppResult<CopyPlan> {
    let gltf_dir = parent_rel(gltf_rel);
    let gltf_name = format!("{stem}.gltf");
    let bin_name = format!("{stem}.bin");

    let (gltf_dst, bin_dst, textures_dst_dir, image_uri_prefix) = if preserve_structure {
        let dir = format!("{pack}/{category}");
        (
            format!("{dir}/{gltf_name}"),
            format!("{dir}/{bin_name}"),
            format!("{pack}/textures"),
            "../textures",
        )
    } else {
        (
            gltf_name,
            bin_name,
            "textures".to_string(),
            "./textures",
        )
    };

    let mut gltf = gltf_doc.clone();

    // Rewrite the single buffer URI to the (possibly namespaced) bin basename.
    for buf in gltf_parse::buffer_uris(gltf_doc) {
        gltf_parse::set_uri(&mut gltf, "buffers", buf.index, basename(&bin_dst))?;
    }

    // Rewrite each image URI to the local textures dir; collect copies (deduped).
    let mut textures: Vec<TextureCopy> = Vec::new();
    for img in gltf_parse::image_uris(gltf_doc) {
        let src_rel = paths::resolve_uri_rel(&gltf_dir, &img.uri);
        let dst_basename = basename(&src_rel).to_string();
        gltf_parse::set_uri(
            &mut gltf,
            "images",
            img.index,
            &format!("{image_uri_prefix}/{dst_basename}"),
        )?;
        let copy = TextureCopy { src_rel, dst_basename };
        if !textures.contains(&copy) {
            textures.push(copy);
        }
    }

    Ok(CopyPlan {
        gltf,
        gltf_dst,
        bin_dst,
        bin_src_rel: bin_rel.to_string(),
        textures,
        textures_dst_dir,
    })
}

fn parent_rel(rel: &str) -> String {
    let norm = rel.replace('\\', "/");
    match norm.rfind('/') {
        Some(i) => norm[..i].to_string(),
        None => String::new(),
    }
}

fn basename(rel: &str) -> &str {
    let norm = rel.trim_end_matches('/');
    match norm.rfind(['/', '\\']) {
        Some(i) => &norm[i + 1..],
        None => norm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc() -> Value {
        json!({
            "buffers": [{ "uri": "Name.bin", "byteLength": 10 }],
            "images": [
                { "uri": "../textures/Atlas.png" },
                { "uri": "../textures/Emissive.png" }
            ]
        })
    }

    #[test]
    fn flatten_layout_rewrites_uris() {
        let plan = plan_copy(
            &doc(),
            "library/polygon_city/props/Name.gltf",
            "library/polygon_city/props/Name.bin",
            "polygon_city",
            "props",
            "Name",
            false,
        )
        .unwrap();

        assert_eq!(plan.gltf_dst, "Name.gltf");
        assert_eq!(plan.bin_dst, "Name.bin");
        assert_eq!(plan.textures_dst_dir, "textures");
        assert_eq!(plan.textures.len(), 2);
        assert_eq!(plan.gltf["buffers"][0]["uri"], "Name.bin");
        assert_eq!(plan.gltf["images"][0]["uri"], "./textures/Atlas.png");
        assert_eq!(
            plan.textures[0].src_rel,
            "library/polygon_city/textures/Atlas.png"
        );
    }

    #[test]
    fn preserve_layout_uses_pack_dirs() {
        let plan = plan_copy(
            &doc(),
            "library/polygon_city/props/Name.gltf",
            "library/polygon_city/props/Name.bin",
            "polygon_city",
            "props",
            "Name",
            true,
        )
        .unwrap();

        assert_eq!(plan.gltf_dst, "polygon_city/props/Name.gltf");
        assert_eq!(plan.bin_dst, "polygon_city/props/Name.bin");
        assert_eq!(plan.textures_dst_dir, "polygon_city/textures");
        assert_eq!(plan.gltf["images"][0]["uri"], "../textures/Atlas.png");
    }

    #[test]
    fn namespaced_stem_renames_gltf_and_bin_together() {
        let plan = plan_copy(
            &doc(),
            "library/polygon_city/props/Name.gltf",
            "library/polygon_city/props/Name.bin",
            "polygon_city",
            "props",
            "polygon_city_Name",
            false,
        )
        .unwrap();
        assert_eq!(plan.gltf_dst, "polygon_city_Name.gltf");
        assert_eq!(plan.bin_dst, "polygon_city_Name.bin");
        // The buffer URI inside the gltf points at the renamed bin.
        assert_eq!(plan.gltf["buffers"][0]["uri"], "polygon_city_Name.bin");
    }
}
