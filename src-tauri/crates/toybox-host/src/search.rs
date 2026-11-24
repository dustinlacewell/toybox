//! Finding items the way the app lists them, for front ends that do not hold a
//! filter state: one call per question, over one library or all of them, with or
//! without a type.
//!
//! Without a type the query is run once per type that has items and the pages are
//! merged by name (the order the app uses), so a page costs the types times the
//! offset plus limit.

use std::collections::HashSet;

use toybox_core::error::{AppError, AppResult};
use toybox_core::infra::store::browse::{BrowseFilter, Count, Visibility};
use toybox_core::infra::store::Store;
use toybox_core::ops::browse::{self, BrowseLibrary, ItemRef, Row};

use crate::host::Host;

/// What to match. Empty lists constrain nothing; within `sources`, `publishers`,
/// `licenses`, `formats` and `categories` any one value matches, every one of `tags`
/// must be present.
#[derive(Debug, Clone, Default)]
pub struct ItemFilter {
    /// The asset type; `None` searches every type.
    pub kind: Option<String>,
    /// Part of the name or of a tag.
    pub text: String,
    pub tags: Vec<String>,
    pub sources: Vec<String>,
    pub publishers: Vec<String>,
    pub licenses: Vec<String>,
    pub formats: Vec<String>,
    pub categories: Vec<String>,
    pub collection: Option<String>,
    pub favorites_only: bool,
}

impl ItemFilter {
    /// Whether anything besides the type narrows the match.
    pub fn is_narrowing(&self) -> bool {
        !self.text.is_empty()
            || !self.tags.is_empty()
            || !self.sources.is_empty()
            || !self.publishers.is_empty()
            || !self.licenses.is_empty()
            || !self.formats.is_empty()
            || !self.categories.is_empty()
            || self.collection.is_some()
            || self.favorites_only
    }

    fn browse_filter(&self, kind: &str, visibility: &Visibility) -> BrowseFilter {
        BrowseFilter {
            kind: kind.to_string(),
            libraries: Vec::new(),
            sources: self.sources.clone(),
            publishers: self.publishers.clone(),
            licenses: self.licenses.clone(),
            formats: self.formats.clone(),
            categories: self.categories.clone(),
            tags: self.tags.clone(),
            favorites_only: self.favorites_only,
            collection: self.collection.clone(),
            text: self.text.clone(),
            visibility: visibility.clone(),
            ..BrowseFilter::default()
        }
    }
}

/// A page of matches.
#[derive(Debug, Clone)]
pub struct SearchPage {
    /// How many items match in all.
    pub total: i64,
    pub offset: i64,
    pub rows: Vec<Row>,
}

impl Host {
    /// The libraries a query reads: one library by id, or (`None`) every readable one
    /// ranked as the app ranks them. A library that cannot be read is an error when named.
    pub fn browse_scope(&self, library: Option<&str>) -> AppResult<Vec<BrowseLibrary>> {
        let Some(id) = library else { return self.browse_libraries() };
        let info = self.library_info(id)?;
        if !info.available {
            return Err(AppError::msg(format!("library '{}' cannot be read right now", info.name)));
        }
        Ok(vec![BrowseLibrary { handle: self.resolve(id)?, rank: 0 }])
    }

    /// The visibility to query with: what the app hides, or nothing hidden.
    pub fn visibility_for(&self, include_hidden: bool) -> AppResult<Visibility> {
        if include_hidden { Ok(Visibility::default()) } else { self.visibility() }
    }

    /// Types that have visible items in `scope`, with their counts.
    pub fn types_in(&self, scope: &[BrowseLibrary], visibility: &Visibility) -> AppResult<Vec<Count>> {
        browse::type_counts(scope, visibility)
    }

    fn kinds_of(&self, scope: &[BrowseLibrary], filter: &ItemFilter, visibility: &Visibility) -> AppResult<Vec<String>> {
        match &filter.kind {
            Some(kind) => Ok(vec![kind.clone()]),
            None => Ok(browse::type_counts(scope, visibility)?.into_iter().filter(|count| count.count > 0).map(|count| count.value).collect()),
        }
    }

    /// A page of the matches in name order.
    pub fn search(&self, scope: &[BrowseLibrary], filter: &ItemFilter, visibility: &Visibility, descending: bool, offset: i64, limit: i64) -> AppResult<SearchPage> {
        let offset = offset.max(0);
        let limit = limit.max(0);
        let kinds = self.kinds_of(scope, filter, visibility)?;
        let mut total = 0;
        let mut rows: Vec<Row> = Vec::new();
        for kind in &kinds {
            let browse_filter = filter.browse_filter(kind, visibility);
            total += browse::count_assets(scope, &browse_filter)?;
            let page = browse::query_assets(scope, &browse_filter, descending, 0, offset + limit)?;
            rows.extend(page.rows);
        }
        if kinds.len() > 1 {
            rows.sort_by(|a, b| {
                let order = (a.asset.name.to_lowercase(), &a.asset.id).cmp(&(b.asset.name.to_lowercase(), &b.asset.id));
                if descending { order.reverse() } else { order }
            });
        }
        let rows = rows.into_iter().skip(offset as usize).take(limit as usize).collect();
        Ok(SearchPage { total, offset, rows })
    }

