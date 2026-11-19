//! Plugin discovery and the dependency rules. Plugins are folders under
//! `<app-data>/plugins/<id>/` each carrying a `manifest.json` and an entry module.
//! The jailed filesystem surface plugins call through `ctx.fs` is in `plugin_fs`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use serde::Serialize;
use toybox_core::error::{AppError, AppResult};

use crate::bundled::{self, Retirement};
use crate::host::Host;

/// A discovered plugin's validated manifest plus the absolute path of its entry
/// module (so the loader can address it via the `plugin://` protocol).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifestDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub asset_icons: serde_json::Value,
    pub id: String,
    pub name: String,
    pub version: String,
    /// Entry module, relative to the plugin folder; empty when the plugin has none.
    pub entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub permissions: serde_json::Value,
    /// Asset kinds the plugin works with (an importer: what it produces);
    /// `["model"]` when the manifest omits it. Opaque here, like `ui`.
    pub asset_kinds: serde_json::Value,
    /// The library types an `assetType` plugin defines ({ id, label, … }[]).
    /// Opaque here; the frontend registry validates each declaration.
    pub asset_types: serde_json::Value,
    pub fields: serde_json::Value,
    /// Settings the plugin asks the user for ({ key, type, label, … }[]),
    /// shown under the plugin in Settings. Opaque here.
    pub settings: serde_json::Value,
    /// Plugin-shipped slot UI modules ({ exportPanel?, importPanel? }), opaque
    /// here — the frontend loader validates and mounts them.
    pub ui: serde_json::Value,
    /// Absolute path to the entry module on disk (entry resolved under the dir);
    /// empty when the plugin has no entry.
    pub entry_abs_path: String,
    /// False when the user has turned the plugin off in settings. Says nothing
    /// about dependencies; see `blocked_by`.
    pub enabled: bool,
    /// Other plugins this one needs, by id, with the version range each must satisfy.
    pub dependencies: BTreeMap<String, String>,
    /// The direct dependencies that are not usable, one entry each.
    pub blocked_by: Vec<Blocked>,
    /// Ids of switched-on installed plugins that list this one as a dependency.
    pub required_by: Vec<String>,
    /// Retired plugin ids this plugin replaces (see `RETIRED_PLUGINS`). Library
    /// items that record one of them in `origin.plugin` follow this plugin.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub replaces: Vec<String>,
}

/// One dependency of a plugin that cannot be used, and why: `missing`,
/// `version`, `disabled`, `blocked` (it has unusable dependencies itself) or
/// `cycle` (it and the plugin depend on each other).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blocked {
    pub id: String,
    pub reason: String,
    /// The version range asked for (`version` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs: Option<String>,
    /// The installed version (`version` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found: Option<String>,
}

impl Host {
    /// The installed plugins with their on/off switch and dependency state resolved.
    /// A directory without a readable/valid manifest or with a mismatched id is
    /// skipped (not an error) so one bad plugin never breaks discovery. A missing
    /// entry file is not checked here; the frontend loader reports it. A retired
    /// bundled plugin is left out while its replacement is installed.
    pub fn list_plugins(&self) -> AppResult<Vec<PluginManifestDto>> {
        let dir = self.paths.plugins_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            // `entry.path().is_dir()` (not `file_type().is_dir()`) so a directory
            // junction/symlink — how the dev tooling links in-repo plugins on
            // Windows — is followed and counts as a plugin dir. `file_type()` does
            // not traverse reparse points, so a junction would report as a symlink
            // and be skipped.
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(m) = read_manifest(&path) {
                out.push(m);
            }
        }
        drop_retired(&mut out, bundled::RETIRED_PLUGINS);
        mark_replaced(&mut out, bundled::RETIRED_PLUGINS);
        // Disabled plugins are still listed, so settings can offer to turn them
        // back on; the frontend does not load them.
        let disabled = self.load_settings()?.disabled_plugins;
        resolve_dependencies(&mut out, &disabled);
        Ok(out)
    }
}

