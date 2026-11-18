//! Exact file ownership and reversible staging before catalog deletion commits.
#[cfg(test)]
use crate::domain::{catalog_model::{Asset, AssetContent, Catalog}, gltf_uri};
use crate::error::{AppError, AppResult};
#[cfg(test)]
use std::collections::HashSet;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[cfg(test)]
fn files(asset: &Asset) -> Vec<&str> {
    let mut result = match &asset.content {
        AssetContent::Audio {audio} => vec![audio.file.as_str()],
        AssetContent::Plugin { files, .. } => files.iter().map(String::as_str).collect(),
        AssetContent::Model { fileset, .. } | AssetContent::Animation { fileset, .. } => {
            let mut paths = vec![fileset.gltf.as_str(), fileset.bin.as_str()];
            paths.extend(fileset.textures.iter().map(String::as_str).filter(|path| !gltf_uri::is_embedded_path(path)));
            paths
        }
        AssetContent::Material { material } => {
            material.maps.iter().map(|map| map.file.as_str()).collect()
        }
    };
    // Source files the importing plugin keeps with the item go with it.
    if let Some(origin) = &asset.origin {
        result.extend(origin.files.iter().map(String::as_str));
    }
    for variant in &asset.variants {
        result.extend(files(variant));
    }
    result
}

