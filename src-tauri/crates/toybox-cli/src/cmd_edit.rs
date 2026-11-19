//! Changing items: `tag`, `edit`, `favorite`, `collection`.

use clap::ArgMatches;
use toybox_host::edit::{parse_tag_list, DetailEdit, EditPlan, TagChange};
use toybox_host::libraries::LibraryInfo;

use crate::ctx::Ctx;
use crate::error::{CliError, CliResult};
use crate::json::*;
use crate::out::{plural, Table};
use crate::selection;

fn flag(matches: &ArgMatches, name: &str) -> bool {
    matches.try_get_one::<bool>(name).ok().flatten().copied().unwrap_or(false)
}

fn text_arg(matches: &ArgMatches, name: &str) -> Option<String> {
    matches.try_get_one::<String>(name).ok().flatten().cloned()
}

/// The ids and names `ids` stand for, as `ID  NAME` lines.
fn id_lines(ctx: &Ctx, library: &LibraryInfo, ids: &[String]) -> String {
    let names: std::collections::HashMap<String, String> = ctx
        .host
        .resolve(&library.id)
        .ok()
        .and_then(|handle| toybox_core::infra::store::Store::for_handle(&handle).load_assets(ids).ok())
        .map(|assets| assets.into_iter().map(|asset| (asset.id, asset.name)).collect())
        .unwrap_or_default();
    let mut table = Table::new(&["ID", "NAME"]);
    for id in ids {
        table.row(vec![id.clone(), names.get(id).cloned().unwrap_or_default()]);
    }
    table.render()
}

/// Print the outcome of a change (or of a dry run) and return.
fn finish(ctx: &Ctx, library: &LibraryInfo, dry_run: bool, verb: &str, plan: &EditPlan) -> CliResult<()> {
    let doc = ChangeDoc { library: library.id.clone(), dry_run, changed: plan.changed.clone(), unchanged: plan.unchanged.clone() };
    let mut text = String::new();
    if dry_run {
        text.push_str(&format!("Would {verb} {} in '{}':\n", plural(plan.changed.len(), "item", "items"), library.name));
        text.push_str(&id_lines(ctx, library, &plan.changed));
        if !plan.unchanged.is_empty() {
            text.push_str(&format!("({} already as asked)\n", plural(plan.unchanged.len(), "item", "items")));
        }
    } else {
        text.push_str(&format!("{} {}", past(verb), plural(plan.changed.len(), "item", "items")));
        if !plan.unchanged.is_empty() {
            text.push_str(&format!(" ({} already as asked)", plan.unchanged.len()));
        }
        text.push('\n');
    }
    ctx.report(&doc, &text)
}

/// `tag` -> `Tagged`, `add to 'X'` -> `Added to 'X'`.
fn past(verb: &str) -> String {
    let (first, rest) = verb.split_once(' ').map(|(first, rest)| (first, format!(" {rest}"))).unwrap_or((verb, String::new()));
    let word = match first {
        "tag" => "Tagged",
        "untag" => "Untagged",
        "retag" => "Retagged",
        "edit" => "Edited",
        "favorite" => "Marked as favorite:",
        "unfavorite" => "Unmarked as favorite:",
        "add" => "Added",
        "remove" => "Removed",
        other => other,
    };
    format!("{word}{rest}")
}

