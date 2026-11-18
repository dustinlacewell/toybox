//! What the window asks of its libraries (see `docs/sqlite-storage.md`, "App queries"): a page of
//! the list, counts, assets by id, and the small writes of personal data. The window never holds a
//! library's catalog. The queries run on all readable libraries together, ranked as the window
//! ranks them (`libraryRanks` in `src/domain/duplicates.ts`).

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use toybox_core::domain::catalog_model::Asset;
use toybox_core::domain::rig::Rig;
use toybox_core::infra::store::browse::{BrowseFilter, CollectionCount, Count, Elsewhere, FacetCounts, Visibility};
use toybox_core::infra::store::Store;
use toybox_core::ops::browse::{self, AssetPage, BrowseLibrary, IdPage, ItemRef, LibrarySummary, Row};

use crate::commands::remote::blocking;
use crate::error::{AppError, AppResult};
use crate::infra::library;

const UNAVAILABLE: i64 = 1_000_000;
const REMOTE: i64 = 1_000;

/// Every readable library with its rank: the default first, then the other local libraries, then remote
/// ones (each in registry order); one that is offline comes after the rest.
fn libraries(app: &AppHandle) -> AppResult<Vec<BrowseLibrary>> {
    let mut out = Vec::new();
    for (index, info) in library::list(app).into_iter().enumerate() {
        if !info.available {
            continue;
        }
        let down = info.offline || info.remote.as_ref().is_some_and(|remote| remote.offline);
        let base = if info.is_default { 0 } else if info.remote.is_some() { REMOTE + index as i64 } else { 1 + index as i64 };
        out.push(BrowseLibrary { handle: library::resolve(app, &info.id)?, rank: base + if down { UNAVAILABLE } else { 0 } });
    }
    Ok(out)
}

/// Make a library ready to be read, and say where it stands. A local library finishes its migrations and
/// builds its catalog if it has none that is current (`force`: rebuilds it). A remote library is brought up to
/// date from its server (`force`: downloaded again); when the server cannot be reached its replica is used.
#[tauri::command]
pub async fn open_library(app: AppHandle, library: String, force: bool) -> AppResult<LibrarySummary> {
    if let Some(remote) = library::remote_if(&app, &library)? {
        let handle = library::resolve(&app, &library)?;
        return blocking(move || {
            remote.refresh(force).map_err(AppError::from)?;
            browse::summary(&handle)
        })
        .await;
    }
    let handle = library::resolve(&app, &library)?;
    blocking(move || {
        if force {
            toybox_core::ops::scan::scan_library_catalog(&handle)?;
        }
        browse::prepare(&handle)
    })
    .await
}

/// A page of the list the filter shows, in name order, and how long the list is.
#[tauri::command]
pub async fn query_assets(app: AppHandle, filter: BrowseFilter, descending: bool, offset: i64, limit: i64) -> AppResult<AssetPage> {
    let libraries = libraries(&app)?;
    blocking(move || browse::query_assets(&libraries, &filter, descending, offset, limit)).await
}

/// How many items the filter shows, exactly. `query_assets` answers with a lower bound (`exact: false`) for a broad search, so
/// the first page does not wait for this pass over the list.
#[tauri::command]
pub async fn count_assets(app: AppHandle, filter: BrowseFilter) -> AppResult<i64> {
    let libraries = libraries(&app)?;
    blocking(move || browse::count_assets(&libraries, &filter)).await
}

/// The ids of the items the filter shows from `offset` on, at most `cap` (select all, a range between two clicks).
#[tauri::command]
pub async fn query_ids(app: AppHandle, filter: BrowseFilter, descending: bool, offset: i64, cap: i64) -> AppResult<IdPage> {
    let libraries = libraries(&app)?;
    blocking(move || browse::query_ids(&libraries, &filter, descending, offset, cap)).await
}

/// Items (or variants) by id, in the order asked; ids that are not there are left out.
#[tauri::command]
pub async fn get_assets(app: AppHandle, items: Vec<ItemRef>) -> AppResult<Vec<Row>> {
    let libraries = libraries(&app)?;
    blocking(move || browse::get_assets(&libraries, &items)).await
}

/// The rail's numbers for one type.
#[tauri::command]
pub async fn facet_counts(app: AppHandle, kind: String, visibility: Visibility, tag_limit: usize, selected_tags: Vec<String>) -> AppResult<FacetCounts> {
    let libraries = libraries(&app)?;
    blocking(move || browse::facet_counts(&libraries, &kind, &visibility, tag_limit, &selected_tags)).await
}

/// The tags of a type that contain `text`, most used first.
#[tauri::command]
pub async fn search_tags(app: AppHandle, kind: String, visibility: Visibility, text: String, limit: usize) -> AppResult<Vec<Count>> {
    let libraries = libraries(&app)?;
    blocking(move || browse::search_tags(&libraries, &kind, &visibility, &text, limit)).await
}

