//! Idempotent re-scan: a freshly seeded catalog carries default `user`/`thumb`
//! metadata. Merging preserves the app-owned metadata of assets that already
//! existed (keyed by `id`), so re-scanning never discards favorites, user tags,
//! or thumbnail state. `upsert_assets` is the additive counterpart used by an
//! import: it adds assets to an existing catalog without dropping any.

use std::collections::HashSet;

use crate::domain::catalog_model::{Asset, Catalog};

/// Produce a catalog with `fresh`'s asset set but `prior`'s `user`/`thumb` for
/// any asset whose `id` survived. New assets keep their (default) metadata;
/// removed assets simply drop out.
pub fn merge_preserving_user(prior: &Catalog, fresh: Catalog) -> Catalog {
    let ids: HashSet<_> = fresh.assets.iter().map(|asset| asset.id.clone()).collect();
    let mut merged = upsert_assets(prior.clone(), fresh.assets);
    // Full scans prune removed entries and obsolete derived clips, while the
    // upsert also carries metadata from formerly separate variant tiles.
    merged.assets.retain(|asset| ids.contains(&asset.id));
    Catalog { rigs: fresh.rigs, assets: merged.assets, ..fresh_meta(fresh.schema_version, fresh.library_root) }
}

/// Whether `old` is a row that `fresh` absorbs: the same asset, or (of the same
/// kind) a formerly separate row that is now one of `fresh`'s variants or the
/// group `fresh` is the primary of.
pub fn is_old_member(fresh: &Asset, old: &Asset) -> bool {
    old.id == fresh.id
        || (std::mem::discriminant(&old.content) == std::mem::discriminant(&fresh.content)
            && ((!fresh.variants.is_empty() && old.rel_path == fresh.rel_path)
                || fresh.variants.iter().any(|v| v.id == old.id || v.rel_path == old.rel_path)))
}

/// The ids and relative paths under which rows that `fresh` may absorb are stored.
pub fn old_member_keys(fresh: &Asset) -> (Vec<String>, Vec<String>) {
    let mut ids = vec![fresh.id.clone()];
    let mut paths = Vec::new();
    if !fresh.variants.is_empty() {
        paths.push(fresh.rel_path.clone());
    }
    for variant in &fresh.variants {
        ids.push(variant.id.clone());
        paths.push(variant.rel_path.clone());
    }
    (ids, paths)
}

/// Carry what the app owns (provenance, preview choice, favourite, personal tags,
/// collections, thumbnail) from the rows `fresh` absorbs onto `fresh`.
pub fn absorb(a: &mut Asset, old_members: &[&Asset]) {
    for old in old_members {
        if a.user.provenance.is_none(){a.user.provenance=old.user.provenance.clone();}
        if a.user.preview_asset_id.is_none() { a.user.preview_asset_id = old.user.preview_asset_id.clone(); }
        a.user.favorite |= old.user.favorite;
        let superseded_tags=if a.user.provenance!=old.user.provenance {old.user.provenance.as_ref().map(|value|value.tags()).unwrap_or_default()}else{Vec::new()};
        for tag in &old.user.tags {
            if superseded_tags.contains(tag){continue;}
            if !a.user.tags.contains(tag) { a.user.tags.push(tag.clone()); }
        }
        for collection in &old.user.collections {
            if !a.user.collections.contains(collection) { a.user.collections.push(collection.clone()); }
        }
        if old.rel_path == a.rel_path { a.thumb = old.thumb.clone(); }
    }
}

