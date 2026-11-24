//! Finding and reading: `search`, `count`, `show`, `path`, `types`, `facets`.

use std::collections::HashMap;

use clap::ArgMatches;
use toybox_core::domain::catalog_model::Asset;
use toybox_core::domain::paths::abs_under_root;
use toybox_core::infra::store::browse::Visibility;
use toybox_core::infra::store::Store;
use toybox_host::libraries::LibraryInfo;

use crate::ctx::Ctx;
use crate::error::{CliError, CliResult};
use crate::json::*;
use crate::out::{self, human_bytes, truncate, Table};
use crate::selection::{self, item_filter};

/// The libraries a read looks at: every readable one (each made ready to be read), or the one `-L`
/// names.
pub fn read_scope(ctx: &Ctx) -> CliResult<Vec<toybox_core::ops::browse::BrowseLibrary>> {
    Ok(scope_of(ctx)?.0)
}

/// [`read_scope`] and every registered library.
fn scope_of(ctx: &Ctx) -> CliResult<(Vec<toybox_core::ops::browse::BrowseLibrary>, Vec<LibraryInfo>)> {
    let libraries: Vec<LibraryInfo> = ctx.host.list_libraries();
    if ctx.g.library.is_some() {
        let library = ctx.library()?;
        ctx.open(&library)?;
        return Ok((ctx.host.browse_scope(Some(&library.id))?, libraries));
    }
    if libraries.is_empty() {
        ctx.host.target_library(None)?;
    }
    if !libraries.iter().any(|library| library.available) {
        return Err(CliError::failed("no registered library can be read right now").hint("see `tbx library list`"));
    }
    for library in &libraries {
        if !library.available {
            ctx.warn(&format!("library '{}' cannot be read and is left out", library.name));
        } else if let Err(error) = ctx.open(library) {
            ctx.warn(&error.message);
        }
    }
    Ok((ctx.host.browse_scope(None)?, libraries))
}

fn flag(matches: &ArgMatches, name: &str) -> bool {
    matches.try_get_one::<bool>(name).ok().flatten().copied().unwrap_or(false)
}

fn source_of(asset: &Asset) -> String {
    asset.user.provenance.as_ref().and_then(|provenance| provenance.source.clone()).unwrap_or_default()
}

fn visibility(ctx: &Ctx, matches: &ArgMatches) -> CliResult<Visibility> {
    Ok(ctx.host.visibility_for(flag(matches, "include-hidden"))?)
}

pub fn search(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let (scope, libraries) = scope_of(ctx)?;
    let show_library = libraries.len() > 1;
    let mut filter = item_filter(matches);
    filter.text = matches.get_one::<String>("text").cloned().unwrap_or_default();
    let visibility = visibility(ctx, matches)?;
    let descending = flag(matches, "desc");
    let offset = matches.get_one::<i64>("offset").copied().unwrap_or(0);
    let limit = matches.get_one::<i64>("limit").copied().unwrap_or(50);
    let page = ctx.host.search(&scope, &filter, &visibility, descending, offset, limit)?;
    let names: HashMap<&str, &str> = libraries.iter().map(|library| (library.id.as_str(), library.name.as_str())).collect();
    let mut text = String::new();
    if flag(matches, "ids") {
        for row in &page.rows {
            text.push_str(&format!("{}\n", row.asset.id));
        }
    } else {
        let mut headers = vec!["ID", "TYPE", "NAME", "CATEGORY", "SOURCE", "TAGS"];
        if show_library {
            headers.insert(0, "LIBRARY");
        }
        let mut table = Table::new(&headers);
        for row in &page.rows {
            let asset = &row.asset;
            let mut cells = vec![asset.id.clone(), asset.content.type_id().to_string(), asset.name.clone(), asset.category.clone(), source_of(asset), truncate(&asset.tags.join(","), 40)];
            if show_library {
                cells.insert(0, names.get(row.library.as_str()).copied().unwrap_or(&row.library).to_string());
            }
            table.row(cells);
        }
        text = table.render();
    }
    let shown = page.rows.len() as i64;
    if page.rows.is_empty() && !ctx.g.json && !ctx.g.quiet {
        out::stderr("No items match.\n");
    }
    if !ctx.g.json && !ctx.g.quiet && page.total > page.offset + shown {
        out::stderr(&format!("showing {} of {} (use --offset and --limit for more)\n", shown, page.total));
    }
    let doc = SearchDoc { total: page.total, exact: true, offset: page.offset, items: page.rows };
    ctx.output(&doc, &text)
}

