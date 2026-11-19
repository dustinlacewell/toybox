//! What a front end asks of its libraries (see `docs/sqlite-storage.md`, "App queries"):
//! a page of the list, counts, assets by id, and the small writes of personal data.
//! The queries run on all readable libraries together, ranked as the window ranks
//! them (`libraryRanks` in `src/domain/duplicates.ts`).

use serde::{Deserialize, Serialize};
use toybox_core::domain::catalog_model::Asset;
use toybox_core::domain::rig::Rig;
use toybox_core::error::{AppError, AppResult};
pub use toybox_core::infra::store::browse::{BrowseFilter, CollectionCount, Count, Elsewhere, FacetCounts, Visibility};
use toybox_core::infra::store::Store;
use toybox_core::ops::browse::{self, AssetPage, BrowseLibrary, IdPage, ItemRef, LibrarySummary, Row};

use crate::host::Host;

const UNAVAILABLE: i64 = 1_000_000;
const REMOTE: i64 = 1_000;

/// The rigs of a library and the models and animations that bind to one: what a viewer needs to
/// match an animation to a model.
#[derive(Serialize)]
pub struct RigCatalog {
    pub rigs: Vec<Rig>,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingThumbsRequest {
    pub library: String,
    pub after: String,
    pub limit: usize,
}

#[derive(Serialize)]
pub struct PendingThumbs {
    pub ids: Vec<String>,
    pub next: Option<String>,
}

/// Take personal tags off items: they were moved into the library as library tags.
#[derive(Deserialize)]
pub struct PersonalTagRemoval {
    pub id: String,
    pub tags: Vec<String>,
}

impl Host {
    /// Every readable library with its rank: the default first, then the other local libraries, then remote
    /// ones (each in registry order); one that is offline comes after the rest.
    pub fn browse_libraries(&self) -> AppResult<Vec<BrowseLibrary>> {
        let mut out = Vec::new();
        for (index, info) in self.list_libraries().into_iter().enumerate() {
            if !info.available {
                continue;
            }
            let down = info.offline || info.remote.as_ref().is_some_and(|remote| remote.offline);
            let base = if info.is_default { 0 } else if info.remote.is_some() { REMOTE + index as i64 } else { 1 + index as i64 };
            out.push(BrowseLibrary { handle: self.resolve(&info.id)?, rank: base + if down { UNAVAILABLE } else { 0 } });
        }
        Ok(out)
    }

    /// Make a library ready to be read, and say where it stands. A local library finishes its migrations and
    /// builds its catalog if it has none that is current (`force`: rebuilds it). A remote library is brought up to
    /// date from its server (`force`: downloaded again); when the server cannot be reached its replica is used.
    pub fn open_library(&self, library: &str, force: bool) -> AppResult<LibrarySummary> {
        if let Some(remote) = self.remote_if(library)? {
            let handle = self.resolve(library)?;
            remote.refresh(force).map_err(AppError::from)?;
            return browse::summary(&handle);
        }
        let handle = self.resolve(library)?;
        if force {
            toybox_core::ops::scan::scan_library_catalog(&handle)?;
        }
        browse::prepare(&handle)
    }

    /// A page of the list the filter shows, in name order, and how long the list is.
    pub fn query_assets(&self, filter: &BrowseFilter, descending: bool, offset: i64, limit: i64) -> AppResult<AssetPage> {
        browse::query_assets(&self.browse_libraries()?, filter, descending, offset, limit)
    }

    /// How many items the filter shows, exactly. `query_assets` answers with a lower bound (`exact: false`) for a broad search.
    pub fn count_assets(&self, filter: &BrowseFilter) -> AppResult<i64> {
        browse::count_assets(&self.browse_libraries()?, filter)
    }

    /// The ids of the items the filter shows from `offset` on, at most `cap`.
    pub fn query_ids(&self, filter: &BrowseFilter, descending: bool, offset: i64, cap: i64) -> AppResult<IdPage> {
        browse::query_ids(&self.browse_libraries()?, filter, descending, offset, cap)
    }

