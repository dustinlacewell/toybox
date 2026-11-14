//! Idempotent re-scan: a freshly seeded catalog carries default `user`/`thumb`
//! metadata. Merging preserves the app-owned metadata of assets that already
//! existed (keyed by `id`), so re-scanning never discards favorites, user tags,
//! or thumbnail state. `upsert_assets` is the additive counterpart used by an
//! import: it adds assets to an existing catalog without dropping any.

use std::collections::HashMap;

use crate::domain::catalog_model::{Asset, Catalog};

/// Produce a catalog with `fresh`'s asset set but `prior`'s `user`/`thumb` for
/// any asset whose `id` survived. New assets keep their (default) metadata;
/// removed assets simply drop out.
pub fn merge_preserving_user(prior: &Catalog, fresh: Catalog) -> Catalog {
    let prior_by_id: HashMap<&str, &Asset> =
        prior.assets.iter().map(|a| (a.id.as_str(), a)).collect();

    let assets = fresh
        .assets
        .into_iter()
        .map(|mut a| {
            if let Some(old) = prior_by_id.get(a.id.as_str()) {
                a.user = old.user.clone();
                a.thumb = old.thumb.clone();
            }
            a
        })
        .collect();

    Catalog { assets, ..fresh_meta(fresh.schema_version, fresh.library_root) }
}

/// Add `fresh` assets to `prior` without dropping anything. A fresh asset whose
/// `id` already exists replaces it in place, keeping the prior `user`/`thumb`;
/// the rest are appended. `schema_version` and `library_root` stay `prior`'s.
pub fn upsert_assets(prior: Catalog, fresh: Vec<Asset>) -> Catalog {
    let mut assets = prior.assets;
    for mut a in fresh {
        match assets.iter().position(|old| old.id == a.id) {
            Some(i) => {
                a.user = assets[i].user.clone();
                a.thumb = assets[i].thumb.clone();
                assets[i] = a;
            }
            None => assets.push(a),
        }
    }
    Catalog {
        schema_version: prior.schema_version,
        library_root: prior.library_root,
        assets,
    }
}

fn fresh_meta(schema_version: u32, library_root: String) -> Catalog {
    Catalog { schema_version, library_root, assets: Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::catalog_model::*;

    fn asset(id: &str, fav: bool) -> Asset {
        Asset {
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
            user: UserMeta { favorite: fav, tags: vec![] },
        }
    }

    #[test]
    fn preserves_user_and_thumb_by_id() {
        let prior = Catalog {
            schema_version: 1,
            library_root: "root".into(),
            assets: vec![asset("a", true)],
        };
        // Fresh seed: same id but default (missing thumb, not favorite).
        let mut fresh_a = asset("a", false);
        fresh_a.thumb = ThumbMeta::default();
        let fresh = Catalog {
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
        Catalog { schema_version: 1, library_root: root.into(), assets }
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
            schema_version: 7,
            library_root: "prior-root".into(),
            assets: vec![],
        };
        let merged = upsert_assets(prior, vec![asset("a", false)]);
        assert_eq!(merged.schema_version, 7);
        assert_eq!(merged.library_root, "prior-root");
    }
}