pub fn count(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let (scope, _) = scope_of(ctx)?;
    let mut filter = item_filter(matches);
    filter.text = matches.get_one::<String>("text").cloned().unwrap_or_default();
    let count = ctx.host.count_matches(&scope, &filter, &visibility(ctx, matches)?)?;
    ctx.output(&CountDoc { count }, &format!("{count}\n"))
}

/// Assets by id, in the order asked, for each library the selection reaches.
fn selected_assets(ctx: &Ctx, matches: &ArgMatches) -> CliResult<Vec<(LibraryInfo, Vec<Asset>)>> {
    let mut out = Vec::new();
    for selected in selection::resolve_across(ctx, matches)? {
        let handle = ctx.host.resolve(&selected.library.id)?;
        let mut assets = Store::for_handle(&handle).load_assets(&selected.ids)?;
        toybox_core::ops::content_hash::fill_assets(&handle, &mut assets);
        out.push((selected.library, assets));
    }
    Ok(out)
}

fn files_of(ctx: &Ctx, library: &LibraryInfo, asset: &Asset) -> CliResult<Vec<FileDoc>> {
    let handle = ctx.host.resolve(&library.id)?;
    let mut paths = Vec::new();
    asset.content_files(&mut paths);
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    Ok(paths.into_iter().map(|path| FileDoc { size: std::fs::metadata(abs_under_root(&handle.root_str(), &path)).ok().map(|meta| meta.len()), path }).collect())
}

/// The source files kept with an item (not its variants).
fn sources_of(ctx: &Ctx, library: &LibraryInfo, asset: &Asset) -> CliResult<Vec<FileDoc>> {
    let handle = ctx.host.resolve(&library.id)?;
    Ok(asset
        .origin
        .iter()
        .flat_map(|origin| origin.files.iter())
        .map(|path| FileDoc { size: std::fs::metadata(abs_under_root(&handle.root_str(), path)).ok().map(|meta| meta.len()), path: path.clone() })
        .collect())
}

/// What an item was made from (`origin.derivedFrom`) and what was made from it, as lines of `show`.
fn links_of(ctx: &Ctx, library: &LibraryInfo, asset: &Asset) -> CliResult<Vec<(&'static str, String)>> {
    let store = Store::for_handle(&ctx.host.resolve(&library.id)?);
    let mut out = Vec::new();
    if let Some(parent) = asset.origin.as_ref().and_then(|origin| origin.derived_from.clone()) {
        let found = store.load_assets(std::slice::from_ref(&parent))?.into_iter().next();
        out.push(("made from", match found {
            Some(parent_asset) => format!("{} ({parent}, {})", parent_asset.name, parent_asset.content.type_id()),
            None => format!("{parent} (not in this library)"),
        }));
    }
    let derived = store.derived_from(&asset.id)?;
    if !derived.is_empty() {
        out.push(("used by", out::plural(derived.len(), "asset made from it", "assets made from it")));
    }
    Ok(out)
}

pub fn show(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let mut items = Vec::new();
    let mut text = String::new();
    for (library, assets) in selected_assets(ctx, matches)? {
        for asset in assets {
            let files = files_of(ctx, &library, &asset)?;
            let sources = sources_of(ctx, &library, &asset)?;
            let links = links_of(ctx, &library, &asset)?;
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&describe(&library, &asset, &files, &sources, &links, ""));
            items.push(ShowItem { library: library.id.clone(), asset, files, sources });
        }
    }
    ctx.output(&ShowDoc { items }, &text)
}

