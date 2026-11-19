//! What the window asks of its libraries (see `docs/sqlite-storage.md`, "App queries"): a page of
//! the list, counts, assets by id, and the small writes of personal data. The window never holds a
//! library's catalog. The work, including ranking the libraries as the window ranks them
//! (`libraryRanks` in `src/domain/duplicates.ts`), is `toybox_host::browse`.

use tauri::AppHandle;
use toybox_core::domain::catalog_model::Asset;
use toybox_core::domain::rig::Rig;
use toybox_core::ops::browse::{AssetPage, IdPage, ItemRef, LibrarySummary, Row};
use toybox_host::browse::{
    BrowseFilter, CollectionCount, Count, Elsewhere, FacetCounts, PendingThumbs, PendingThumbsRequest, PersonalTagRemoval, RigCatalog,
    Visibility,
};

use crate::app_host::host;
use crate::commands::remote::blocking;
use crate::error::AppResult;

/// Make a library ready to be read, and say where it stands. A local library finishes its migrations and
/// builds its catalog if it has none that is current (`force`: rebuilds it). A remote library is brought up to
/// date from its server (`force`: downloaded again); when the server cannot be reached its replica is used.
#[tauri::command]
pub async fn open_library(app: AppHandle, library: String, force: bool) -> AppResult<LibrarySummary> {
    let host = host(&app);
    blocking(move || host.open_library(&library, force)).await
}

/// A page of the list the filter shows, in name order, and how long the list is.
#[tauri::command]
pub async fn query_assets(app: AppHandle, filter: BrowseFilter, descending: bool, offset: i64, limit: i64) -> AppResult<AssetPage> {
    let host = host(&app);
    blocking(move || host.query_assets(&filter, descending, offset, limit)).await
}

/// How many items the filter shows, exactly. `query_assets` answers with a lower bound (`exact: false`) for a broad search, so
/// the first page does not wait for this pass over the list.
#[tauri::command]
pub async fn count_assets(app: AppHandle, filter: BrowseFilter) -> AppResult<i64> {
    let host = host(&app);
    blocking(move || host.count_assets(&filter)).await
}

/// The ids of the items the filter shows from `offset` on, at most `cap` (select all, a range between two clicks).
#[tauri::command]
pub async fn query_ids(app: AppHandle, filter: BrowseFilter, descending: bool, offset: i64, cap: i64) -> AppResult<IdPage> {
    let host = host(&app);
    blocking(move || host.query_ids(&filter, descending, offset, cap)).await
}

/// Items (or variants) by id, in the order asked; ids that are not there are left out.
#[tauri::command]
pub async fn get_assets(app: AppHandle, items: Vec<ItemRef>) -> AppResult<Vec<Row>> {
    let host = host(&app);
    blocking(move || host.get_assets(&items)).await
}

/// The rail's numbers for one type.
#[tauri::command]
pub async fn facet_counts(app: AppHandle, kind: String, visibility: Visibility, tag_limit: usize, selected_tags: Vec<String>) -> AppResult<FacetCounts> {
    let host = host(&app);
    blocking(move || host.facet_counts(&kind, &visibility, tag_limit, &selected_tags)).await
}

/// The tags of a type that contain `text`, most used first.
#[tauri::command]
pub async fn search_tags(app: AppHandle, kind: String, visibility: Visibility, text: String, limit: usize) -> AppResult<Vec<Count>> {
    let host = host(&app);
    blocking(move || host.search_tags(&kind, &visibility, &text, limit)).await
}

/// Items per type, each once (the sidebar).
#[tauri::command]
pub async fn type_counts(app: AppHandle, visibility: Visibility) -> AppResult<Vec<Count>> {
    let host = host(&app);
    blocking(move || host.type_counts(&visibility)).await
}

/// The collections and how many items of `kind` each holds.
#[tauri::command]
pub async fn list_collections(app: AppHandle, kind: String, visibility: Visibility) -> AppResult<Vec<CollectionCount>> {
    let host = host(&app);
    blocking(move || host.list_collections(&kind, &visibility)).await
}

/// Where else these items live, and which copies are another version of them.
#[tauri::command]
pub async fn elsewhere(app: AppHandle, items: Vec<ItemRef>, visibility: Visibility) -> AppResult<Vec<Elsewhere>> {
    let host = host(&app);
    blocking(move || host.elsewhere(&items, &visibility)).await
}

/// The rigs of a library (a handful).
#[tauri::command]
pub async fn list_rigs(app: AppHandle, library: String) -> AppResult<Vec<Rig>> {
    let host = host(&app);
    blocking(move || host.list_rigs(&library)).await
}

#[tauri::command]
pub async fn rig_catalog(app: AppHandle, library: String) -> AppResult<RigCatalog> {
    let host = host(&app);
    blocking(move || host.rig_catalog(&library)).await
}

/// Items that carry personal tags from before library tags existed (at most `limit`).
#[tauri::command]
pub async fn personal_tag_assets(app: AppHandle, library: String, limit: usize) -> AppResult<Vec<Asset>> {
    let host = host(&app);
    blocking(move || host.personal_tag_assets(&library, limit)).await
}

/// Up to `limit` items of a library, after id `after`, that have no picture yet (or a stale one).
#[tauri::command]
pub async fn pending_thumbs(app: AppHandle, request: PendingThumbsRequest) -> AppResult<PendingThumbs> {
    let host = host(&app);
    blocking(move || host.pending_thumbs(&request)).await
}

/// Mark items as favourites (or not). Personal data: only this computer's database is written.
#[tauri::command]
pub async fn set_favorite(app: AppHandle, library: String, ids: Vec<String>, favorite: bool) -> AppResult<()> {
    let host = host(&app);
    blocking(move || host.set_favorite(&library, &ids, favorite)).await
}

/// Put items into a collection (or take them out).
#[tauri::command]
pub async fn set_collection(app: AppHandle, library: String, ids: Vec<String>, name: String, member: bool) -> AppResult<()> {
    let host = host(&app);
    blocking(move || host.set_collection(&library, &ids, &name, member)).await
}

#[tauri::command]
pub async fn drop_personal_tags(app: AppHandle, library: String, removals: Vec<PersonalTagRemoval>) -> AppResult<()> {
    let host = host(&app);
    blocking(move || host.drop_personal_tags(&library, removals)).await
}

/// Choose the asset an item is previewed on (`None` clears the choice).
#[tauri::command]
pub async fn set_preview_choice(app: AppHandle, library: String, asset_id: String, choice: Option<String>) -> AppResult<()> {
    let host = host(&app);
    blocking(move || host.set_preview_choice(&library, &asset_id, choice.as_deref())).await
}