pub fn tag(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let (action, sub) = matches.subcommand().ok_or_else(|| CliError::usage("choose list, add, remove or set"))?;
    if action == "list" {
        let scope = crate::cmd_items::read_scope(ctx)?;
        let tags = ctx.host.tags_in(&scope, &ctx.host.visibility_for(false)?, text_arg(sub, "type").as_deref(), &text_arg(sub, "search").unwrap_or_default(), sub.get_one::<usize>("limit").copied().unwrap_or(50))?;
        let mut table = Table::new(&["TAG", "ITEMS"]);
        for count in &tags {
            table.row(vec![count.value.clone(), count.count.to_string()]);
        }
        return ctx.output(&TagsDoc { tags }, &table.render());
    }
    let (change, verb) = match action {
        "add" => (TagChange::Add, "tag"),
        "remove" => (TagChange::Remove, "untag"),
        _ => (TagChange::Set, "retag"),
    };
    let tags = parse_tag_list(&text_arg(sub, "tags").unwrap_or_default());
    if tags.is_empty() && change != TagChange::Set {
        return Err(CliError::usage("no tags given").hint("give comma-separated tags, e.g. wood,prop"));
    }
    let selected = selection::resolve(ctx, sub, true, verb)?;
    let dry_run = flag(sub, "dry-run");
    let plan = ctx.host.plan_tag_change(&selected.library.id, &selected.ids, change, &tags)?;
    if !dry_run {
        ctx.open_for_write(&selected.library)?;
        ctx.host.apply_edits(&selected.library.id, &plan.edits)?;
    }
    finish(ctx, &selected.library, dry_run, verb, &plan)
}

pub fn edit(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let details = DetailEdit {
        source: text_arg(matches, "set-source"),
        publisher: text_arg(matches, "set-publisher"),
        license: text_arg(matches, "set-license"),
        url: text_arg(matches, "set-url"),
    };
    if details.source.is_none() && details.publisher.is_none() && details.license.is_none() && details.url.is_none() {
        return Err(CliError::usage("nothing to change").hint("give --source, --publisher, --license or --url (an empty value clears the field)"));
    }
    let selected = selection::resolve(ctx, matches, true, "edit")?;
    let dry_run = flag(matches, "dry-run");
    let plan = ctx.host.plan_detail_change(&selected.ids, &details);
    if !dry_run {
        ctx.open_for_write(&selected.library)?;
        ctx.host.apply_edits(&selected.library.id, &plan.edits)?;
    }
    finish(ctx, &selected.library, dry_run, "edit", &plan)
}

pub fn favorite(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let (action, sub) = matches.subcommand().ok_or_else(|| CliError::usage("choose add or remove"))?;
    let on = action == "add";
    let selected = selection::resolve(ctx, sub, true, "change")?;
    let dry_run = flag(sub, "dry-run");
    if !dry_run {
        ctx.host.set_favorite(&selected.library.id, &selected.ids, on)?;
    }
    let plan = EditPlan { changed: selected.ids.clone(), ..EditPlan::default() };
    finish(ctx, &selected.library, dry_run, if on { "favorite" } else { "unfavorite" }, &plan)
}

pub fn collection(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let (action, sub) = matches.subcommand().ok_or_else(|| CliError::usage("choose list, add or remove"))?;
    if action == "list" {
        let scope = crate::cmd_items::read_scope(ctx)?;
        let collections = ctx.host.collections_in(&scope, &ctx.host.visibility_for(false)?, text_arg(sub, "type").as_deref())?;
        let mut table = Table::new(&["COLLECTION", "ITEMS"]);
        for collection in &collections {
            table.row(vec![collection.name.clone(), collection.count.to_string()]);
        }
        return ctx.output(&CollectionsDoc { collections }, &table.render());
    }
    let on = action == "add";
    let name = text_arg(sub, "name").unwrap_or_default();
    if name.trim().is_empty() {
        return Err(CliError::usage("a collection needs a name"));
    }
    let selected = selection::resolve(ctx, sub, true, "change")?;
    let dry_run = flag(sub, "dry-run");
    if !dry_run {
        ctx.host.set_collection(&selected.library.id, &selected.ids, &name, on)?;
    }
    let plan = EditPlan { changed: selected.ids.clone(), ..EditPlan::default() };
    let verb = if on { format!("add to '{}'", name.trim()) } else { format!("remove from '{}'", name.trim()) };
    finish(ctx, &selected.library, dry_run, &verb, &plan)
}