    /// How many items match, exactly.
    pub fn count_matches(&self, scope: &[BrowseLibrary], filter: &ItemFilter, visibility: &Visibility) -> AppResult<i64> {
        let mut total = 0;
        for kind in self.kinds_of(scope, filter, visibility)? {
            total += browse::count_assets(scope, &filter.browse_filter(&kind, visibility))?;
        }
        Ok(total)
    }

    /// The ids of the matches (at most `cap`), and whether there were more.
    pub fn matching_ids(&self, scope: &[BrowseLibrary], filter: &ItemFilter, visibility: &Visibility, cap: i64) -> AppResult<(Vec<ItemRef>, bool)> {
        let mut out = Vec::new();
        let mut truncated = false;
        for kind in self.kinds_of(scope, filter, visibility)? {
            let room = cap - out.len() as i64;
            if room <= 0 {
                truncated = true;
                break;
            }
            let page = browse::query_ids(scope, &filter.browse_filter(&kind, visibility), false, 0, room)?;
            truncated |= page.truncated;
            out.extend(page.ids);
        }
        Ok((out, truncated))
    }

    /// Which of `ids` are items or variants of the library: `(found, missing)`, in the order given.
    pub fn split_known_ids(&self, library: &str, ids: &[String]) -> AppResult<(Vec<String>, Vec<String>)> {
        let handle = self.resolve(library)?;
        let known: HashSet<String> = Store::for_handle(&handle).load_assets(ids)?.into_iter().map(|asset| asset.id).collect();
        let mut seen = HashSet::new();
        let (mut found, mut missing) = (Vec::new(), Vec::new());
        for id in ids {
            if !seen.insert(id.clone()) {
                continue;
            }
            if known.contains(id) { found.push(id.clone()) } else { missing.push(id.clone()) }
        }
        Ok((found, missing))
    }
}

impl Host {
    /// The rail's numbers for one type, over `scope`.
    pub fn facets_in(&self, scope: &[BrowseLibrary], kind: &str, visibility: &Visibility, tag_limit: usize) -> AppResult<toybox_core::infra::store::browse::FacetCounts> {
        browse::facet_counts(scope, kind, visibility, tag_limit, &[])
    }

    /// Tags in use (most used first) over `scope`: of one type, or (without) summed over every type.
    pub fn tags_in(&self, scope: &[BrowseLibrary], visibility: &Visibility, kind: Option<&str>, text: &str, limit: usize) -> AppResult<Vec<Count>> {
        let kinds = match kind {
            Some(kind) => vec![kind.to_string()],
            None => browse::type_counts(scope, visibility)?.into_iter().filter(|count| count.count > 0).map(|count| count.value).collect(),
        };
        let mut merged: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for kind in kinds {
            for count in browse::search_tags(scope, &kind, visibility, text, limit)? {
                *merged.entry(count.value).or_default() += count.count;
            }
        }
        let mut out: Vec<Count> = merged.into_iter().map(|(value, count)| Count { value, count }).collect();
        out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
        out.truncate(limit);
        Ok(out)
    }

    /// Collections and their sizes over `scope`: of one type, or (without) summed over every type.
    pub fn collections_in(&self, scope: &[BrowseLibrary], visibility: &Visibility, kind: Option<&str>) -> AppResult<Vec<toybox_core::infra::store::browse::CollectionCount>> {
        use toybox_core::infra::store::browse::CollectionCount;
        let kinds = match kind {
            Some(kind) => vec![kind.to_string()],
            None => browse::type_counts(scope, visibility)?.into_iter().filter(|count| count.count > 0).map(|count| count.value).collect(),
        };
        let mut merged: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
        for kind in kinds {
            for count in browse::collections(scope, &kind, visibility)? {
                *merged.entry(count.name).or_default() += count.count;
            }
        }
        Ok(merged.into_iter().map(|(name, count)| CollectionCount { name, count }).collect())
    }

    /// The pending-thumbnail count of a library (items with no picture yet or a stale one).
    pub fn pending_thumb_count(&self, library: &str) -> AppResult<usize> {
        let handle = self.resolve(library)?;
        let mut after = String::new();
        let mut total = 0;
        loop {
            let (ids, next) = browse::pending_thumbs(&handle, &after, 1000)?;
            total += ids.len();
            match next {
                Some(next) => after = next,
                None => return Ok(total),
            }
        }
    }
}