/// Remove each retired plugin whose replacements are all in the list; one retired
/// without replacements is always removed.
fn drop_retired(manifests: &mut Vec<PluginManifestDto>, retired: &[Retirement]) {
    let ids: BTreeSet<String> = manifests.iter().map(|manifest| manifest.id.clone()).collect();
    manifests.retain(|manifest| !retired.iter().any(|(old, replacements)| {
        manifest.id == *old && bundled::is_superseded(replacements, |id| ids.contains(id))
    }));
}

/// Tell each plugin which retired ids it replaces, so the frontend can follow
/// library items that still record those ids.
fn mark_replaced(manifests: &mut [PluginManifestDto], retired: &[Retirement]) {
    for manifest in manifests {
        manifest.replaces = bundled::replaces(retired, &manifest.id);
    }
}

/// Set `enabled`, `blocked_by` and `required_by` on every manifest from the
/// user's switches (`disabled` ids) and the dependencies the manifests declare.
fn resolve_dependencies(manifests: &mut [PluginManifestDto], disabled: &[String]) {
    let index: HashMap<String, usize> = manifests.iter().enumerate().map(|(i, manifest)| (manifest.id.clone(), i)).collect();
    let edges: Vec<Vec<usize>> = manifests.iter()
        .map(|manifest| manifest.dependencies.keys().filter_map(|id| index.get(id).copied()).collect())
        .collect();
    // Whether `to` can be reached from `from` by following dependencies.
    let reaches = |from: usize, to: usize| {
        let mut seen = vec![false; edges.len()];
        let mut stack = vec![from];
        while let Some(at) = stack.pop() {
            for &next in &edges[at] {
                if next == to { return true; }
                if !std::mem::replace(&mut seen[next], true) { stack.push(next); }
            }
        }
        false
    };
    let is_off = |manifest: &PluginManifestDto| disabled.contains(&manifest.id);
    // Edges inside a cycle are blocked as "cycle"; the rest form an acyclic
    // graph, so a dependency's own state can be resolved first.
    let mut state: Vec<Option<Vec<Blocked>>> = vec![None; manifests.len()];
    fn blocked_of(at: usize, manifests: &[PluginManifestDto], index: &HashMap<String, usize>, state: &mut Vec<Option<Vec<Blocked>>>, is_off: &dyn Fn(&PluginManifestDto) -> bool, reaches: &dyn Fn(usize, usize) -> bool) -> Vec<Blocked> {
        if let Some(done) = &state[at] { return done.clone(); }
        let mut out = Vec::new();
        for (id, range) in &manifests[at].dependencies {
            let blocked = |reason: &str| Blocked { id: id.clone(), reason: reason.to_string(), needs: None, found: None };
            let Some(&dep) = index.get(id) else { out.push(blocked("missing")); continue };
            if reaches(dep, at) { out.push(blocked("cycle")); continue; }
            let installed = &manifests[dep];
            let satisfied = semver::VersionReq::parse(range).ok()
                .zip(semver::Version::parse(&installed.version).ok())
                .is_some_and(|(req, version)| req.matches(&version));
            if !satisfied {
                out.push(Blocked { needs: Some(range.clone()), found: Some(installed.version.clone()), ..blocked("version") });
            } else if is_off(installed) {
                out.push(blocked("disabled"));
            } else if !blocked_of(dep, manifests, index, state, is_off, reaches).is_empty() {
                out.push(blocked("blocked"));
            }
        }
        state[at] = Some(out.clone());
        out
    }
    let mut blocked: Vec<Vec<Blocked>> = (0..manifests.len())
        .map(|at| blocked_of(at, manifests, &index, &mut state, &is_off, &reaches))
        .collect();
    let mut required_by: Vec<Vec<String>> = vec![Vec::new(); manifests.len()];
    for (at, manifest) in manifests.iter().enumerate() {
        if is_off(manifest) { continue; }
        for &dep in &edges[at] {
            if dep != at { required_by[dep].push(manifest.id.clone()); }
        }
    }
    for (at, manifest) in manifests.iter_mut().enumerate() {
        manifest.enabled = !disabled.contains(&manifest.id);
        manifest.blocked_by = std::mem::take(&mut blocked[at]);
        manifest.required_by = std::mem::take(&mut required_by[at]);
        manifest.required_by.sort();
    }
}

