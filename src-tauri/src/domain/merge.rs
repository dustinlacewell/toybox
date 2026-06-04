//! Idempotent re-scan: a freshly seeded catalog carries default `user`/`thumb`
//! metadata. Merging preserves the app-owned metadata of assets that already
//! existed (keyed by `id`), so re-scanning never discards favorites, user tags,
//! or thumbnail state.

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
            fileset: AssetFileset { gltf: "g".into(), bin: "b".into(), textures: vec![] },
            thumb: ThumbMeta { state: ThumbState::Ready, error: None },
            user: UserMeta { favorite: fav, tags: vec![] },
            animation: AnimationMeta::default(),
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
}
