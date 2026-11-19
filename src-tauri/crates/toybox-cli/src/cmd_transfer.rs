//! Moving items about: `copy`, `move`, `delete`, `thumbs`.

use clap::ArgMatches;
use toybox_core::ops::transfer::{OnConflict, Outcome, TransferMode, TransferReport};

use crate::ctx::Ctx;
use crate::error::{CliError, CliResult};
use crate::json::*;
use crate::out::{self, plural, Table};
use crate::prompt;
use crate::selection;

fn flag(matches: &ArgMatches, name: &str) -> bool {
    matches.try_get_one::<bool>(name).ok().flatten().copied().unwrap_or(false)
}

pub fn transfer(ctx: &Ctx, matches: &ArgMatches, moving: bool) -> CliResult<()> {
    let mode = if moving { TransferMode::Move } else { TransferMode::Copy };
    let verb = if moving { "move" } else { "copy" };
    let source = ctx.library()?;
    let target_name = matches.get_one::<String>("to").cloned().unwrap_or_default();
    let target = ctx.host.find_library(&target_name)?;
    if source.id == target.id {
        return Err(CliError::usage(format!("'{}' is both the source and the target", source.name)).hint("name the other library with --to, and the source with -L"));
    }
    let selected = selection::resolve(ctx, matches, true, verb)?;
    ctx.open(&target)?;
    let on_conflict = if matches.get_one::<String>("on-conflict").map(String::as_str) == Some("replace") { OnConflict::Replace } else { OnConflict::Skip };
    let include_clips = flag(matches, "with-clips");
    let dry_run = flag(matches, "dry-run");
    let mut doc = TransferDoc {
        from: source.id.clone(),
        to: target.id.clone(),
        mode: verb.to_string(),
        dry_run,
        copied: 0,
        replaced: 0,
        skipped: 0,
        failed: 0,
        removed: 0,
        report: TransferReport::default(),
        would_transfer: Vec::new(),
    };
    if dry_run {
        doc.would_transfer = selected.ids.clone();
        let present = if target.remote.is_none() { ctx.host.split_known_ids(&target.id, &selected.ids)?.0 } else { Vec::new() };
        let mut text = format!("Would {verb} {} from '{}' to '{}':\n", plural(selected.ids.len(), "item", "items"), source.name, target.name);
        for id in &selected.ids {
            let note = if present.contains(id) { if on_conflict == OnConflict::Replace { "  (replaces the one there)" } else { "  (already there: skipped)" } } else { "" };
            text.push_str(&format!("  {id}{note}\n"));
        }
        return ctx.report(&doc, &text);
    }
    if selected.ids.is_empty() {
        return ctx.report(&doc, "Nothing selected\n");
    }
    ctx.open_for_write(&target)?;
    if moving {
        ctx.open_for_write(&source)?;
        prompt::confirm(ctx, &format!("Move {} from '{}' to '{}'? The originals are deleted once they have arrived.", plural(selected.ids.len(), "item", "items"), source.name, target.name))?;
    }
    let live = ctx.progress_visible();
    let report = ctx.host.transfer_assets(&source.id, &target.id, &selected.ids, mode, on_conflict, include_clips, &|event| {
        if live {
            out::stderr(&format!("\r{} {}/{}", verb, event.progress.done, event.progress.total));
        }
    })?;
    if live {
        out::stderr("\n");
    }
    doc.copied = report.count(Outcome::Copied);
    doc.replaced = report.count(Outcome::Replaced);
    doc.skipped = report.count(Outcome::Skipped);
    doc.failed = report.count(Outcome::Failed);
    doc.removed = report.items.iter().filter(|item| item.source_removed).count();
    doc.report = report;
    let mut table = Table::new(&["RESULT", "ID", "NAME", "NOTE"]);
    for item in &doc.report.items {
        table.row(vec![format!("{:?}", item.outcome).to_lowercase(), item.id.clone(), item.name.clone(), item.reason.clone().unwrap_or_default()]);
    }
    let mut text = table.render();
    text.push_str(&format!("{} copied, {} replaced, {} skipped, {} failed", doc.copied, doc.replaced, doc.skipped, doc.failed));
    if moving {
        text.push_str(&format!(", {} removed from '{}'", doc.removed, source.name));
    }
    text.push('\n');
    let failed = doc.failed;
    let total = doc.report.items.len();
    ctx.report(&doc, &text)?;
    if failed > 0 {
        return Err(CliError::partial(format!("{failed} of {total} items failed")));
    }
    Ok(())
}

