//! The commands that can run plugin code: `import`, `export` and the commands plugins add.
//! A `PluginRuntime` is created only when one of them actually runs plugin code.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use clap::ArgMatches;
use serde_json::{Map, Value};
use toybox_core::domain::export_model::{ExportCopyReq, ExportGlbReq, ExportReport};
use toybox_host::edit::parse_tag_list;

use crate::cli::{absolute, plugin_values};
use crate::ctx::Ctx;
use crate::error::{CliError, CliResult};
use crate::json::*;
use crate::out::{self, plural, Table};
use crate::plugin_runtime::{CliCommand, CommandRequest, Event, ExportRequest, ImportDefaults, ImportRequest, PluginCli, PluginRuntime};
use crate::selection;

fn flag(matches: &ArgMatches, name: &str) -> bool {
    matches.try_get_one::<bool>(name).ok().flatten().copied().unwrap_or(false)
}

fn text_arg(matches: &ArgMatches, name: &str) -> Option<String> {
    matches.try_get_one::<String>(name).ok().flatten().cloned().filter(|text| !text.trim().is_empty())
}

/// A running plugin command and the flag Ctrl-C sets.
struct Running {
    runtime: PluginRuntime,
    cancel: Arc<AtomicBool>,
}

impl Running {
    /// A failure of the run, as the command line reports it: a run that Ctrl-C stopped is `cancelled`.
    fn error(&self, error: toybox_core::error::AppError) -> CliError {
        if self.cancel.load(Ordering::Relaxed) || error.to_string() == "Cancelled" {
            CliError::cancelled("cancelled")
        } else {
            // A plugin may end its message with a line "hint: <what to do>"; it is shown as the hint.
            let message = error.to_string();
            match message.split_once("\nhint: ") {
                Some((what, hint)) => CliError::failed(what.to_string()).hint(hint.trim().to_string()),
                None => error.into(),
            }
        }
    }
}

/// The first Ctrl-C asks the run to stop (its `signal` aborts and every host call fails); the second
/// stops script that is running right now, stops the programs plugins started and ends `tbx`
/// with status 130.
fn handle_interrupts(ctx: &Ctx, runtime: &PluginRuntime) {
    let (cancel, interrupt, host) = (runtime.cancel_flag(), runtime.interrupt_flag(), ctx.host.clone());
    let presses = AtomicUsize::new(0);
    let _ = ctrlc::set_handler(move || {
        if presses.fetch_add(1, Ordering::SeqCst) == 0 {
            cancel.store(true, Ordering::SeqCst);
            out::stderr("Cancelling... (press Ctrl-C again to stop at once)\n");
        } else {
            interrupt.store(true, Ordering::SeqCst);
            host.tools.stop_all();
            std::process::exit(130);
        }
    });
}

/// Load the plugins' command-line modules; what they say while running goes to the right stream.
fn runtime(ctx: &Ctx) -> CliResult<Running> {
    let (json, quiet, verbose, color) = (ctx.g.json, ctx.g.quiet, ctx.g.verbose, ctx.g.color);
    let sink = Box::new(move |event: Event| match event {
        // Under --json standard output is the one JSON document, so a plugin's own prints go to standard error.
        Event::Print(text) if json => out::stderr(&format!("{text}\n")),
        Event::Print(text) => out::line(&text),
        Event::Log(message) => {
            if !quiet {
                out::stderr(&format!("{message}\n"));
            }
        }
        Event::Warn(message) => {
            if !quiet {
                out::stderr(&format!("{}: {message}\n", out::paint("warning", "1;33", color)));
            }
        }
        Event::Console(message) => {
            if verbose {
                out::stderr(&format!("{message}\n"));
            }
        }
    });
    let runtime = PluginRuntime::load(ctx.host.clone(), sink)?;
    for (plugin, problem) in runtime.problems() {
        ctx.warn(&format!("plugin '{plugin}' did not load: {problem}"));
    }
    handle_interrupts(ctx, &runtime);
    let cancel = runtime.cancel_flag();
    Ok(Running { runtime, cancel })
}

