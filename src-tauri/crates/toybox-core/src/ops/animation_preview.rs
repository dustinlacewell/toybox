use super::export_util::read_gltf;
use crate::infra::store::Store;
use crate::{
    domain::{
        catalog_model::{Asset, AssetContent, Catalog},
        paths, rig,
    },
    error::{AppError, AppResult},
    infra::registry::LibraryHandle,
};
use serde_json::Value;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChoice {
    pub asset_id: String,
    pub rig_id: String,
    pub exact: bool,
}

fn find<'a>(catalog: &'a Catalog, id: &str) -> AppResult<&'a Asset> {
    catalog
        .assets
        .iter()
        .flat_map(|asset| std::iter::once(asset).chain(asset.variants.iter()))
        .find(|asset| asset.id == id)
        .ok_or_else(|| AppError::msg("Preview asset is no longer in this library"))
}

fn load_pair(handle: &LibraryHandle, ids: &[&str]) -> AppResult<(Store, Vec<Asset>)> {
    super::catalog::open_library(handle)?;
    let store = Store::for_handle(handle);
    if !store.catalog_exists() {
        return Err(AppError::msg("Scan the library first"));
    }
    let wanted: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
    let found = store.load_assets(&wanted)?;
    let mut ordered = Vec::new();
    for id in ids {
        ordered.push(found.iter().find(|asset| asset.id == *id).cloned().ok_or_else(|| AppError::msg("Preview asset is no longer in this library"))?);
    }
    Ok((store, ordered))
}

/// Previews that can play `asset_id` (an animation) or that `asset_id` (a model) can play.
/// Only the assets that could match are read: those of the other kind that bind to a
/// rig of a kind this one binds to.
pub fn compatible_previews(handle: &LibraryHandle, asset_id: &str) -> AppResult<Vec<PreviewChoice>> {
    let (store, loaded) = load_pair(handle, &[asset_id])?;
    let asset = &loaded[0];
    let subject_rigs = store.rigs_for(std::slice::from_ref(asset))?;
    let wanted_kind = match &asset.content {
        AssetContent::Animation { .. } => "model",
        AssetContent::Model { .. } => "animation",
        _ => return Ok(Vec::new()),
    };
    let mut kinds: Vec<String> = subject_rigs.iter().map(|rig| rig.kind.clone()).collect();
    kinds.sort();
    kinds.dedup();
    let candidates = store.rigged_assets(wanted_kind, &kinds)?;
    let mut rigs = subject_rigs;
    let fetched = store.rigs_for(&candidates)?;
    rigs.extend(fetched);
    Ok(choices_among(asset, candidates.iter(), &rigs))
}

/// The composed glTF of one animation played on one model of the library.
pub fn read_animation_preview(handle: &LibraryHandle, animation_id: &str, model_id: &str) -> AppResult<Value> {
    let (_, pair) = load_pair(handle, &[animation_id, model_id])?;
    handle.ensure_assets(&pair)?;
    read_preview_assets(&handle.root_str(), &pair[0], &pair[1])
}

pub fn choices(catalog: &Catalog, asset_id: &str) -> AppResult<Vec<PreviewChoice>> {
    let asset = find(catalog, asset_id)?;
    Ok(choices_among(asset, catalog.assets.iter(), &catalog.rigs))
}

fn choices_among<'a>(asset: &Asset, others: impl Iterator<Item = &'a Asset>, rigs: &[crate::domain::rig::Rig]) -> Vec<PreviewChoice> {
    others
        .filter_map(|other| {
            let found = match &asset.content {
                AssetContent::Animation { .. } => rig::match_animation(asset, other, rigs),
                AssetContent::Model { .. } => rig::match_animation(other, asset, rigs),
                _ => None,
            }?;
            Some(PreviewChoice {
                asset_id: other.id.clone(),
                rig_id: found.rig_id,
                exact: found.exact,
            })
        })
        .collect()
}

pub fn read_preview(
    root: &str,
    catalog: &Catalog,
    animation_id: &str,
    model_id: &str,
) -> AppResult<Value> {
    read_preview_assets(root, find(catalog, animation_id)?, find(catalog, model_id)?)
}

/// The composed glTF of `animation` played on `model`, both read from their files.
pub fn read_preview_assets(root: &str, animation: &Asset, model: &Asset) -> AppResult<Value> {
    let mut animation_doc = read_gltf(root, animation)?;
    let mut model_doc = read_gltf(root, model)?;
    // Recheck live documents after origin corrections or external edits.
    let (a, ab) = rig::describe(&animation_doc)
        .ok_or_else(|| AppError::msg("Invalid animation hierarchy"))?;
    let (b, bb) =
        rig::describe(&model_doc).ok_or_else(|| AppError::msg("Invalid model hierarchy"))?;
    let mut current_animation = animation.clone();
    current_animation.rigs = vec![ab];
    let mut current_model = model.clone();
    current_model.rigs = vec![bb];
    let mapping = rig::match_animation(&current_animation,&current_model,&[a,b]).ok_or_else(|| AppError::msg("The animation and model do not have compatible named hierarchies and rest transforms. Rescan if either file changed."))?;
    for (doc, asset) in [(&mut animation_doc, animation), (&mut model_doc, model)] {
        let fileset = asset.fileset()?;
        let parent = std::path::Path::new(&fileset.gltf)
            .parent()
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        for field in ["buffers", "images"] {
            if let Some(items) = doc.get_mut(field).and_then(Value::as_array_mut) {
                for item in items {
                    if let Some(uri) = item["uri"].as_str().and_then(crate::domain::gltf_uri::external_file)
                    {
                        item["uri"] = Value::String(format!(
                            "toybox-resource:{}",
                            paths::resolve_uri_rel(&parent, uri)
                        ));
                    }
                }
            }
        }
    }
    crate::domain::animation_preview::compose(model_doc, &animation_doc, &mapping.nodes)
}
