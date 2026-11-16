//! Exact file ownership and reversible staging before catalog deletion commits.
use crate::{
    domain::catalog_model::{Asset, AssetContent, Catalog},
    error::{AppError, AppResult},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

fn files(asset: &Asset) -> Vec<&str> {
    let mut result = match &asset.content {
        AssetContent::Audio {audio} => vec![audio.file.as_str()],
        AssetContent::Model { fileset, .. } | AssetContent::Animation { fileset, .. } => {
            let mut paths = vec![fileset.gltf.as_str(), fileset.bin.as_str()];
            paths.extend(fileset.textures.iter().map(String::as_str));
            paths
        }
        AssetContent::Material { material } => {
            material.maps.iter().map(|map| map.file.as_str()).collect()
        }
    };
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
                        path.display()
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
        for (_, staged) in &self.files {
            if let Err(error) = crate::infra::fsio::remove_file_if_exists(staged) {
                failure = Some(error);
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
}
