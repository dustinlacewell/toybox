//! Choosing items: ids, a filter, or both (docs/cli.md, "Selecting items"). Every
//! command that acts on items goes through here.

use std::collections::HashSet;

use clap::{Arg, ArgAction, ArgMatches, Command};
use toybox_host::libraries::LibraryInfo;
use toybox_host::search::ItemFilter;

use crate::ctx::Ctx;
use crate::error::{CliError, CliResult};
use crate::prompt;

/// A change through a filter that selects more than this many items asks first.
const ASK_ABOVE: usize = 100;
/// Most items a filter may select.
const FILTER_CAP: i64 = 1_000_000;

/// Where a command takes the item ids from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ids {
    /// `[ID]...` after the other positional arguments.
    Positional,
    /// `--id <ID>` (for a command whose own positional arguments come first).
    Flag,
}

/// The filter options. `text_flag`: `--text` (otherwise the command has a positional `TEXT`).
pub fn add_filter_args(command: Command, text_flag: bool) -> Command {
    add_filter_args_without(command, text_flag, &[])
}

/// The filter options except the named ones (a command that uses the name for something else).
pub fn add_filter_args_without(command: Command, text_flag: bool, skip: &[&str]) -> Command {
    let mut command = command;
    if text_flag {
        command = command.arg(Arg::new("text").long("text").value_name("TEXT").help_heading("Selection").help("Part of the name or of a tag"));
    }
    let all = [
        Arg::new("type").long("type").value_name("TYPE").help("Asset type: model, material, animation, audio, skybox, ..."),
        Arg::new("tag").long("tag").value_name("TAG").action(ArgAction::Append).help("Has this tag (repeat for several: every one must be present)"),
        Arg::new("source").long("source").value_name("VALUE").action(ArgAction::Append).help("From this source (repeat: any one matches)"),
        Arg::new("publisher").long("publisher").value_name("VALUE").action(ArgAction::Append).help("From this publisher (repeat: any one matches)"),
        Arg::new("license").long("license").value_name("VALUE").action(ArgAction::Append).help("Under this license (repeat: any one matches)"),
        Arg::new("format").long("format").value_name("VALUE").action(ArgAction::Append).help("Imported from this file format (repeat: any one matches)"),
        Arg::new("category").long("category").value_name("VALUE").action(ArgAction::Append).help("In this category (repeat: any one matches)"),
        Arg::new("collection").long("collection").value_name("NAME").help("In this collection"),
        Arg::new("favorites").long("favorites").action(ArgAction::SetTrue).help("Only favorites"),
    ];
    for arg in all {
        if !skip.contains(&arg.get_id().as_str()) {
            command = command.arg(arg.help_heading("Selection"));
        }
    }
    command
}

/// The selection: ids and the filter options.
pub fn add_selection_args(command: Command, ids: Ids) -> Command {
    add_selection_args_without(command, ids, &[])
}

/// The selection without the named filter options.
pub fn add_selection_args_without(command: Command, ids: Ids, skip: &[&str]) -> Command {
    let command = match ids {
        Ids::Positional => command.arg(Arg::new("ids").value_name("ID").num_args(0..).help("Item ids as `tbx search` prints them; `-` reads ids from standard input")),
        Ids::Flag => command.arg(Arg::new("ids").long("id").value_name("ID").action(ArgAction::Append).help("Item id; `-` reads ids from standard input (repeatable)")),
    };
    add_filter_args_without(command, true, skip)
}

fn many(matches: &ArgMatches, name: &str) -> Vec<String> {
    matches.try_get_many::<String>(name).ok().flatten().map(|values| values.cloned().collect()).unwrap_or_default()
}

fn one(matches: &ArgMatches, name: &str) -> Option<String> {
    matches.try_get_one::<String>(name).ok().flatten().cloned().filter(|text| !text.is_empty())
}

/// The filter the options describe.
pub fn item_filter(matches: &ArgMatches) -> ItemFilter {
    ItemFilter {
        kind: one(matches, "type"),
        text: one(matches, "text").unwrap_or_default(),
        tags: many(matches, "tag"),
        sources: many(matches, "source"),
        publishers: many(matches, "publisher"),
        licenses: many(matches, "license"),
        formats: many(matches, "format"),
        categories: many(matches, "category"),
        collection: one(matches, "collection"),
        favorites_only: matches.try_get_one::<bool>("favorites").ok().flatten().copied().unwrap_or(false),
    }
}

/// The ids the command line names, with `-` replaced by the ids on standard input.
fn named_ids(matches: &ArgMatches) -> CliResult<Vec<String>> {
    let mut ids = Vec::new();
    for id in many(matches, "ids") {
        if id == "-" {
            for line in prompt::read_stdin()?.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    ids.push(line.to_string());
                }
            }
        } else {
            ids.push(id);
        }
    }
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(id.clone()));
    Ok(ids)
}

/// A resolved selection in one library.
pub struct Selected {
    pub library: LibraryInfo,
    pub ids: Vec<String>,
}

/// What the command line chose, checked for usage mistakes.
struct Chosen {
    ids: Vec<String>,
    filter: ItemFilter,
    filtered: bool,
}

fn parse(matches: &ArgMatches) -> CliResult<Chosen> {
    let ids = named_ids(matches)?;
    let filter = item_filter(matches);
    let filtered = filter.kind.is_some() || filter.is_narrowing();
    if ids.is_empty() {
        if !filtered {
            return Err(CliError::usage("no items chosen").hint("name item ids, or choose with --type and the filter options"));
        }
        if filter.kind.is_none() {
            return Err(CliError::usage("--type is required with a filter"));
        }
    }
    Ok(Chosen { ids, filter, filtered })
}