/// The items an import replaced, one line each on standard error (always: existing data was overwritten).
fn report_replaced(replaced: &[crate::plugin_runtime::ReplacedItem]) {
    for item in replaced {
        match &item.was {
            Some(was) => out::stderr(&format!("replaced: {} (was {was})\n", item.name)),
            None => out::stderr(&format!("replaced: {}\n", item.name)),
        }
    }
}

/// What an importer or exporter left out, one line each on standard error (always: it is data that
/// was not handled). A leading "skipped " in the plugin's words is not said twice.
fn report_skipped(skipped: &[String]) {
    for reason in skipped {
        out::stderr(&format!("skipped: {}\n", reason.strip_prefix("skipped ").unwrap_or(reason)));
    }
}

// ---- import ----------------------------------------------------------------------------

pub fn import(ctx: &Ctx, plugins: &[PluginCli], matches: &ArgMatches) -> CliResult<()> {
    let Some((name, sub)) = matches.subcommand() else {
        let mut table = Table::new(&["IMPORTER", "PLUGIN", "SUMMARY"]);
        let mut docs = Vec::new();
        for plugin in plugins {
            for importer in &plugin.importers {
                table.row(vec![importer.name.clone(), plugin.plugin_id.clone(), importer.summary.clone()]);
                docs.push(ImporterDoc { name: importer.name.clone(), plugin: plugin.plugin_id.clone(), summary: importer.summary.clone() });
            }
        }
        let text = if docs.is_empty() { "No importers: no active plugin provides one (see `tbx plugin list`)\n".to_string() } else { table.render() };
        return ctx.output(&ImporterListDoc { importers: docs }, &text);
    };
    let (plugin, importer) = plugins
        .iter()
        .find_map(|plugin| plugin.importers.iter().find(|importer| importer.name == name).map(|importer| (plugin, importer)))
        .ok_or_else(|| CliError::not_found(format!("no importer named '{name}'")).hint("`tbx import` lists them"))?;
    let library = ctx.library()?;
    let paths: Vec<String> = sub.try_get_many::<String>("paths").ok().flatten().map(|values| values.map(|path| absolute(path)).collect()).unwrap_or_default();
    for path in &paths {
        if !std::path::Path::new(path).exists() {
            return Err(CliError::not_found(format!("no such file or folder: {path}")));
        }
    }
    let dry_run = flag(sub, "dry-run");
    let defaults = ImportDefaults {
        tags: text_arg(sub, "shared-tag").map(|tags| parse_tag_list(&tags)).unwrap_or_default(),
        source: text_arg(sub, "shared-source"),
        publisher: text_arg(sub, "shared-publisher"),
        license: text_arg(sub, "shared-license"),
        url: text_arg(sub, "shared-url"),
    };
    if dry_run {
        let doc = ImportDoc { library: library.id.clone(), dry_run: true, imported: Vec::new(), replaced: Vec::new(), replaced_items: Vec::new(), skipped: Vec::new(), warnings: Vec::new() };
        let text = format!("Would import {} into '{}' with the '{name}' importer of {}:\n{}", plural(paths.len(), "path", "paths"), library.name, plugin.plugin_id, paths.iter().map(|path| format!("  {path}\n")).collect::<String>());
        return ctx.report(&doc, &text);
    }
    ctx.open_for_write(&library)?;
    let options = plugin_values(sub, &importer.options);
    let runtime = runtime(ctx)?;
    let outcome = runtime.runtime.run_importer(&plugin.plugin_id, name, ImportRequest { library: library.id.clone(), paths, options, defaults }).map_err(|error| runtime.error(error))?;
    for warning in &outcome.warnings {
        ctx.warn(warning);
    }
    report_skipped(&outcome.skipped);
    report_replaced(&outcome.replaced);
    let doc = ImportDoc { library: library.id.clone(), dry_run: false, imported: outcome.imported.clone(), replaced: outcome.replaced.iter().map(|item| item.id.clone()).collect(), replaced_items: outcome.replaced.clone(), skipped: outcome.skipped.clone(), warnings: outcome.warnings.clone() };
    let text: String = outcome.imported.iter().map(|id| format!("{id}\n")).collect();
    ctx.output(&doc, &text)?;
    if !ctx.g.json {
        if outcome.imported.is_empty() {
            ctx.log(&format!("Nothing new was imported into '{}'.", library.name));
        } else {
            let replaced = outcome.replaced.len();
            let added = outcome.imported.len().saturating_sub(replaced);
            let what = if replaced == 0 { plural(outcome.imported.len(), "item", "items") } else { format!("{} ({added} new, {replaced} replaced)", plural(outcome.imported.len(), "item", "items")) };
            ctx.log(&format!("Imported {what} into '{}'. Thumbnails are drawn when the app next opens the library.", library.name));
        }
    }
    if !outcome.skipped.is_empty() {
        return Err(CliError::partial(format!("imported {}, skipped {}", plural(outcome.imported.len(), "item", "items"), outcome.skipped.len())));
    }
    Ok(())
}