fn key(path: &Path) -> String {
    let value = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

#[cfg(test)]
pub fn exclusive_thumbnails(
    cache: &Path,
    catalog: &Catalog,
    ids: &[String],
) -> AppResult<Vec<PathBuf>> {
    let parent = cache
        .parent()
        .ok_or_else(|| AppError::msg("Catalog cache has no parent"))?
        .canonicalize()?;
    let folder = parent.join("thumbs");
    if !folder.exists() {
        return Ok(vec![]);
    }
    let folder = folder.canonicalize()?;
    if !folder.starts_with(&parent) {
        return Err(AppError::msg("Thumbnail cache is outside application data"));
    }
    let selected: HashSet<_> = ids.iter().map(String::as_str).collect();
    let mut removed = HashSet::new();
    let mut kept = HashSet::new();
    for asset in &catalog.assets {
        let names = if selected.contains(asset.id.as_str()) {
            &mut removed
        } else {
            &mut kept
        };
        for item in std::iter::once(asset).chain(asset.variants.iter()) {
            names.insert(crate::infra::appdata::sanitize_id(&item.id));
        }
    }
    let mut paths = Vec::new();
    for name in removed.difference(&kept) {
        let path = folder.join(format!("{name}.png"));
        if !path.exists() {
            continue;
        }
        let path = path.canonicalize()?;
        if !path.starts_with(&folder) || !path.is_file() {
            return Err(AppError::msg("Thumbnail is outside its cache"));
        }
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
pub fn exclusive_files(root: &Path, catalog: &Catalog, ids: &[String]) -> AppResult<Vec<PathBuf>> {
    let root = root
        .canonicalize()
        .map_err(|error| AppError::file("Resolve library", root, error))?;
    let data = root
        .join("library")
        .canonicalize()
        .map_err(|error| AppError::file("Resolve library files", &root.join("library"), error))?;
    // A library/data junction itself may not escape the chosen library.
    if !data.starts_with(&root) {
        return Err(AppError::msg(
            "Library data directory is outside the selected library",
        ));
    }
    let selected: HashSet<_> = ids.iter().map(String::as_str).collect();
    let mut candidates = HashMap::new();
    let mut retained = HashSet::new();
    for asset in &catalog.assets {
        for relative in files(asset) {
            if Path::new(relative).is_absolute() {
                return Err(AppError::msg("Asset file must be relative to its library"));
            }
            let path = root.join(relative);
            let canonical = match path.canonicalize() {
                Ok(path) => path,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(AppError::file("Resolve asset file", &path, error)),
            };
            if selected.contains(asset.id.as_str()) {
                if !canonical.starts_with(&data) || !canonical.is_file() {
                    return Err(AppError::msg(format!(
                        "Refusing to delete a path outside library asset files: {}",
                        crate::error::short_path(&path)
                    )));
                }
                candidates.insert(key(&canonical), canonical);
            } else {
                retained.insert(key(&canonical));
            }
        }
    }
    let mut result: Vec<_> = candidates
        .into_iter()
        .filter_map(|(key, path)| (!retained.contains(&key)).then_some(path))
        .collect();
    result.sort();
    Ok(result)
}

/// The thumbnail files only the assets `ids` (and their variants) own, found
/// through the store's name index instead of walking the whole catalog.
pub fn exclusive_thumbnails_in(cache: &Path, store: &crate::infra::store::Store, ids: &[String]) -> AppResult<Vec<PathBuf>> {
    let parent = cache
        .parent()
        .ok_or_else(|| AppError::msg("Catalog cache has no parent"))?
        .canonicalize()?;
    let folder = parent.join("thumbs");
    if !folder.exists() {
        return Ok(vec![]);
    }
    let folder = folder.canonicalize()?;
    if !folder.starts_with(&parent) {
        return Err(AppError::msg("Thumbnail cache is outside application data"));
    }
    let mut paths = Vec::new();
    for name in store.exclusive_thumb_names(ids)? {
        let path = folder.join(format!("{name}.png"));
        if !path.exists() {
            continue;
        }
        let path = path.canonicalize()?;
        if !path.starts_with(&folder) || !path.is_file() {
            return Err(AppError::msg("Thumbnail is outside its cache"));
        }
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}

/// The files only the assets `ids` (and their variants) own, found through the
/// store's file-reference table instead of walking every file of every asset.
pub fn exclusive_files_in(root: &Path, store: &crate::infra::store::Store, ids: &[String]) -> AppResult<Vec<PathBuf>> {
    let root = root
        .canonicalize()
        .map_err(|error| AppError::file("Resolve library", root, error))?;
    let data = root
        .join("library")
        .canonicalize()
        .map_err(|error| AppError::file("Resolve library files", &root.join("library"), error))?;
    // A library/data junction itself may not escape the chosen library.
    if !data.starts_with(&root) {
        return Err(AppError::msg(
            "Library data directory is outside the selected library",
        ));
    }
    let mut candidates = HashMap::new();
    for relative in store.exclusive_files(ids)? {
        if Path::new(&relative).is_absolute() {
            return Err(AppError::msg("Asset file must be relative to its library"));
        }
        let path = root.join(&relative);
        let canonical = match path.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(AppError::file("Resolve asset file", &path, error)),
        };
        if !canonical.starts_with(&data) || !canonical.is_file() {
            return Err(AppError::msg(format!(
                "Refusing to delete a path outside library asset files: {}",
                crate::error::short_path(&path)
            )));
        }
        candidates.insert(key(&canonical), canonical);
    }
    let mut result: Vec<_> = candidates.into_values().collect();
    result.sort();
    Ok(result)
}

pub struct StagedFiles {
    files: Vec<(PathBuf, PathBuf)>,
    committed: bool,
}
impl StagedFiles {
    pub fn stage(paths: Vec<PathBuf>, stamp: u128) -> AppResult<Self> {
        let mut result = Self {
            files: vec![],
            committed: false,
        };
        for (index, path) in paths.into_iter().enumerate() {
            let staged = path.with_file_name(format!(".toybox-delete-{stamp}-{index}.tmp"));
            std::fs::rename(&path, &staged)
                .map_err(|error| AppError::file("Prepare file for deletion", &path, error))?;
            result.files.push((path, staged));
        }
        Ok(result)
    }
    pub fn erase(mut self) -> AppResult<()> {
        // The catalogs have committed. Unlink only these exact staged files;
        // never recursively delete an asset folder or shared dependency.
        self.committed = true;
        let mut failure = None;
        for (original, staged) in &self.files {
            if let Err(error) = crate::infra::fsio::remove_file_if_exists(staged) {
                failure = Some(error);
            } else {
                // The last file of a set takes its (now empty) set folder with it.
                crate::infra::retention::remove_set_folder_if_empty(original);
            }
        }
        failure.map_or(Ok(()),|error|Err(AppError::msg(format!("Items were removed from the library, but a staged file could not be erased: {error}"))))
    }
}
impl Drop for StagedFiles {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (original, staged) in self.files.iter().rev() {
            if let Err(error) = std::fs::rename(staged, original) {
                eprintln!(
                    "Could not restore {} from {}: {error}",
                    original.display(),
                    staged.display()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn ownership_covers_variants_and_preserves_shared_files_across_asset_types() {
        let root =
            std::env::temp_dir().join(format!("toybox-file-ownership-{}", std::process::id()));
        std::fs::create_dir_all(root.join("library/p")).unwrap();
        for name in [
            "hero.gltf",
            "hero.bin",
            "shared.png",
            "unique.png",
            "small.png",
            "clip.gltf",
        ] {
            std::fs::write(root.join("library/p").join(name), b"asset").unwrap();
        }
        let base = json!({"id":"model","name":"Hero","kind":"model","pack":"p","category":"characters","fileName":"hero.gltf","relPath":"library/p/hero.gltf","fileset":{"gltf":"library/p/hero.gltf","bin":"library/p/hero.bin","textures":["library/p/shared.png"]},"user":{"favorite":false,"tags":[]},"thumb":{"state":"ready"}});
        let mut material = base.clone();
        material["id"] = json!("material");
        material["kind"] = json!("material");
        material["material"] = json!({"maps":[{"role":"albedo","file":"library/p/unique.png"},{"role":"normal","file":"library/p/shared.png"}]});
        let mut variant = material.clone();
        variant["id"] = json!("small");
        variant["material"]["maps"] = json!([{"role":"albedo","file":"library/p/small.png"}]);
        material["variants"] = json!([variant]);
        let mut survivor = base.clone();
        survivor["id"] = json!("animation");
        survivor["kind"] = json!("animation");
        survivor["fileset"]["gltf"] = json!("library/p/clip.gltf");
        survivor["clip"] = json!({"index":0,"name":"Walk","duration":1,"sourceId":"model","sourceName":"Hero","targetNodes":[],"rig":""});
        let mut catalog: Catalog = serde_json::from_value(
            json!({"schemaVersion":5,"libraryRoot":root,"assets":[base,material,survivor]}),
        )
        .unwrap();
        let owned = exclusive_files(&root, &catalog, &["model".into(), "material".into()]).unwrap();
        let names: HashSet<_> = owned
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            HashSet::from([
                "hero.gltf".to_string(),
                "unique.png".to_string(),
                "small.png".to_string()
            ])
        );
        StagedFiles::stage(owned, 123).unwrap().erase().unwrap();
        assert!(!root.join("library/p/hero.gltf").exists());
        assert!(!root.join("library/p/small.png").exists());
        assert!(root.join("library/p/hero.bin").exists());
        assert!(root.join("library/p/shared.png").exists());
        assert!(root.join("library/p/clip.gltf").exists());
        if let AssetContent::Model { fileset, .. } = &mut catalog.assets[0].content {
            fileset.gltf = "_library_config/catalog.json".into();
        }
        std::fs::create_dir_all(root.join("_library_config")).unwrap();
        std::fs::write(root.join("_library_config/catalog.json"), b"metadata").unwrap();
        assert!(exclusive_files(&root, &catalog, &["model".into()]).is_err());
        if let AssetContent::Model { fileset, .. } = &mut catalog.assets[0].content {
            fileset.gltf = format!(
                "../toybox-file-ownership-{}.outside.gltf",
                std::process::id()
            );
        }
        let outside = root.with_extension("outside.gltf");
        std::fs::write(&outside, b"outside").unwrap();
        assert!(exclusive_files(&root, &catalog, &["model".into()]).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
        std::fs::remove_file(outside).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_model_is_deleted_together_with_the_original_files_it_kept() {
        let root = std::env::temp_dir().join(format!("toybox-kept-originals-{}", std::process::id()));
        std::fs::create_dir_all(root.join("library/models/key")).unwrap();
        for name in ["Crate.gltf", "Crate.bin", "Crate.obj", "Crate.mtl"] {
            std::fs::write(root.join("library/models/key").join(name), b"x").unwrap();
        }
        let catalog: Catalog = serde_json::from_value(json!({"schemaVersion":5,"libraryRoot":root,"assets":[{
            "id":"crate","name":"Crate","kind":"model","pack":"p","category":"props","fileName":"Crate.gltf","relPath":"library/models/key/Crate.gltf",
            "fileset":{"gltf":"library/models/key/Crate.gltf","bin":"library/models/key/Crate.bin","textures":[]},
            "origin":{"plugin":"x","files":["library/models/key/Crate.obj","library/models/key/Crate.mtl"]},
            "user":{"favorite":false,"tags":[]},"thumb":{"state":"ready"}}]})).unwrap();
        let owned = exclusive_files(&root, &catalog, &["crate".into()]).unwrap();
        let names: HashSet<_> = owned.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, HashSet::from(["Crate.gltf", "Crate.bin", "Crate.obj", "Crate.mtl"].map(String::from)));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn staging_failure_restores_every_previously_moved_file() {
        let root =
            std::env::temp_dir().join(format!("toybox-stage-rollback-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("original.bin");
        std::fs::write(&path, b"original").unwrap();
        assert!(StagedFiles::stage(vec![path.clone(), root.join("missing.bin")], 456).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_models_set_folder_goes_with_its_files_and_shared_resources_go_with_their_last_user() {
        let root = std::env::temp_dir().join(format!("toybox-model-sets-{}", std::process::id()));
        let (key_a, key_b) = ("a".repeat(64), "b".repeat(64));
        for (set, files) in [(&key_a, ["Hero.gltf", "Hero.bin"]), (&key_b, ["Tree.gltf", "Tree.bin"])] {
            for name in files {
                std::fs::create_dir_all(root.join("library/models").join(set)).unwrap();
                std::fs::write(root.join("library/models").join(set).join(name), b"x").unwrap();
            }
        }
        std::fs::create_dir_all(root.join("library/models/_resources")).unwrap();
        for name in ["shared.png", "only-hero.png"] {
            std::fs::write(root.join("library/models/_resources").join(name), b"x").unwrap();
        }
        let model = |id: &str, set: &str, name: &str, textures: serde_json::Value| json!({"id":id,"name":name,"kind":"model","pack":"p","category":"props","fileName":format!("{name}.gltf"),
            "relPath":format!("library/models/{set}/{name}.gltf"),"fileset":{"gltf":format!("library/models/{set}/{name}.gltf"),"bin":format!("library/models/{set}/{name}.bin"),"textures":textures},
            "user":{"favorite":false,"tags":[]},"thumb":{"state":"ready"}});
        let mut catalog: Catalog = serde_json::from_value(json!({"schemaVersion":5,"libraryRoot":root,"assets":[
            model("hero", &key_a, "Hero", json!(["library/models/_resources/shared.png","library/models/_resources/only-hero.png"])),
            model("tree", &key_b, "Tree", json!(["library/models/_resources/shared.png"]))]})).unwrap();
        let models = root.join("library/models");
        StagedFiles::stage(exclusive_files(&root, &catalog, &["hero".into()]).unwrap(), 1).unwrap().erase().unwrap();
        assert!(!models.join(&key_a).exists(), "the glTF and bin were the whole set");
        assert!(!models.join("_resources/only-hero.png").exists(), "a resource nobody else uses goes with its model");
        assert!(models.join("_resources/shared.png").exists() && models.join(&key_b).join("Tree.gltf").exists());
        catalog.assets.retain(|asset| asset.id == "tree"); // the first delete committed
        StagedFiles::stage(exclusive_files(&root, &catalog, &["tree".into()]).unwrap(), 2).unwrap().erase().unwrap();
        assert!(!models.join(&key_b).exists() && !models.join("_resources/shared.png").exists());
        assert!(models.join("_resources").is_dir() && models.is_dir(), "the shared folder and the type folder stay");
        std::fs::remove_dir_all(root).unwrap();
    }
}
