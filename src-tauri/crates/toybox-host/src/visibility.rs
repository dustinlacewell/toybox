//! Which items the app hides because their plugin is switched off: the same rule
//! the frontend builds (`pluginVisibility` in `src/services/assetTypes.ts`), so the
//! command line lists what the window lists.

use std::collections::{BTreeSet, HashSet};

use toybox_core::error::AppResult;
use toybox_core::infra::store::browse::Visibility;

use crate::host::Host;
use crate::plugins::PluginManifestDto;

const NATIVE_KINDS: [&str; 4] = ["model", "material", "animation", "audio"];

fn active(plugin: &PluginManifestDto) -> bool {
    plugin.enabled && plugin.blocked_by.is_empty()
}

fn declared_ids(plugin: &PluginManifestDto) -> impl Iterator<Item = &str> {
    plugin.asset_types.as_array().into_iter().flatten().filter_map(|declaration| declaration.get("id").and_then(|id| id.as_str()))
}

fn replacements_of<'a>(plugins: &'a [PluginManifestDto], retired: &str) -> Vec<&'a PluginManifestDto> {
    plugins.iter().filter(|plugin| plugin.replaces.iter().any(|id| id == retired)).collect()
}

/// The visibility rules for these plugins (every installed plugin, on or off).
pub fn visibility_of(plugins: &[PluginManifestDto]) -> Visibility {
    let on_types: HashSet<&str> = plugins.iter().filter(|plugin| active(plugin)).flat_map(declared_ids).collect();
    let owner_off: BTreeSet<String> = plugins.iter().filter(|plugin| !active(plugin)).flat_map(declared_ids).filter(|id| !on_types.contains(id)).map(String::from).collect();
    // Native types an enabled plugin still imports into, without declaring them.
    let contributed: HashSet<String> = plugins
        .iter()
        .filter(|plugin| active(plugin) && (plugin.asset_types.as_array().is_some_and(|types| !types.is_empty()) || plugin.ui.get("importPanel").is_some_and(|panel| !panel.is_null() && panel != "")))
        .flat_map(|plugin| plugin.asset_kinds.as_array().into_iter().flatten().filter_map(|kind| kind.as_str().map(String::from)))
        .filter(|kind| NATIVE_KINDS.contains(&kind.as_str()))
        .collect();
    let hidden: BTreeSet<String> = owner_off.iter().filter(|id| !contributed.contains(*id)).cloned().collect();
    let installed: HashSet<&str> = plugins.iter().map(|plugin| plugin.id.as_str()).collect();
    let retired: BTreeSet<String> = plugins
        .iter()
        .flat_map(|plugin| plugin.replaces.iter())
        .filter(|id| !installed.contains(id.as_str()) && !replacements_of(plugins, id).is_empty())
        .cloned()
        .collect();
    let any_on = |id: &str| replacements_of(plugins, id).iter().any(|plugin| active(plugin));
    Visibility {
        hidden_types: hidden.into_iter().collect(),
        plugins_off: sorted(plugins.iter().filter(|plugin| !active(plugin)).map(|plugin| plugin.id.clone())),
        plugins_on: sorted(plugins.iter().filter(|plugin| active(plugin)).map(|plugin| plugin.id.clone())),
        retired_on: retired.iter().filter(|id| any_on(id)).cloned().collect(),
        retired_off: retired.iter().filter(|id| !any_on(id)).cloned().collect(),
        owner_off: owner_off.into_iter().collect(),
    }
}

fn sorted(ids: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = ids.collect();
    out.sort();
    out
}

impl Host {
    /// What the app hides now, from the installed plugins and their switches.
    pub fn visibility(&self) -> AppResult<Visibility> {
        Ok(visibility_of(&self.list_plugins()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(doc: serde_json::Value, disabled: bool) -> PluginManifestDto {
        let id = doc["id"].as_str().unwrap().to_string();
        let base = std::env::temp_dir().join(format!("toybox-visibility-{}", std::process::id()));
        let dir = base.join(&id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), serde_json::to_vec(&doc).unwrap()).unwrap();
        let mut found = crate::plugins::read_manifest(&dir).unwrap();
        found.enabled = !disabled;
        let _ = std::fs::remove_dir_all(&base);
        found
    }

    #[test]
    fn a_switched_off_type_is_hidden_and_the_plugin_listed_off() {
        let sky = plugin(serde_json::json!({ "id": "sky", "name": "Sky", "version": "1.0.0", "assetTypes": [{ "id": "skybox", "label": "Skies" }] }), true);
        let models = plugin(serde_json::json!({ "id": "models", "name": "Models", "version": "1.0.0", "assetTypes": [{ "id": "model", "label": "Models", "implementation": "native" }] }), false);
        let rules = visibility_of(&[sky, models]);
        assert_eq!(rules.hidden_types, vec!["skybox"]);
        assert_eq!(rules.plugins_off, vec!["sky"]);
        assert_eq!(rules.plugins_on, vec!["models"]);
        assert_eq!(rules.owner_off, vec!["skybox"]);
    }
}