/// One item as text; a variant is shown indented under its parent.
fn describe(library: &LibraryInfo, asset: &Asset, files: &[FileDoc], sources: &[FileDoc], links: &[(&str, String)], indent: &str) -> String {
    let mut text = String::new();
    let mut field = |name: &str, value: String| {
        if !value.is_empty() {
            text.push_str(&format!("{indent}{name:<13}{value}\n"));
        }
    };
    field("id", asset.id.clone());
    field("name", asset.name.clone());
    field("type", asset.content.type_id().to_string());
    field("library", library.name.clone());
    field("pack", asset.pack.clone());
    field("category", asset.category.clone());
    field("format", asset.format.clone().unwrap_or_default());
    field("tags", asset.tags.join(", "));
    let personal: Vec<&String> = asset.user.tags.iter().filter(|tag| !toybox_core::domain::tags::is_provenance_tag(tag)).collect();
    field("personal tags", personal.iter().map(|tag| tag.as_str()).collect::<Vec<_>>().join(", "));
    if let Some(provenance) = &asset.user.provenance {
        field("source", provenance.source.clone().unwrap_or_default());
        field("publisher", provenance.publisher.clone().unwrap_or_default());
        field("license", provenance.license.clone().unwrap_or_default());
        field("url", provenance.url.clone().unwrap_or_default());
    }
    if asset.user.favorite {
        field("favorite", "yes".into());
    }
    field("collections", asset.user.collections.join(", "));
    if let Some(origin) = &asset.origin {
        field("imported by", origin.plugin.clone());
    }
    for (name, value) in links {
        field(name, value.clone());
    }
    field("hash", asset.content_hash.clone().unwrap_or_default());
    if !files.is_empty() {
        text.push_str(&format!("{indent}files\n"));
        for file in files {
            let size = file.size.map(human_bytes).unwrap_or_else(|| "not on this computer".into());
            text.push_str(&format!("{indent}  {}  ({size})\n", file.path));
        }
    }
    if !sources.is_empty() {
        text.push_str(&format!("{indent}sources (kept from the import; other items may keep the same file)\n"));
        for file in sources {
            let size = file.size.map(human_bytes).unwrap_or_else(|| "not on this computer".into());
            text.push_str(&format!("{indent}  {}  ({size})\n", file.path));
        }
    }
    if !asset.variants.is_empty() {
        text.push_str(&format!("{indent}variants\n"));
        for variant in &asset.variants {
            let variant_sources: Vec<FileDoc> = variant.origin.iter().flat_map(|origin| origin.files.iter()).map(|path| FileDoc { path: path.clone(), size: None }).collect();
            let variant_links: Vec<(&str, String)> = variant.origin.as_ref().and_then(|origin| origin.derived_from.clone()).map(|parent| ("made from", parent)).into_iter().collect();
            text.push_str(&describe(library, variant, &[], &variant_sources, &variant_links, &format!("{indent}  ")));
        }
    }
    text
}

pub fn path(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let all = flag(matches, "all");
    let mut items = Vec::new();
    let mut text = String::new();
    for (library, assets) in selected_assets(ctx, matches)? {
        // A remote library keeps what was asked for on this computer first.
        let ids: Vec<String> = assets.iter().map(|asset| asset.id.clone()).collect();
        ctx.host.ensure_asset_files(&library.id, &ids, false)?;
        let handle = ctx.host.resolve(&library.id)?;
        for asset in &assets {
            let mut relative = Vec::new();
            asset.content_files(&mut relative);
            let absolute: Vec<String> = relative.iter().map(|rel| abs_under_root(&handle.root_str(), rel).to_string_lossy().to_string()).collect();
            let Some(main) = absolute.first().cloned() else {
                return Err(CliError::failed(format!("item {} has no files", asset.id)));
            };
            if all {
                for path in &absolute {
                    text.push_str(&format!("{path}\n"));
                }
            } else {
                text.push_str(&format!("{main}\n"));
            }
            items.push(ItemPaths { library: library.id.clone(), id: asset.id.clone(), path: main, files: all.then_some(absolute) });
        }
    }
    ctx.output(&PathsListDoc { items }, &text)
}

pub fn types(ctx: &Ctx, _matches: &ArgMatches) -> CliResult<()> {
    let (scope, _) = scope_of(ctx)?;
    let types = ctx.host.asset_types(&scope)?;
    let mut table = Table::new(&["TYPE", "LABEL", "PLUGIN", "ITEMS", "STATE"]);
    let mut docs = Vec::new();
    for info in types {
        table.row(vec![info.id.clone(), info.label.clone(), info.plugin.clone().unwrap_or_else(|| "-".into()), info.count.to_string(), if info.hidden { "off".into() } else { String::new() }]);
        docs.push(TypeDoc { id: info.id, label: info.label, plugin: info.plugin, native: info.native, count: info.count, hidden: info.hidden });
    }
    ctx.output(&TypesDoc { types: docs }, &table.render())
}

pub fn facets(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let (scope, _) = scope_of(ctx)?;
    let kind = matches.get_one::<String>("type").cloned().unwrap_or_default();
    let limit = matches.get_one::<usize>("tags").copied().unwrap_or(20);
    let facets = ctx.host.facets_in(&scope, &kind, &ctx.host.visibility_for(false)?, limit)?;
    let mut text = format!("total  {}\n", facets.total);
    let groups = [("categories", &facets.categories), ("sources", &facets.sources), ("publishers", &facets.publishers), ("licenses", &facets.licenses), ("formats", &facets.formats), ("tags", &facets.tags)];
    for (name, counts) in groups {
        if counts.is_empty() {
            continue;
        }
        text.push_str(&format!("\n{name}\n"));
        for count in counts {
            text.push_str(&format!("  {:<28}{}\n", truncate(&count.value, 27), count.count));
        }
    }
    ctx.output(&FacetsDoc { kind, facets }, &text)
}
