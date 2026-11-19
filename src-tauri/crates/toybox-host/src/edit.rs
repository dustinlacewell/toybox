//! Editing the metadata that belongs to the library (tags, source, publisher,
//! licence, address) on many items at once, and the personal collections.

use std::collections::HashMap;

use toybox_core::domain::catalog_model::Asset;
use toybox_core::domain::tags;
use toybox_core::error::AppResult;
use toybox_core::infra::store::Store;
use toybox_core::ops::metadata::{MetadataEdit, ProvenanceEdit, MAX_EDITS};

use crate::host::Host;

/// How a list of tags changes an item's tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagChange {
    Add,
    Remove,
    Set,
}

/// The details an edit may change; `None` leaves a field alone, `Some("")` clears it.
#[derive(Debug, Clone, Default)]
pub struct DetailEdit {
    pub source: Option<String>,
    pub publisher: Option<String>,
    pub license: Option<String>,
    pub url: Option<String>,
}

/// What an edit would do, before it is applied.
#[derive(Debug, Clone, Default)]
pub struct EditPlan {
    pub edits: Vec<MetadataEdit>,
    /// Ids whose tags or details will change.
    pub changed: Vec<String>,
    /// Ids that already have the asked tags.
    pub unchanged: Vec<String>,
}

/// Split a comma-separated list of tags, trimmed, empty entries dropped.
pub fn parse_tag_list(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|tag| !tag.is_empty()).map(String::from).collect()
}

impl Host {
    /// Work out the tag edits that apply `change` with `new_tags` to the items `ids`
    /// of a library (ids must exist: see [`Host::split_known_ids`]).
    pub fn plan_tag_change(&self, library: &str, ids: &[String], change: TagChange, new_tags: &[String]) -> AppResult<EditPlan> {
        let handle = self.resolve(library)?;
        let current: HashMap<String, Asset> = Store::for_handle(&handle).load_assets(ids)?.into_iter().map(|asset| (asset.id.clone(), asset)).collect();
        let wanted = tags::normalize_tags(new_tags);
        let mut plan = EditPlan::default();
        for id in ids {
            let Some(asset) = current.get(id) else { continue };
            let before = asset.tags.clone();
            let after = match change {
                TagChange::Set => wanted.clone(),
                TagChange::Add => tags::normalize_tags(before.iter().chain(wanted.iter())),
                TagChange::Remove => before.iter().filter(|tag| !wanted.contains(tag)).cloned().collect(),
            };
            if after == before {
                plan.unchanged.push(id.clone());
            } else {
                plan.changed.push(id.clone());
                plan.edits.push(MetadataEdit { id: id.clone(), tags: Some(after), provenance: None });
            }
        }
        Ok(plan)
    }

    /// The edits that change the details of the items `ids`.
    pub fn plan_detail_change(&self, ids: &[String], details: &DetailEdit) -> EditPlan {
        let field = |value: &Option<String>| value.as_ref().map(|text| if text.trim().is_empty() { None } else { Some(text.clone()) });
        let provenance = ProvenanceEdit { source: field(&details.source), publisher: field(&details.publisher), license: field(&details.license), url: field(&details.url) };
        EditPlan {
            edits: ids.iter().map(|id| MetadataEdit { id: id.clone(), tags: None, provenance: Some(provenance.clone()) }).collect(),
            changed: ids.to_vec(),
            unchanged: Vec::new(),
        }
    }

    /// Apply edits (in batches the library accepts); returns the items they touched.
    pub fn apply_edits(&self, library: &str, edits: &[MetadataEdit]) -> AppResult<Vec<Asset>> {
        let mut touched = Vec::new();
        for chunk in edits.chunks(MAX_EDITS) {
            touched.extend(self.update_asset_metadata(library, chunk)?);
        }
        Ok(touched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_lists_split_on_commas() {
        assert_eq!(parse_tag_list(" a, b ,,c"), vec!["a", "b", "c"]);
        assert!(parse_tag_list("").is_empty());
    }
}
