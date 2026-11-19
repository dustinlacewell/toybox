//! The asset types: the four the host implements, the ones plugins declare, and
//! any other type that has items (its plugin is not installed).

use toybox_core::error::AppResult;
use toybox_core::infra::store::browse::Visibility;
use toybox_core::ops::browse::BrowseLibrary;

use crate::host::Host;

/// One asset type and how many items it has.
#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub id: String,
    pub label: String,
    /// The plugin that declares the type; `None` for a built-in default or a type no installed plugin declares.
    pub plugin: Option<String>,
    /// The host implements the type itself.
    pub native: bool,
    /// Items of the type (each once), whatever is hidden.
    pub count: i64,
    /// Its plugin is off: the app shows no section and hides its items.
    pub hidden: bool,
}

const NATIVE: [(&str, &str, i64); 4] = [("model", "Models", 10), ("material", "Materials", 20), ("animation", "Animations", 30), ("audio", "Audio", 40)];

impl Host {
    /// Every known type with its count in `scope`, in the app's order.
    pub fn asset_types(&self, scope: &[BrowseLibrary]) -> AppResult<Vec<TypeInfo>> {
        let plugins = self.list_plugins()?;
        let visibility: Visibility = crate::visibility::visibility_of(&plugins);
        let mut entries: Vec<(i64, TypeInfo)> = NATIVE
            .iter()
            .map(|(id, label, order)| (*order, TypeInfo { id: id.to_string(), label: label.to_string(), plugin: None, native: true, count: 0, hidden: false }))
            .collect();
        let mut by_plugin: Vec<&crate::plugins::PluginManifestDto> = plugins.iter().collect();
        by_plugin.sort_by(|a, b| a.id.cmp(&b.id));
        for plugin in by_plugin {
            for declaration in plugin.asset_types.as_array().into_iter().flatten() {
                let (Some(id), Some(label)) = (declaration["id"].as_str(), declaration["label"].as_str()) else { continue };
                if label.trim().is_empty() {
                    continue;
                }
                let native = declaration["implementation"] == "native";
                let order = declaration["order"].as_i64().unwrap_or(100);
                match entries.iter_mut().find(|(_, entry)| entry.id == id) {
                    Some((_, existing)) if existing.plugin.is_some() => {}
                    Some((existing_order, existing)) => {
                        existing.label = label.trim().to_string();
                        existing.plugin = Some(plugin.id.clone());
                        existing.native = native;
                        *existing_order = declaration["order"].as_i64().unwrap_or(*existing_order);
                    }
                    None => entries.push((order, TypeInfo { id: id.to_string(), label: label.trim().to_string(), plugin: Some(plugin.id.clone()), native, count: 0, hidden: false })),
                }
            }
        }
        let counts = toybox_core::ops::browse::type_counts(scope, &Visibility::default())?;
        for count in counts {
            match entries.iter_mut().find(|(_, entry)| entry.id == count.value) {
                Some((_, entry)) => entry.count = count.count,
                None => {
                    let mut label = count.value.clone();
                    if let Some(first) = label.get(..1) {
                        label = first.to_uppercase() + &label[1..];
                    }
                    entries.push((1000, TypeInfo { id: count.value, label, plugin: None, native: false, count: count.count, hidden: false }));
                }
            }
        }
        for (_, entry) in &mut entries {
            entry.hidden = visibility.hidden_types.contains(&entry.id);
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.label.cmp(&b.1.label)));
        Ok(entries.into_iter().map(|(_, entry)| entry).collect())
    }
}
