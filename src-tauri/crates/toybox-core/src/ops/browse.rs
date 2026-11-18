//! What the window asks of its libraries: a page of the list, counts, assets by id.
//! The queries are in `infra::store::browse`; this module turns their answers
//! into assets (personal data, content hashes and thumbnail states filled in) and
//! knows how a library is made ready to be read.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::domain::catalog_model::{Asset, ThumbState};
use crate::error::AppResult;
use crate::infra::registry::LibraryHandle;
use crate::infra::store::browse::{BrowseFilter, Count, CollectionCount, Elsewhere, FacetCounts, Session, Source, Visibility};
use crate::infra::store::Store;

/// A library to read and where it ranks (lower wins when the same item is in two libraries).
#[derive(Clone)]
pub struct BrowseLibrary {
    pub handle: LibraryHandle,
    pub rank: i64,
}

/// An item (or variant) named by its library and its id inside it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ItemRef {
    pub library: String,
    pub id: String,
}

/// One row of the list: the asset with raw ids; the window qualifies them.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub library: String,
    pub asset: Asset,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPage {
    /// Items the filter shows in all (not only in this page) when `exact`; otherwise what is known
    /// (the rows up to the end of this page, plus one when the page was full): the exact number takes a pass
    /// over the list, so it is asked for separately (`count_assets`).
    pub total: i64,
    pub exact: bool,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdPage {
    pub ids: Vec<ItemRef>,
    /// More items matched than `cap` allowed.
    pub truncated: bool,
}

/// Where a library stands after it was made ready.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySummary {
    pub library: String,
    pub count: i64,
    pub revision: i64,
}

fn sources(libraries: &[BrowseLibrary]) -> Vec<Source> {
    libraries
        .iter()
        .map(|library| Source { id: library.handle.id.clone(), library: library.handle.library_db_path(), user: library.handle.user_db_path(), rank: library.rank })
        .collect()
}

/// Where the browse index of these libraries lives: `<app-data>/browse-index.db` (a library's per-machine folder is
/// `<app-data>/libraries/<id>`). `None` when the handles do not sit in such a folder.
pub fn index_path(libraries: &[BrowseLibrary]) -> Option<std::path::PathBuf> {
    let dir = libraries.first()?.handle.data_dir();
    let app_data = dir.parent()?.parent()?;
    Some(app_data.join(crate::infra::store::browse_index::INDEX_FILE))
}

fn open_session(libraries: &[BrowseLibrary], only: &[String]) -> AppResult<Session> {
    Session::open_with(&sources(libraries), only, index_path(libraries).as_deref())
}

/// Bring the browse index up to date now (a build runs to its end here): for the benchmark, and for a caller that
/// wants the fast paths from the first query.
pub fn warm_index(libraries: &[BrowseLibrary]) -> AppResult<()> {
    if let Some(path) = index_path(libraries) {
        crate::infra::store::browse_index::ensure(&path, &sources(libraries), true)?;
    }
    Ok(())
}

fn handle_of<'a>(libraries: &'a [BrowseLibrary], id: &str) -> Option<&'a LibraryHandle> {
    libraries.iter().find(|library| library.handle.id == id).map(|library| &library.handle)
}

/// What opening a local library does before it is read: finish migrations, build the
/// catalog if there is none that is current, and (once, for a library from before
/// library tags) move personal tags. A remote library is brought up to date by its
/// connection instead (`toybox_remote::RemoteLibrary::refresh`). Nothing is loaded.
pub fn prepare(handle: &LibraryHandle) -> AppResult<LibrarySummary> {
    super::catalog::open_library(handle)?;
    if !handle.is_remote() {
        super::catalog::ready_for_write(handle)?;
        if Store::for_handle(handle).meta(super::metadata::PERSONAL_TAGS_DONE).is_none() {
            // A library from before library tags: the one-time move reads its catalog once.
            super::metadata::load_catalog_migrating(handle)?;
        }
    }
    super::catalog::refresh_animation_preview_thumbs(handle)?;
    summary(handle)
}

/// How many items a library has and its revision.
pub fn summary(handle: &LibraryHandle) -> AppResult<LibrarySummary> {
    let store = Store::for_handle(handle);
    Ok(LibrarySummary { library: handle.id.clone(), count: store.asset_count()?, revision: store.status()?.revision })
}

/// Turn stored assets into what the window shows: content hashes from the library, the
/// thumbnail state from the files on disk (an error that was recorded stays).
fn finish(handle: &LibraryHandle, mut assets: Vec<Asset>) -> Vec<Asset> {
    super::content_hash::fill_assets(handle, &mut assets);
    for asset in &mut assets {
        let exists = handle.has_thumb(&asset.id);
        asset.thumb.state = match (exists, &asset.thumb.state) {
            (true, _) => ThumbState::Ready,
            (false, ThumbState::Error) => ThumbState::Error,
            (false, _) => ThumbState::Missing,
        };
    }
    assets
}