/// `-L <name>` for a hint: the name, quoted when it has spaces.
fn flag_for(library: &LibraryInfo) -> String {
    if library.name.contains(char::is_whitespace) { format!("\"{}\"", library.name) } else { library.name.clone() }
}

/// Ids that are not in `current` but are in another library: say which.
fn elsewhere_hint(ctx: &Ctx, current: &LibraryInfo, missing: &[String]) -> Option<String> {
    for other in ctx.host.list_libraries().into_iter().filter(|other| other.id != current.id && other.available) {
        if ctx.open(&other).is_err() {
            continue;
        }
        let Ok((found, _)) = ctx.host.split_known_ids(&other.id, missing) else { continue };
        if found.is_empty() {
            continue;
        }
        let which = flag_for(&other);
        return Some(if found.len() == missing.len() && missing.len() == 1 {
            format!("it is in library '{}'; add -L {which}", other.name)
        } else if found.len() == missing.len() {
            format!("they are in library '{}'; add -L {which}", other.name)
        } else {
            format!("{} of them are in library '{}'; add -L {which}", found.len(), other.name)
        });
    }
    None
}

fn narrow(ctx: &Ctx, library: &LibraryInfo, filter: &ItemFilter, ids: &mut Vec<String>) -> CliResult<()> {
    let scope = ctx.host.browse_scope(Some(&library.id))?;
    let visibility = ctx.host.visibility_for(false)?;
    let (matching, _) = ctx.host.matching_ids(&scope, filter, &visibility, FILTER_CAP)?;
    let keep: HashSet<String> = matching.into_iter().map(|item| item.id).collect();
    ids.retain(|id| keep.contains(id));
    Ok(())
}

/// Resolve the selection of a command that works on one library (`-L`, else the default). `change`: the
/// command changes items, so a filter that selects more than 100 asks first. `verb` completes
/// "... selects N items" in that question.
pub fn resolve(ctx: &Ctx, matches: &ArgMatches, change: bool, verb: &str) -> CliResult<Selected> {
    let chosen = parse(matches)?;
    let library = ctx.library()?;
    ctx.open(&library)?;
    let host = &ctx.host;
    let mut ids = chosen.ids.clone();
    if !chosen.ids.is_empty() {
        let (found, missing) = host.split_known_ids(&library.id, &chosen.ids)?;
        if !missing.is_empty() {
            let mut error = CliError::not_found(format!("no such item in library '{}': {}", library.name, missing.join(", ")));
            error = error.hint(elsewhere_hint(ctx, &library, &missing).unwrap_or_else(|| "ids are printed by `tbx search`".to_string()));
            return Err(error);
        }
        ids = found;
    }
    if chosen.filtered {
        if chosen.ids.is_empty() {
            let scope = host.browse_scope(Some(&library.id))?;
            let (matching, _) = host.matching_ids(&scope, &chosen.filter, &host.visibility_for(false)?, FILTER_CAP)?;
            ids = matching.into_iter().map(|item| item.id).collect();
            if change && ids.len() > ASK_ABOVE {
                prompt::confirm(ctx, &format!("The filter selects {} items; {verb} them?", ids.len()))?;
            }
        } else {
            narrow(ctx, &library, &chosen.filter, &mut ids)?;
        }
    }
    Ok(Selected { library, ids })
}

/// Resolve the selection of a command that only reads. With `-L` it is [`resolve`]. Without, the
/// ids are looked up in every library: an id found in several is returned for each, so the
/// caller can label them; a filter selects across all libraries.
pub fn resolve_across(ctx: &Ctx, matches: &ArgMatches) -> CliResult<Vec<Selected>> {
    if ctx.g.library.is_some() {
        return Ok(vec![resolve(ctx, matches, false, "read")?]);
    }
    let chosen = parse(matches)?;
    let host = &ctx.host;
    let mut libraries: Vec<LibraryInfo> = host.list_libraries().into_iter().filter(|library| library.available).collect();
    if libraries.is_empty() {
        host.target_library(None)?;
        return Err(CliError::failed("no library can be read right now").hint("see `tbx library list`"));
    }
    libraries.sort_by_key(|library| !library.is_default);
    libraries.retain(|library| match ctx.open(library) {
        Ok(()) => true,
        Err(error) => {
            ctx.warn(&error.message);
            false
        }
    });
    let mut groups: Vec<Selected> = Vec::new();
    if chosen.ids.is_empty() {
        let scope = host.browse_scope(None)?;
        let (matching, _) = host.matching_ids(&scope, &chosen.filter, &host.visibility_for(false)?, FILTER_CAP)?;
        for library in &libraries {
            let ids: Vec<String> = matching.iter().filter(|item| item.library == library.id).map(|item| item.id.clone()).collect();
            if !ids.is_empty() {
                groups.push(Selected { library: library.clone(), ids });
            }
        }
        return Ok(groups);
    }
    let mut found_anywhere: HashSet<String> = HashSet::new();
    for library in &libraries {
        let (found, _) = host.split_known_ids(&library.id, &chosen.ids)?;
        let mut ids = found;
        found_anywhere.extend(ids.iter().cloned());
        if chosen.filtered {
            narrow(ctx, library, &chosen.filter, &mut ids)?;
        }
        if !ids.is_empty() {
            groups.push(Selected { library: library.clone(), ids });
        }
    }
    let missing: Vec<&String> = chosen.ids.iter().filter(|id| !found_anywhere.contains(*id)).collect();
    if !missing.is_empty() {
        return Err(CliError::not_found(format!("no such item in any library: {}", missing.iter().map(|id| id.as_str()).collect::<Vec<_>>().join(", "))).hint("ids are printed by `tbx search`"));
    }
    Ok(groups)
}