/// Add `fresh` assets to `prior` without dropping anything. A fresh asset whose
/// `id` already exists replaces it in place, keeping the prior `user`/`thumb`;
/// the rest are appended. `schema_version` and `library_root` stay `prior`'s.
pub fn upsert_assets(prior: Catalog, fresh: Vec<Asset>) -> Catalog {
    use std::collections::HashMap;
    // Rows are looked up by id and by path, not by scanning: a tombstone marks a removed row.
    let mut assets: Vec<Option<Asset>> = prior.assets.into_iter().map(Some).collect();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    let mut by_rel: HashMap<String, Vec<usize>> = HashMap::new();
    for (position, asset) in assets.iter().enumerate() {
        let asset = asset.as_ref().expect("just built");
        by_id.entry(asset.id.clone()).or_insert(position);
        by_rel.entry(asset.rel_path.clone()).or_default().push(position);
    }
    for mut a in fresh {
        let (ids, paths) = old_member_keys(&a);
        let mut candidates: Vec<usize> = Vec::new();
        for id in &ids {
            candidates.extend(by_id.get(id));
        }
        for path in &paths {
            candidates.extend(by_rel.get(path).into_iter().flatten());
        }
        candidates.sort_unstable();
        candidates.dedup();
        candidates.retain(|&position| assets[position].as_ref().is_some_and(|old| is_old_member(&a, old)));
        let members: Vec<usize> = candidates;
        {
            let old_members: Vec<&Asset> = members.iter().filter_map(|&position| assets[position].as_ref()).collect();
            absorb(&mut a, &old_members);
        }
        for &position in &members {
            if assets[position].as_ref().is_some_and(|old| old.id != a.id) {
                assets[position] = None;
            }
        }
        match by_id.get(&a.id).copied().filter(|&position| assets[position].is_some()) {
            Some(position) => {
                if let Some(old) = assets[position].take() {
                    if old.rel_path == a.rel_path { a.thumb = old.thumb.clone(); }
                    if old.rel_path != a.rel_path {
                        if let Some(list) = by_rel.get_mut(&old.rel_path) { list.retain(|&p| p != position); }
                        by_rel.entry(a.rel_path.clone()).or_default().push(position);
                    }
                }
                assets[position] = Some(a);
            }
            None => {
                let position = assets.len();
                by_id.insert(a.id.clone(), position);
                by_rel.entry(a.rel_path.clone()).or_default().push(position);
                assets.push(Some(a));
            }
        }
    }
    Catalog {
        rigs: prior.rigs,
        schema_version: prior.schema_version,
        library_root: prior.library_root,
        assets: assets.into_iter().flatten().collect(),
    }
}

