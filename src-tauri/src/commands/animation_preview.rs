use super::{catalog::load_catalog_inner, export_util::read_gltf};
use crate::{
    domain::{
        catalog_model::{Asset, AssetContent, Catalog},
        paths, rig,
    },
    error::{AppError, AppResult},
    infra::library,
};
use serde_json::Value;
use tauri::AppHandle;

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

#[tauri::command]
pub async fn compatible_previews(
    app: AppHandle,
    asset_id: String,
) -> AppResult<Vec<PreviewChoice>> {
    let catalog =
        load_catalog_inner(&app)?.ok_or_else(|| AppError::msg("Scan the library first"))?;
    Ok(choices(&catalog, &asset_id)?)
}

pub fn choices(catalog: &Catalog, asset_id: &str) -> AppResult<Vec<PreviewChoice>> {
    let asset = find(catalog, asset_id)?;
    Ok(catalog
        .assets
        .iter()
        .filter_map(|other| {
            let found = match &asset.content {
                AssetContent::Animation { .. } => rig::match_animation(asset, other, &catalog.rigs),
                AssetContent::Model { .. } => rig::match_animation(other, asset, &catalog.rigs),
                _ => None,
            }?;
            Some(PreviewChoice {
                asset_id: other.id.clone(),
                rig_id: found.rig_id,
                exact: found.exact,
            })
        })
        .collect())
}

#[tauri::command]
pub async fn read_animation_preview(
    app: AppHandle,
    animation_id: String,
    model_id: String,
) -> AppResult<Value> {
    let catalog =
        load_catalog_inner(&app)?.ok_or_else(|| AppError::msg("Scan the library first"))?;
    let root = library::resolve(&app)?;
    read_preview(&root.to_string_lossy(), &catalog, &animation_id, &model_id)
}

pub fn read_preview(
    root: &str,
    catalog: &Catalog,
    animation_id: &str,
    model_id: &str,
) -> AppResult<Value> {
    let animation = find(catalog, animation_id)?;
    let model = find(catalog, model_id)?;
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
                    if let Some(uri) = item["uri"].as_str().filter(|uri| !uri.starts_with("data:"))
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