/// The result of switching a plugin on or off: the new list of switched-off
/// ids and the ids whose switch changed, both sorted.
#[derive(Debug, PartialEq)]
pub struct EnablePlan {
    pub disabled: Vec<String>,
    pub changed: Vec<String>,
}

/// Work out switching `id` on or off. Switching on also switches on every
/// installed plugin it depends on, transitively. Switching off is refused while
/// a switched-on installed plugin directly depends on it.
pub fn plan_plugin_enabled(manifests: &[PluginManifestDto], disabled: &[String], id: &str, enabled: bool) -> AppResult<EnablePlan> {
    let mut off: BTreeSet<String> = disabled.iter().cloned().collect();
    let before = off.clone();
    if enabled {
        let by_id: HashMap<&str, &PluginManifestDto> = manifests.iter().map(|manifest| (manifest.id.as_str(), manifest)).collect();
        let mut stack = vec![id.to_string()];
        let mut seen = BTreeSet::new();
        while let Some(next) = stack.pop() {
            if !seen.insert(next.clone()) { continue; }
            off.remove(&next);
            if let Some(manifest) = by_id.get(next.as_str()) {
                stack.extend(manifest.dependencies.keys().filter(|dep| by_id.contains_key(dep.as_str())).cloned());
            }
        }
    } else if !off.contains(id) {
        let mut dependents: Vec<&str> = manifests.iter()
            .filter(|manifest| manifest.id != id && !off.contains(&manifest.id) && manifest.dependencies.contains_key(id))
            .map(|manifest| manifest.name.as_str())
            .collect();
        if !dependents.is_empty() {
            dependents.sort();
            let name = manifests.iter().find(|manifest| manifest.id == id).map_or(id, |manifest| manifest.name.as_str());
            let list = match dependents.split_last() {
                Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
                _ => dependents[0].to_string(),
            };
            let pronoun = if dependents.len() == 1 { "it" } else { "them" };
            return Err(AppError::msg(format!("{name} is needed by {list}. Turn {pronoun} off first.")));
        }
        off.insert(id.to_string());
    }
    let changed = before.symmetric_difference(&off).cloned().collect();
    Ok(EnablePlan { disabled: off.into_iter().collect(), changed })
}

/// Read + validate one plugin dir's `manifest.json`. Returns `None` on any
/// problem (so discovery is tolerant); detailed surfacing is the loader's job.
pub(crate) fn read_manifest(plugin_dir: &Path) -> Option<PluginManifestDto> {
    let dir_name = plugin_dir.file_name()?.to_str()?.to_string();
    let manifest_path = plugin_dir.join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    parse_manifest(&dir_name, &doc, plugin_dir)
}

/// The plugin ids a manifest's `dependencies` names, each with its version
/// range: an object of id to range, or an array of ids (any version).
fn parse_dependencies(value: Option<&serde_json::Value>) -> BTreeMap<String, String> {
    match value {
        Some(serde_json::Value::Object(map)) => map.iter()
            .map(|(id, range)| (id.clone(), range.as_str().unwrap_or("*").to_string()))
            .collect(),
        Some(serde_json::Value::Array(ids)) => ids.iter()
            .filter_map(|id| id.as_str())
            .map(|id| (id.to_string(), "*".to_string()))
            .collect(),
        _ => BTreeMap::new(),
    }
}