fn fresh_meta(schema_version: u32, library_root: String) -> Catalog {
    Catalog { rigs: Vec::new(), schema_version, library_root, assets: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::catalog_model::*;

    fn asset(id: &str, fav: bool) -> Asset {
        Asset {
            rigs: Vec::new(),
            variants: Vec::new(),
            id: id.into(),
            name: "n".into(),
            file_name: "n.gltf".into(),
            rel_path: "r".into(),
            pack: "p".into(),
            category: "c".into(),
            content: AssetContent::Model {
                fileset: AssetFileset { gltf: "g".into(), bin: "b".into(), textures: vec![] },
                animation: AnimationMeta::default(),
            },
            thumb: ThumbMeta { state: ThumbState::Ready, error: None },
            user: UserMeta { provenance: None, preview_asset_id: None, favorite: fav, tags: vec![], collections: vec![] },
            tags: vec![],
            origin: None,
            format: None,
            content_hash: None,
        }
    }

    #[test]
    fn grouping_absorbs_old_rows_and_keeps_their_favorites() {
        let mut full = asset("full", false); full.rel_path = "full.gltf".into();
        let mut low = asset("low", true); low.rel_path = "low.gltf".into();
        low.user.tags.push("metal".into());
        low.user.collections.push("Spygame".into());
        let mut group = full.clone(); group.id = "group".into(); group.variants.push(low.clone());
        let prior = Catalog { rigs: Vec::new(), schema_version: 3, library_root: "root".into(), assets: vec![full, low] };
        let fresh = Catalog { rigs: Vec::new(), schema_version: 4, library_root: "root".into(), assets: vec![group.clone()] };
        let rescanned = merge_preserving_user(&prior, fresh);
        assert_eq!(rescanned.assets.len(), 1);
        assert!(rescanned.assets[0].user.favorite);
        assert_eq!(rescanned.assets[0].user.collections, vec!["Spygame"]);
        assert_eq!(rescanned.schema_version, 4);
        let result = upsert_assets(prior, vec![group]);
        assert_eq!(result.assets.len(), 1);
        assert!(result.assets[0].user.favorite);
        assert_eq!(result.assets[0].user.tags, vec!["metal"]);
        assert_eq!(result.assets[0].user.collections, vec!["Spygame"]);
        assert_eq!(result.assets[0].variants.len(), 1);
    }

    #[test]
    fn a_rescan_takes_library_tags_from_the_manifest_and_keeps_personal_ones() {
        let mut cached = asset("a", false);
        cached.tags = vec!["stale".into()];
        cached.user.tags = vec!["mine".into()];
        let prior = Catalog { rigs: Vec::new(), schema_version: 1, library_root: "root".into(), assets: vec![cached] };
        let mut fresh_a = asset("a", false);
        fresh_a.tags = vec!["footstep".into()];
        let fresh = Catalog { rigs: Vec::new(), schema_version: 1, library_root: "root".into(), assets: vec![fresh_a] };
        let merged = merge_preserving_user(&prior, fresh);
        assert_eq!(merged.assets[0].tags, vec!["footstep"]);
        assert_eq!(merged.assets[0].user.tags, vec!["mine"]);
    }

    #[test]
    fn preserves_user_and_thumb_by_id() {
        let prior = Catalog {
            rigs: Vec::new(),
            schema_version: 1,
            library_root: "root".into(),
            assets: vec![asset("a", true)],
        };
        // Fresh seed: same id but default (missing thumb, not favorite).
        let mut fresh_a = asset("a", false);
        fresh_a.thumb = ThumbMeta::default();
        let fresh = Catalog {
            rigs: Vec::new(),
            schema_version: 1,
            library_root: "root".into(),
            assets: vec![fresh_a, asset("b", false)],
        };

        let merged = merge_preserving_user(&prior, fresh);
        let a = merged.assets.iter().find(|x| x.id == "a").unwrap();
        assert!(a.user.favorite, "favorite preserved");
        assert!(matches!(a.thumb.state, ThumbState::Ready), "thumb state preserved");
        assert_eq!(merged.assets.len(), 2, "new asset b retained");
    }

    fn catalog(root: &str, assets: Vec<Asset>) -> Catalog {
        Catalog { rigs: Vec::new(), schema_version: 1, library_root: root.into(), assets }
    }

    fn ids(c: &Catalog) -> Vec<&str> {
        c.assets.iter().map(|a| a.id.as_str()).collect()
    }

    #[test]
    fn upsert_keeps_prior_assets_and_appends_new_ones() {
        let prior = catalog("old", vec![asset("a", true), asset("b", false)]);
        let merged = upsert_assets(prior, vec![asset("c", false)]);
        assert_eq!(ids(&merged), ["a", "b", "c"]);
    }

    #[test]
    fn upsert_replaces_same_id_in_place_keeping_user_and_thumb() {
        let prior = catalog("old", vec![asset("a", true), asset("b", false)]);
        let mut fresh_a = asset("a", false);
        fresh_a.name = "renamed".into();
        fresh_a.thumb = ThumbMeta::default();

        let merged = upsert_assets(prior, vec![fresh_a]);
        assert_eq!(ids(&merged), ["a", "b"]);
        let a = &merged.assets[0];
        assert_eq!(a.name, "renamed", "fresh content wins");
        assert!(a.user.favorite, "user preserved");
        assert!(matches!(a.thumb.state, ThumbState::Ready), "thumb preserved");
    }

    #[test]
    fn upsert_takes_schema_version_and_root_from_prior() {
        let prior = Catalog {
            rigs: Vec::new(),
            schema_version: 7,
            library_root: "prior-root".into(),
            assets: vec![],
        };
        let merged = upsert_assets(prior, vec![asset("a", false)]);
        assert_eq!(merged.schema_version, 7);
        assert_eq!(merged.library_root, "prior-root");
    }
}