    /// Items (or variants) by id, in the order asked; ids that are not there are left out.
    pub fn get_assets(&self, items: &[ItemRef]) -> AppResult<Vec<Row>> {
        browse::get_assets(&self.browse_libraries()?, items)
    }

    /// The rail's numbers for one type.
    pub fn facet_counts(&self, kind: &str, visibility: &Visibility, tag_limit: usize, selected_tags: &[String]) -> AppResult<FacetCounts> {
        browse::facet_counts(&self.browse_libraries()?, kind, visibility, tag_limit, selected_tags)
    }

    /// The tags of a type that contain `text`, most used first.
    pub fn search_tags(&self, kind: &str, visibility: &Visibility, text: &str, limit: usize) -> AppResult<Vec<Count>> {
        browse::search_tags(&self.browse_libraries()?, kind, visibility, text, limit)
    }

    /// Items per type, each once.
    pub fn type_counts(&self, visibility: &Visibility) -> AppResult<Vec<Count>> {
        browse::type_counts(&self.browse_libraries()?, visibility)
    }

    /// The collections and how many items of `kind` each holds.
    pub fn list_collections(&self, kind: &str, visibility: &Visibility) -> AppResult<Vec<CollectionCount>> {
        browse::collections(&self.browse_libraries()?, kind, visibility)
    }

    /// Where else these items live, and which copies are another version of them.
    pub fn elsewhere(&self, items: &[ItemRef], visibility: &Visibility) -> AppResult<Vec<Elsewhere>> {
        browse::elsewhere(&self.browse_libraries()?, items, visibility)
    }

    /// The rigs of a library (a handful).
    pub fn list_rigs(&self, library: &str) -> AppResult<Vec<Rig>> {
        Store::for_handle(&self.resolve(library)?).all_rigs()
    }

    pub fn rig_catalog(&self, library: &str) -> AppResult<RigCatalog> {
        let handle = self.resolve(library)?;
        let store = Store::for_handle(&handle);
        Ok(RigCatalog { rigs: store.all_rigs()?, assets: store.rigged_assets_all()? })
    }

    /// Items that carry personal tags from before library tags existed (at most `limit`).
    pub fn personal_tag_assets(&self, library: &str, limit: usize) -> AppResult<Vec<Asset>> {
        let handle = self.resolve(library)?;
        let mut assets = Store::for_handle(&handle).assets_with_personal_tags(limit)?;
        toybox_core::ops::content_hash::fill_assets(&handle, &mut assets);
        Ok(assets)
    }

    /// Up to `limit` items of a library, after id `after`, that have no picture yet (or a stale one).
    pub fn pending_thumbs(&self, request: &PendingThumbsRequest) -> AppResult<PendingThumbs> {
        let handle = self.resolve(&request.library)?;
        let (ids, next) = browse::pending_thumbs(&handle, &request.after, request.limit)?;
        Ok(PendingThumbs { ids, next })
    }

    /// Mark items as favourites (or not). Personal data: only this computer's database is written.
    pub fn set_favorite(&self, library: &str, ids: &[String], favorite: bool) -> AppResult<()> {
        Store::for_handle(&self.resolve(library)?).set_favorite(ids, favorite)
    }

    /// Put items into a collection (or take them out).
    pub fn set_collection(&self, library: &str, ids: &[String], name: &str, member: bool) -> AppResult<()> {
        let handle = self.resolve(library)?;
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::msg("A collection needs a name"));
        }
        Store::for_handle(&handle).set_collection_member(ids, &name, member)
    }

    pub fn drop_personal_tags(&self, library: &str, removals: Vec<PersonalTagRemoval>) -> AppResult<()> {
        let handle = self.resolve(library)?;
        let pairs: Vec<(String, Vec<String>)> = removals.into_iter().map(|removal| (removal.id, removal.tags)).collect();
        Store::for_handle(&handle).drop_personal_tags(&pairs)
    }

    /// Choose the asset an item is previewed on (`None` clears the choice).
    pub fn set_preview_choice(&self, library: &str, asset_id: &str, choice: Option<&str>) -> AppResult<()> {
        Store::for_handle(&self.resolve(library)?).set_preview_choice(asset_id, choice)
    }
}