pub fn delete(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let selected = selection::resolve(ctx, matches, true, "delete")?;
    let library = &selected.library;
    let dry_run = flag(matches, "dry-run");
    let mut doc = DeleteDoc { library: library.id.clone(), dry_run, deleted: selected.ids.clone(), remaining: None };
    if dry_run {
        let text = format!("Would delete {} from '{}':\n{}", plural(selected.ids.len(), "item", "items"), library.name, selected.ids.iter().map(|id| format!("  {id}\n")).collect::<String>());
        return ctx.report(&doc, &text);
    }
    if selected.ids.is_empty() {
        doc.deleted.clear();
        return ctx.report(&doc, "Nothing selected\n");
    }
    ctx.open_for_write(library)?;
    prompt::confirm(ctx, &format!("Delete {} and their files from '{}'? This cannot be undone.", plural(selected.ids.len(), "item", "items"), library.name))?;
    let result = ctx.host.delete_assets(&library.id, selected.ids.clone())?;
    doc.deleted = result.deleted;
    doc.remaining = Some(result.summary.count);
    let text = format!("Deleted {} from '{}' ({} left)\n", plural(doc.deleted.len(), "item", "items"), library.name, result.summary.count);
    let missed = selected.ids.len() - doc.deleted.len().min(selected.ids.len());
    ctx.report(&doc, &text)?;
    if missed > 0 {
        return Err(CliError::partial(format!("{missed} of {} items could not be deleted", selected.ids.len())));
    }
    Ok(())
}

pub fn thumbs(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let (action, sub) = matches.subcommand().ok_or_else(|| CliError::usage("choose status or clear"))?;
    let library = ctx.library()?;
    ctx.open(&library)?;
    if action == "status" {
        let pending = ctx.host.pending_thumb_count(&library.id)?;
        let items = ctx.host.item_count(&library.id).unwrap_or(0);
        let doc = ThumbStatusDoc { library: library.id.clone(), pending, items };
        return ctx.output(&doc, &format!("{pending} of {items} items wait for a thumbnail\n"));
    }
    let dry_run = flag(sub, "dry-run");
    let chosen = sub.try_get_many::<String>("ids").ok().flatten().is_some_and(|ids| ids.len() > 0) || sub.try_get_one::<String>("type").ok().flatten().is_some() || selection::item_filter(sub).is_narrowing();
    if !chosen {
        let doc = ThumbsClearDoc { library: library.id.clone(), dry_run, cleared: None, all: true };
        if dry_run {
            return ctx.report(&doc, &format!("Would discard every thumbnail of '{}'\n", library.name));
        }
        prompt::confirm(ctx, &format!("Discard every thumbnail of '{}'? The app draws them again.", library.name))?;
        ctx.host.clear_thumbs(&library.id, None, None)?;
        return ctx.report(&doc, &format!("Discarded every thumbnail of '{}'\n", library.name));
    }
    let selected = selection::resolve(ctx, sub, true, "discard the thumbnails of")?;
    let doc = ThumbsClearDoc { library: library.id.clone(), dry_run, cleared: Some(selected.ids.len()), all: false };
    if dry_run {
        return ctx.report(&doc, &format!("Would discard the thumbnails of {}\n", plural(selected.ids.len(), "item", "items")));
    }
    prompt::confirm(ctx, &format!("Discard the thumbnails of {}?", plural(selected.ids.len(), "item", "items")))?;
    ctx.host.clear_thumbs(&library.id, Some(selected.ids.clone()), None)?;
    ctx.report(&doc, &format!("Discarded the thumbnails of {}\n", plural(selected.ids.len(), "item", "items")))
}