// ---- export ----------------------------------------------------------------------------

pub fn export(ctx: &Ctx, plugins: &[PluginCli], matches: &ArgMatches) -> CliResult<()> {
    let Some((name, sub)) = matches.subcommand() else {
        let mut table = Table::new(&["EXPORTER", "PLUGIN", "SUMMARY"]);
        let mut docs = vec![
            ExporterDoc { name: "copy".into(), plugin: None, summary: "Copy the items' files out as loose glTF filesets".into() },
            ExporterDoc { name: "glb".into(), plugin: None, summary: "Write each model as one self-contained .glb".into() },
        ];
        for plugin in plugins {
            if let Some(exporter) = &plugin.exporter {
                docs.push(ExporterDoc { name: exporter.name.clone(), plugin: Some(plugin.plugin_id.clone()), summary: exporter.summary.clone() });
            }
        }
        for doc in &docs {
            table.row(vec![doc.name.clone(), doc.plugin.clone().unwrap_or_else(|| "built in".into()), doc.summary.clone()]);
        }
        return ctx.output(&ExporterListDoc { exporters: docs }, &table.render());
    };
    let selected = selection::resolve(ctx, sub, false, "export")?;
    let target = absolute(sub.get_one::<String>("to").map(String::as_str).unwrap_or("."));
    let preserve = flag(sub, "preserve-structure");
    let library = &selected.library;
    if selected.ids.is_empty() {
        return Err(CliError::not_found("nothing matches the selection"));
    }
    let (report, preview): (ExportReport, bool) = match name {
        "copy" | "glb" => {
            if flag(sub, "dry-run") {
                let planned = ctx.host.preview_export(&library.id, &selected.ids, name, preserve)?;
                (ExportReport { written: planned, ..ExportReport::default() }, true)
            } else if name == "copy" {
                (ctx.host.export_copy(&library.id, &ExportCopyReq { asset_ids: selected.ids.clone(), target_dir: target.clone(), preserve_structure: preserve })?, false)
            } else {
                (ctx.host.export_glb(&library.id, &ExportGlbReq { asset_ids: selected.ids.clone(), target_dir: target.clone(), preserve_structure: preserve })?, false)
            }
        }
        exporter_name => {
            let (plugin, exporter) = plugins
                .iter()
                .find_map(|plugin| plugin.exporter.as_ref().filter(|exporter| exporter.name == exporter_name).map(|exporter| (plugin, exporter)))
                .ok_or_else(|| CliError::not_found(format!("no exporter named '{exporter_name}'")).hint("`tbx export` lists them"))?;
            let mut options: Map<String, Value> = plugin_values(sub, &exporter.options);
            options.insert("variantPolicy".into(), Value::String(sub.get_one::<String>("variants").cloned().unwrap_or_else(|| "all".into())));
            let preview_only = flag(sub, "preview");
            let runtime = runtime(ctx)?;
            let outcome = runtime
                .runtime
                .run_exporter(
                    &plugin.plugin_id,
                    ExportRequest { library: library.id.clone(), asset_ids: selected.ids.clone(), target_dir: target.clone(), preserve_structure: preserve, options, preview_only },
                )
                .map_err(|error| runtime.error(error))?;
            (ExportReport { written: outcome.written, skipped: outcome.skipped, warnings: outcome.warnings }, preview_only)
        }
    };
    for warning in &report.warnings {
        ctx.warn(warning);
    }
    let doc = ExportDoc { library: library.id.clone(), target: target.clone(), preview, written: report.written.clone(), skipped: report.skipped.clone(), warnings: report.warnings.clone() };
    let text: String = report.written.iter().map(|path| format!("{path}\n")).collect();
    ctx.output(&doc, &text)?;
    if !ctx.g.json {
        let verb = if preview { "Would write" } else { "Wrote" };
        ctx.log(&format!("{verb} {} to {target}", plural(report.written.len(), "file", "files")));
    }
    if !report.skipped.is_empty() {
        report_skipped(&report.skipped);
        return Err(CliError::partial(format!("{} skipped", plural(report.skipped.len(), "item was", "items were"))));
    }
    Ok(())
}