/// Validate a parsed manifest living in the folder `dir_name`.
fn parse_manifest(dir_name: &str, doc: &serde_json::Value, plugin_dir: &Path) -> Option<PluginManifestDto> {
    let id = doc.get("id")?.as_str()?.to_string();
    if id != dir_name {
        return None; // id must match the folder it lives in
    }
    let name = doc.get("name")?.as_str()?.to_string();
    let version = doc.get("version")?.as_str()?.to_string();
    // A plugin works with the asset types it declares unless it says otherwise,
    // and with models when it declares none.
    let asset_types = doc.get("assetTypes").filter(|v| v.is_array()).cloned().unwrap_or_else(|| serde_json::json!([]));
    let declared_ids: Vec<serde_json::Value> = asset_types.as_array().into_iter().flatten()
        .filter_map(|declaration| declaration.get("id").and_then(|id| id.as_str()))
        .map(|id| serde_json::json!(id)).collect();
    let default_kinds = if declared_ids.is_empty() { serde_json::json!(["model"]) } else { serde_json::Value::Array(declared_ids) };
    // The entry module is optional, and a missing file is for the frontend
    // loader to report.
    let entry = doc.get("entry").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let entry_abs_path = if entry.is_empty() { String::new() } else { plugin_dir.join(&entry).to_string_lossy().to_string() };

    Some(PluginManifestDto {
        icon: doc.get("icon").and_then(|v|v.as_str()).map(String::from),
        asset_icons: doc.get("assetIcons").cloned().unwrap_or_else(||serde_json::json!({})),
        id,
        name,
        version,
        entry,
        description: doc.get("description").and_then(|v| v.as_str()).map(String::from),
        permissions: doc.get("permissions").cloned().unwrap_or_else(|| serde_json::json!({})),
        asset_kinds: doc.get("assetKinds").cloned().unwrap_or(default_kinds),
        asset_types,
        fields: doc.get("fields").cloned().unwrap_or_else(|| serde_json::json!([])),
        settings: doc.get("settings").filter(|v| v.is_array()).cloned().unwrap_or_else(|| serde_json::json!([])),
        ui: doc.get("ui").cloned().unwrap_or_else(|| serde_json::json!({})),
        entry_abs_path,
        enabled: true,
        dependencies: parse_dependencies(doc.get("dependencies")),
        blocked_by: Vec::new(),
        required_by: Vec::new(),
        replaces: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_plugins_with_or_without_their_entry_file() {
        let dir=std::env::temp_dir().join(format!("toybox-rig-plugin-test-{}",std::process::id())).join("example.rigs");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"),r#"{"id":"example.rigs","name":"Rigs","version":"1.0.0","entry":"index.js"}"#).unwrap();
        let missing=read_manifest(&dir).unwrap();
        assert_eq!(missing.entry,"index.js");
        assert!(missing.entry_abs_path.ends_with("index.js"));
        std::fs::write(dir.join("index.js"),"export default {profiles:[]}").unwrap();
        assert_eq!(read_manifest(&dir).unwrap().entry,"index.js");
        std::fs::write(dir.join("manifest.json"),r#"{"id":"example.rigs","name":"Rigs","version":"1.0.0"}"#).unwrap();
        let bare=read_manifest(&dir).unwrap();
        assert_eq!((bare.entry.as_str(),bare.entry_abs_path.as_str()),("",""));
        assert!(read_manifest(&dir.parent().unwrap().join("nowhere")).is_none());
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn asset_type_plugins_work_with_the_types_they_declare() {
        let dir=std::env::temp_dir().join(format!("toybox-type-plugin-test-{}",std::process::id())).join("example.skies");
        std::fs::create_dir_all(&dir).unwrap();
        // The entry module is optional: a natively implemented type needs none.
        std::fs::write(dir.join("manifest.json"),r#"{"id":"example.skies","name":"Skies","version":"1.0.0","assetTypes":[{"id":"skybox","label":"Skyboxes"},{"label":"no id"}]}"#).unwrap();
        let manifest=read_manifest(&dir).unwrap();
        assert_eq!(manifest.asset_kinds,serde_json::json!(["skybox"]));
        assert_eq!(manifest.asset_types[0]["label"],"Skyboxes");
        // Explicit kinds win; a plugin declaring no types works with models.
        std::fs::write(dir.join("manifest.json"),r#"{"id":"example.skies","name":"Skies","version":"1.0.0","assetKinds":["skybox","model"],"assetTypes":[{"id":"skybox","label":"Skyboxes"}]}"#).unwrap();
        assert_eq!(read_manifest(&dir).unwrap().asset_kinds,serde_json::json!(["skybox","model"]));
        std::fs::write(dir.join("manifest.json"),r#"{"id":"example.skies","name":"Skies","version":"1.0.0"}"#).unwrap();
        let plain=read_manifest(&dir).unwrap();
        assert_eq!(plain.asset_kinds,serde_json::json!(["model"]));
        assert_eq!(plain.asset_types,serde_json::json!([]));
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    fn plugin(id: &str, version: &str, dependencies: serde_json::Value) -> PluginManifestDto {
        let doc = serde_json::json!({ "id": id, "name": format!("Name of {id}"), "version": version, "dependencies": dependencies });
        parse_manifest(id, &doc, Path::new("plugins").join(id).as_path()).unwrap()
    }

    fn resolved(mut manifests: Vec<PluginManifestDto>, disabled: &[&str]) -> BTreeMap<String, PluginManifestDto> {
        let disabled: Vec<String> = disabled.iter().map(|id| id.to_string()).collect();
        resolve_dependencies(&mut manifests, &disabled);
        manifests.into_iter().map(|manifest| (manifest.id.clone(), manifest)).collect()
    }

    fn reasons(manifest: &PluginManifestDto) -> Vec<(&str, &str)> {
        manifest.blocked_by.iter().map(|blocked| (blocked.id.as_str(), blocked.reason.as_str())).collect()
    }

    #[test]
    fn a_dependency_that_is_not_installed_blocks_the_plugin() {
        let all = resolved(vec![plugin("a", "1.0.0", serde_json::json!({ "gone": "^1" }))], &[]);
        assert_eq!(reasons(&all["a"]), vec![("gone", "missing")]);
        assert!(all["a"].enabled);
    }

    #[test]
    fn a_dependency_outside_the_version_range_blocks_the_plugin() {
        let all = resolved(vec![
            plugin("a", "1.0.0", serde_json::json!({ "b": "^2.0.0", "c": ">=1.0.0", "d": "not a range" })),
            plugin("b", "1.4.0", serde_json::json!({})),
            plugin("c", "1.0.0", serde_json::json!({})),
            plugin("d", "1.0.0", serde_json::json!({})),
        ], &[]);
        assert_eq!(reasons(&all["a"]), vec![("b", "version"), ("d", "version")]);
        assert_eq!(all["a"].blocked_by[0].needs.as_deref(), Some("^2.0.0"));
        assert_eq!(all["a"].blocked_by[0].found.as_deref(), Some("1.4.0"));
        let json = serde_json::to_value(&all["a"].blocked_by[0]).unwrap();
        assert_eq!(json, serde_json::json!({ "id": "b", "reason": "version", "needs": "^2.0.0", "found": "1.4.0" }));
        assert_eq!(serde_json::to_value(Blocked { id: "x".into(), reason: "missing".into(), needs: None, found: None }).unwrap(), serde_json::json!({ "id": "x", "reason": "missing" }));
    }

    #[test]
    fn a_switched_off_dependency_blocks_the_plugin_but_not_itself() {
        let all = resolved(vec![plugin("a", "1.0.0", serde_json::json!({ "b": "*" })), plugin("b", "1.0.0", serde_json::json!({}))], &["b"]);
        assert_eq!(reasons(&all["a"]), vec![("b", "disabled")]);
        assert!(all["a"].enabled && !all["b"].enabled);
        assert!(all["b"].blocked_by.is_empty());
    }

    #[test]
    fn a_blocked_dependency_blocks_what_depends_on_it() {
        let all = resolved(vec![
            plugin("a", "1.0.0", serde_json::json!({ "b": "*" })),
            plugin("b", "1.0.0", serde_json::json!({ "c": "*" })),
            plugin("c", "1.0.0", serde_json::json!({})),
        ], &["c"]);
        assert_eq!(reasons(&all["b"]), vec![("c", "disabled")]);
        assert_eq!(reasons(&all["a"]), vec![("b", "blocked")]);
        assert!(all["c"].blocked_by.is_empty());
    }

    #[test]
    fn plugins_that_depend_on_each_other_are_all_blocked_as_a_cycle() {
        let all = resolved(vec![
            plugin("a", "1.0.0", serde_json::json!({ "b": "*" })),
            plugin("b", "1.0.0", serde_json::json!({ "a": "*" })),
            plugin("c", "1.0.0", serde_json::json!({ "d": "*" })),
            plugin("d", "1.0.0", serde_json::json!({ "e": "*" })),
            plugin("e", "1.0.0", serde_json::json!({ "c": "*" })),
            plugin("f", "1.0.0", serde_json::json!({ "a": "*" })),
        ], &[]);
        assert_eq!(reasons(&all["a"]), vec![("b", "cycle")]);
        assert_eq!(reasons(&all["b"]), vec![("a", "cycle")]);
        for (id, other) in [("c", "d"), ("d", "e"), ("e", "c")] {
            assert_eq!(reasons(&all[id]), vec![(other, "cycle")]);
        }
        // Outside the cycle, the plugin is blocked by a blocked dependency.
        assert_eq!(reasons(&all["f"]), vec![("a", "blocked")]);
    }

    #[test]
    fn required_by_counts_only_switched_on_dependents() {
        let all = resolved(vec![
            plugin("core", "1.0.0", serde_json::json!({})),
            plugin("zed", "1.0.0", serde_json::json!({ "core": "*" })),
            plugin("amy", "1.0.0", serde_json::json!({ "core": "*" })),
            plugin("off", "1.0.0", serde_json::json!({ "core": "*" })),
        ], &["off"]);
        assert_eq!(all["core"].required_by, vec!["amy", "zed"]);
        assert!(all["amy"].required_by.is_empty());
    }

    #[test]
    fn dependencies_may_be_listed_as_an_array_of_ids() {
        let all = resolved(vec![plugin("a", "1.0.0", serde_json::json!(["b", "gone"])), plugin("b", "3.1.0", serde_json::json!({}))], &[]);
        assert_eq!(all["a"].dependencies, BTreeMap::from([("b".to_string(), "*".to_string()), ("gone".to_string(), "*".to_string())]));
        assert_eq!(reasons(&all["a"]), vec![("gone", "missing")]);
        assert_eq!(all["b"].required_by, vec!["a"]);
        let json = serde_json::to_value(&all["a"]).unwrap();
        assert!(json.get("dependencies").is_some() && json.get("blockedBy").is_some() && json.get("requiredBy").is_some() && json.get("kind").is_none());
    }

    fn strings(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    fn model_family() -> Vec<PluginManifestDto> {
        let named = |id: &str, name: &str, deps: serde_json::Value| {
            let mut manifest = plugin(id, "1.0.0", deps);
            manifest.name = name.to_string();
            manifest
        };
        vec![
            named("core", "Models", serde_json::json!({})),
            named("fbx", "FBX models", serde_json::json!({ "core": "*" })),
            named("obj", "OBJ models", serde_json::json!(["core"])),
            named("deep", "Deep", serde_json::json!({ "fbx": "*" })),
        ]
    }

    #[test]
    fn turning_a_plugin_on_turns_on_what_it_needs() {
        let plan = plan_plugin_enabled(&model_family(), &strings(&["core", "fbx", "other"]), "deep", true).unwrap();
        assert_eq!(plan.disabled, strings(&["other"]));
        assert_eq!(plan.changed, strings(&["core", "fbx"]));
        // Missing dependencies are skipped.
        let mut family = model_family();
        family[3].dependencies.insert("gone".into(), "*".into());
        let plan = plan_plugin_enabled(&family, &strings(&["deep"]), "deep", true).unwrap();
        assert!(plan.disabled.is_empty());
        assert_eq!(plan.changed, strings(&["deep"]));
    }

    #[test]
    fn a_plugin_that_others_need_cannot_be_turned_off() {
        let family = model_family();
        let error = plan_plugin_enabled(&family, &[], "core", false).unwrap_err().to_string();
        assert_eq!(error, "Models is needed by FBX models and OBJ models. Turn them off first.");
        let error = plan_plugin_enabled(&family, &strings(&["obj", "deep"]), "core", false).unwrap_err().to_string();
        assert_eq!(error, "Models is needed by FBX models. Turn it off first.");
        let mut more = model_family();
        more.push(plugin("zzz", "1.0.0", serde_json::json!({ "core": "*" })));
        more[4].name = "Zed".into();
        let error = plan_plugin_enabled(&more, &[], "core", false).unwrap_err().to_string();
        assert_eq!(error, "Models is needed by FBX models, OBJ models and Zed. Turn them off first.");
    }

    #[test]
    fn a_plugin_can_be_turned_off_once_its_dependents_are_off() {
        let plan = plan_plugin_enabled(&model_family(), &strings(&["fbx", "obj"]), "core", false).unwrap();
        assert_eq!(plan.disabled, strings(&["core", "fbx", "obj"]));
        assert_eq!(plan.changed, strings(&["core"]));
        // Turning off something nothing needs, or turning off twice, is fine.
        let plan = plan_plugin_enabled(&model_family(), &[], "deep", false).unwrap();
        assert_eq!((plan.disabled, plan.changed), (strings(&["deep"]), strings(&["deep"])));
        let plan = plan_plugin_enabled(&model_family(), &strings(&["deep"]), "deep", false).unwrap();
        assert_eq!((plan.disabled, plan.changed), (strings(&["deep"]), Vec::new()));
    }

    #[test]
    fn an_unknown_plugin_id_is_a_plain_switch() {
        let plan = plan_plugin_enabled(&model_family(), &strings(&["b"]), "never.installed", false).unwrap();
        assert_eq!(plan.disabled, strings(&["b", "never.installed"]));
        assert_eq!(plan.changed, strings(&["never.installed"]));
        let plan = plan_plugin_enabled(&model_family(), &strings(&["b", "never.installed"]), "never.installed", true).unwrap();
        assert_eq!((plan.disabled, plan.changed), (strings(&["b"]), strings(&["never.installed"])));
    }

    #[test]
    fn a_retired_plugin_is_not_listed_while_its_replacement_is_installed() {
        let retired: [Retirement; 1] = [("old", &["new"])];
        let mut both = vec![plugin("old", "1.0.0", serde_json::json!({})), plugin("new", "1.0.0", serde_json::json!({})), plugin("keep", "1.0.0", serde_json::json!({}))];
        drop_retired(&mut both, &retired);
        assert_eq!(both.iter().map(|manifest| manifest.id.as_str()).collect::<Vec<_>>(), vec!["new", "keep"]);
        let mut alone = vec![plugin("old", "1.0.0", serde_json::json!({})), plugin("keep", "1.0.0", serde_json::json!({}))];
        drop_retired(&mut alone, &retired);
        assert_eq!(alone.len(), 2);
    }

    #[test]
    fn a_retired_plugin_with_several_replacements_waits_for_all_of_them() {
        let retired: [Retirement; 2] = [("old", &["a", "b"]), ("gone", &[])];
        let list = |ids: &[&str]| ids.iter().map(|id| plugin(id, "1.0.0", serde_json::json!({}))).collect::<Vec<_>>();
        let ids = |manifests: &[PluginManifestDto]| manifests.iter().map(|manifest| manifest.id.clone()).collect::<Vec<_>>();
        let mut partly = list(&["old", "a"]);
        drop_retired(&mut partly, &retired);
        assert_eq!(ids(&partly), vec!["old", "a"], "one replacement is not enough");
        let mut fully = list(&["old", "a", "b", "keep"]);
        drop_retired(&mut fully, &retired);
        assert_eq!(ids(&fully), vec!["a", "b", "keep"]);
        // Retired with nothing in its place: never listed, even if a copy is still on disk.
        let mut dropped = list(&["gone", "keep"]);
        drop_retired(&mut dropped, &retired);
        assert_eq!(ids(&dropped), vec!["keep"]);
    }

    #[test]
    fn a_plugin_lists_the_retired_ids_it_replaces() {
        let retired: [Retirement; 2] = [("old", &["a", "b"]), ("older", &["b"])];
        let mut list = vec![plugin("a", "1.0.0", serde_json::json!({})), plugin("b", "1.0.0", serde_json::json!({})), plugin("c", "1.0.0", serde_json::json!({}))];
        mark_replaced(&mut list, &retired);
        let replaced: Vec<_> = list.iter().map(|manifest| manifest.replaces.clone()).collect();
        let strings = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        assert_eq!(replaced, vec![strings(&["old"]), strings(&["old", "older"]), Vec::<String>::new()]);
        // Only plugins that replace something carry the field over to the frontend.
        assert!(serde_json::to_value(&list[2]).unwrap().get("replaces").is_none());
        assert_eq!(serde_json::to_value(&list[1]).unwrap()["replaces"], serde_json::json!(["old", "older"]));
    }

}