/// The assets with these ids (items or variants), in the order asked; ids that are not there are left out.
pub fn get_assets(libraries: &[BrowseLibrary], items: &[ItemRef]) -> AppResult<Vec<Row>> {
    let mut by_library: Vec<(String, Vec<String>)> = Vec::new();
    for item in items {
        match by_library.iter_mut().find(|(library, _)| *library == item.library) {
            Some((_, ids)) => ids.push(item.id.clone()),
            None => by_library.push((item.library.clone(), vec![item.id.clone()])),
        }
    }
    let mut found: HashMap<(String, String), Asset> = HashMap::new();
    for (library, ids) in by_library {
        let Some(handle) = handle_of(libraries, &library) else { continue };
        let assets = finish(handle, Store::for_handle(handle).load_assets(&ids)?);
        for asset in assets {
            found.insert((library.clone(), asset.id.clone()), asset);
        }
    }
    Ok(items.iter().filter_map(|item| found.remove(&(item.library.clone(), item.id.clone())).map(|asset| Row { library: item.library.clone(), asset })).collect())
}

/// A page of the list and how long the list is.
pub fn query_assets(libraries: &[BrowseLibrary], filter: &BrowseFilter, descending: bool, offset: i64, limit: i64) -> AppResult<AssetPage> {
    let session = open_session(libraries, &filter.libraries)?;
    let ids = session.page(filter, descending, offset, limit)?;
    let (total, exact) = match session.count_cheap(filter)? {
        Some(total) => (total, true),
        None => (offset.max(0) + ids.len() as i64 + i64::from(ids.len() as i64 >= limit), false),
    };
    drop(session);
    let items: Vec<ItemRef> = ids.into_iter().map(|(library, id)| ItemRef { library, id }).collect();
    Ok(AssetPage { total, exact, rows: get_assets(libraries, &items)? })
}

/// How many items the filter shows: the exact answer, which for a broad search is a pass over the list.
pub fn count_assets(libraries: &[BrowseLibrary], filter: &BrowseFilter) -> AppResult<i64> {
    open_session(libraries, &filter.libraries)?.count(filter)
}

/// The ids of the items the filter shows from `offset` on (select all, or the rows between two clicks), at most `cap`.
pub fn query_ids(libraries: &[BrowseLibrary], filter: &BrowseFilter, descending: bool, offset: i64, cap: i64) -> AppResult<IdPage> {
    let session = open_session(libraries, &filter.libraries)?;
    let mut ids = session.page(filter, descending, offset, cap + 1)?;
    let truncated = ids.len() as i64 > cap;
    ids.truncate(cap.max(0) as usize);
    Ok(IdPage { ids: ids.into_iter().map(|(library, id)| ItemRef { library, id }).collect(), truncated })
}

pub fn facet_counts(libraries: &[BrowseLibrary], kind: &str, visibility: &Visibility, tag_limit: usize, selected_tags: &[String]) -> AppResult<FacetCounts> {
    open_session(libraries, &[])?.facet_counts(kind, visibility, tag_limit, selected_tags)
}

pub fn search_tags(libraries: &[BrowseLibrary], kind: &str, visibility: &Visibility, text: &str, limit: usize) -> AppResult<Vec<Count>> {
    open_session(libraries, &[])?.search_tags(kind, visibility, text, limit)
}

pub fn type_counts(libraries: &[BrowseLibrary], visibility: &Visibility) -> AppResult<Vec<Count>> {
    open_session(libraries, &[])?.type_counts(visibility)
}

pub fn collections(libraries: &[BrowseLibrary], kind: &str, visibility: &Visibility) -> AppResult<Vec<CollectionCount>> {
    open_session(libraries, &[])?.collections(kind, visibility)
}

pub fn elsewhere(libraries: &[BrowseLibrary], items: &[ItemRef], visibility: &Visibility) -> AppResult<Vec<Elsewhere>> {
    let pairs: Vec<(String, String)> = items.iter().map(|item| (item.library.clone(), item.id.clone())).collect();
    open_session(libraries, &[])?.elsewhere(&pairs, visibility)
}

/// The items of the type being browsed that have no picture yet, a page at a time: up to `limit`
/// ids after `after` (in id order) and where to continue (`None`: that was the end).
pub fn pending_thumbs(handle: &LibraryHandle, after: &str, limit: usize) -> AppResult<(Vec<String>, Option<String>)> {
    let store = Store::for_handle(handle);
    let mut pending = Vec::new();
    let mut cursor = after.to_string();
    loop {
        let (ids, next) = store.asset_id_page(&cursor, 1000)?;
        for id in &ids {
            let path = handle.thumb_path(id);
            if !handle.has_thumb(id) || crate::infra::fsio::exists(&path.with_extension("stale")) {
                pending.push(id.clone());
            }
        }
        match next {
            Some(next) if pending.len() < limit => cursor = next,
            Some(next) => return Ok((pending, Some(next))),
            None => return Ok((pending, None)),
        }
    }
}