/// Items per type, each once (the sidebar).
#[tauri::command]
pub async fn type_counts(app: AppHandle, visibility: Visibility) -> AppResult<Vec<Count>> {
    let libraries = libraries(&app)?;
    blocking(move || browse::type_counts(&libraries, &visibility)).await
}

/// The collections and how many items of `kind` each holds.
#[tauri::command]
pub async fn list_collections(app: AppHandle, kind: String, visibility: Visibility) -> AppResult<Vec<CollectionCount>> {
    let libraries = libraries(&app)?;
    blocking(move || browse::collections(&libraries, &kind, &visibility)).await
}

/// Where else these items live, and which copies are another version of them.
#[tauri::command]
pub async fn elsewhere(app: AppHandle, items: Vec<ItemRef>, visibility: Visibility) -> AppResult<Vec<Elsewhere>> {
    let libraries = libraries(&app)?;
    blocking(move || browse::elsewhere(&libraries, &items, &visibility)).await
}

/// The rigs of a library and the models and animations that bind to one: what a viewer needs to
/// match an animation to a model.
#[derive(Serialize)]
pub struct RigCatalog {
    pub rigs: Vec<Rig>,
    pub assets: Vec<Asset>,
}

/// The rigs of a library (a handful).
#[tauri::command]
pub async fn list_rigs(app: AppHandle, library: String) -> AppResult<Vec<Rig>> {
    let handle = library::resolve(&app, &library)?;
    blocking(move || Store::for_handle(&handle).all_rigs()).await
}

#[tauri::command]
pub async fn rig_catalog(app: AppHandle, library: String) -> AppResult<RigCatalog> {
    let handle = library::resolve(&app, &library)?;
    blocking(move || {
        let store = Store::for_handle(&handle);
        Ok(RigCatalog { rigs: store.all_rigs()?, assets: store.rigged_assets_all()? })
    })
    .await
}

/// Items that carry personal tags from before library tags existed (at most `limit`).
#[tauri::command]
pub async fn personal_tag_assets(app: AppHandle, library: String, limit: usize) -> AppResult<Vec<Asset>> {
    let handle = library::resolve(&app, &library)?;
    blocking(move || {
        let mut assets = Store::for_handle(&handle).assets_with_personal_tags(limit)?;
        toybox_core::ops::content_hash::fill_assets(&handle, &mut assets);
        Ok(assets)
    })
    .await
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

/// Up to `limit` items of a library, after id `after`, that have no picture yet (or a stale one).
#[tauri::command]
pub async fn pending_thumbs(app: AppHandle, request: PendingThumbsRequest) -> AppResult<PendingThumbs> {
    let handle = library::resolve(&app, &request.library)?;
    blocking(move || {
        let (ids, next) = browse::pending_thumbs(&handle, &request.after, request.limit)?;
        Ok(PendingThumbs { ids, next })
    })
    .await
}

/// Mark items as favourites (or not). Personal data: only this computer's database is written.
#[tauri::command]
pub async fn set_favorite(app: AppHandle, library: String, ids: Vec<String>, favorite: bool) -> AppResult<()> {
    let handle = library::resolve(&app, &library)?;
    blocking(move || Store::for_handle(&handle).set_favorite(&ids, favorite)).await
}

/// Put items into a collection (or take them out).
#[tauri::command]
pub async fn set_collection(app: AppHandle, library: String, ids: Vec<String>, name: String, member: bool) -> AppResult<()> {
    let handle = library::resolve(&app, &library)?;
    blocking(move || {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::msg("A collection needs a name"));
        }
        Store::for_handle(&handle).set_collection_member(&ids, &name, member)
    })
    .await
}

/// Take personal tags off items: they were moved into the library as library tags.
#[derive(Deserialize)]
pub struct PersonalTagRemoval {
    pub id: String,
    pub tags: Vec<String>,
}

#[tauri::command]
pub async fn drop_personal_tags(app: AppHandle, library: String, removals: Vec<PersonalTagRemoval>) -> AppResult<()> {
    let handle = library::resolve(&app, &library)?;
    blocking(move || {
        let pairs: Vec<(String, Vec<String>)> = removals.into_iter().map(|removal| (removal.id, removal.tags)).collect();
        Store::for_handle(&handle).drop_personal_tags(&pairs)
    })
    .await
}

/// Choose the asset an item is previewed on (`None` clears the choice).
#[tauri::command]
pub async fn set_preview_choice(app: AppHandle, library: String, asset_id: String, choice: Option<String>) -> AppResult<()> {
    let handle = library::resolve(&app, &library)?;
    blocking(move || Store::for_handle(&handle).set_preview_choice(&asset_id, choice.as_deref())).await
}