// ---- commands plugins add --------------------------------------------------------------

fn find_command<'a>(commands: &'a [CliCommand], path: &[&str]) -> Option<&'a CliCommand> {
    let (first, rest) = path.split_first()?;
    let found = commands.iter().find(|command| command.name == *first)?;
    if rest.is_empty() { Some(found) } else { find_command(&found.commands, rest) }
}

/// The names from the top-level command down to the one that was typed.
fn typed_path<'a>(top: &'a str, matches: &'a ArgMatches) -> Vec<&'a str> {
    let mut path = vec![top];
    let mut current = matches;
    while let Some((name, sub)) = current.subcommand() {
        path.push(name);
        current = sub;
    }
    path
}

fn leaf<'a>(matches: &'a ArgMatches) -> &'a ArgMatches {
    let mut current = matches;
    while let Some((_, sub)) = current.subcommand() {
        current = sub;
    }
    current
}

pub fn plugin_command(ctx: &Ctx, plugins: &[PluginCli], top: &str, matches: &ArgMatches) -> CliResult<()> {
    let path = typed_path(top, matches);
    let (plugin, declared) = plugins
        .iter()
        .find_map(|plugin| find_command(&plugin.commands, &path).map(|command| (plugin, command)))
        .ok_or_else(|| CliError::usage(format!("unknown command '{top}'")).hint("see `tbx help --all`"))?;
    if !declared.commands.is_empty() {
        return Err(CliError::usage(format!("choose one of the commands of '{}'", path.join(" "))));
    }
    let arguments = leaf(matches);
    let library = ctx.library()?;
    let asset_ids = if declared.selection { Some(selection::resolve(ctx, arguments, false, "run on")?.ids) } else { None };
    let args = plugin_values(arguments, &declared.args);
    let options = plugin_values(arguments, &declared.options);
    let runtime = runtime(ctx)?;
    let outcome = runtime.runtime.run_command(&plugin.plugin_id, CommandRequest { library: library.id.clone(), path: path.join(" "), args, options, asset_ids }).map_err(|error| runtime.error(error))?;
    // A command whose result has a non-empty `skipped` list (strings) finished with items left out:
    // like an importer it says so on standard error and ends with status 4.
    let skipped: Vec<String> = outcome.result.get("skipped").and_then(Value::as_array).map(|list| list.iter().filter_map(|item| item.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if ctx.g.json {
        out::json(&outcome.result)?;
    } else if !outcome.printed && !outcome.result.is_null() {
        match &outcome.result {
            Value::String(text) => out::line(text),
            other => out::line(&serde_json::to_string_pretty(other)?),
        }
    }
    if !skipped.is_empty() {
        report_skipped(&skipped);
        return Err(CliError::partial(format!("{} skipped", plural(skipped.len(), "item was", "items were"))));
    }
    Ok(())
}
