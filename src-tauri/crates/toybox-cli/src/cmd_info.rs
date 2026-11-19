//! `status`, `help` and `completions`.

use clap::{ArgMatches, Command};
use clap_complete::Shell;
use toybox_core::infra::store::Store;

use crate::ctx::{Ctx, Origin};
use crate::error::{CliError, CliResult};
use crate::json::{library_doc, HelpDoc, HelpEntry, PathsDoc, PluginSummary, StatusDoc};
use crate::out::{self, Table};
use crate::plugin_runtime::PluginCli;

pub fn status(ctx: &Ctx, plugins: &[PluginCli]) -> CliResult<()> {
    let host = &ctx.host;
    let paths = host.paths();
    let mut problems: Vec<String> = Vec::new();
    let mut libraries = Vec::new();
    for info in host.list_libraries() {
        let items = if info.available {
            host.resolve(&info.id).ok().map(|handle| Store::for_handle(&handle)).filter(|store| store.catalog_exists()).and_then(|store| store.asset_count().ok())
        } else {
            None
        };
        let doc = library_doc(&info, items);
        match doc.state.as_str() {
            "missing" => problems.push(format!("library '{}' is missing: {} cannot be read", info.name, info.root)),
            "unreachable" => problems.push(format!("library '{}' has never been reachable", info.name)),
            "offline" => problems.push(format!("library '{}' is offline: its server cannot be reached", info.name)),
            "read-only" => problems.push(format!("library '{}' is read-only", info.name)),
            _ => {}
        }
        libraries.push(doc);
    }
    let installed = host.list_plugins()?;
    let mut summary = PluginSummary { on: Vec::new(), off: Vec::new(), blocked: Vec::new() };
    let mut blocked_text = Vec::new();
    for plugin in &installed {
        if !plugin.enabled {
            summary.off.push(plugin.id.clone());
        } else if !plugin.blocked_by.is_empty() {
            summary.blocked.push(plugin.id.clone());
            let reasons: Vec<String> = plugin.blocked_by.iter().map(|blocked| format!("{} {}", blocked.id, blocked.reason)).collect();
            let line = format!("plugin '{}' is blocked: {}", plugin.id, reasons.join(", "));
            blocked_text.push(line.clone());
            problems.push(line);
        } else {
            summary.on.push(plugin.id.clone());
        }
    }
    for plugin in plugins {
        for problem in &plugin.problems {
            problems.push(format!("plugin '{}': {problem}", plugin.plugin_id));
        }
    }
    let doc = StatusDoc {
        paths: PathsDoc {
            app_data: paths.app_data.to_string_lossy().to_string(),
            cache: paths.cache.to_string_lossy().to_string(),
            settings: paths.settings_path().to_string_lossy().to_string(),
            plugins: paths.plugins_dir().to_string_lossy().to_string(),
            exists: paths.app_data.is_dir(),
        },
        libraries,
        plugins: summary,
        problems,
    };
    let mut text = String::new();
    let whose = match &ctx.origin {
        Origin::Home => "home".to_string(),
        Origin::AppId(id) => format!("app {id}"),
    };
    text.push_str(&format!("data      {} ({whose}{})\n", doc.paths.app_data, if doc.paths.exists { "" } else { ", not created yet" }));
    text.push_str(&format!("cache     {}\n", doc.paths.cache));
    text.push('\n');
    if doc.libraries.is_empty() {
        text.push_str("libraries none registered (try `tbx library create <DIR>`)\n");
    } else {
        text.push_str(&format!("libraries {}\n", doc.libraries.len()));
        let mut table = Table::new(&["", "ID", "NAME", "KIND", "ITEMS", "STATE", "LOCATION"]);
        for library in &doc.libraries {
            table.row(vec![
                if library.default { "*".into() } else { String::new() },
                library.id.clone(),
                library.name.clone(),
                library.kind.clone(),
                library.items.map(|items| items.to_string()).unwrap_or_else(|| "-".into()),
                library.state.clone(),
                library.location.clone(),
            ]);
        }
        for line in table.render().lines() {
            text.push_str(&format!("  {line}\n"));
        }
    }
    text.push('\n');
    text.push_str(&format!("plugins   {} on, {} off, {} blocked\n", doc.plugins.on.len(), doc.plugins.off.len(), doc.plugins.blocked.len()));
    if !doc.plugins.off.is_empty() {
        text.push_str(&format!("  off: {}\n", doc.plugins.off.join(", ")));
    }
    if !doc.problems.is_empty() {
        text.push_str("\nproblems\n");
        for problem in &doc.problems {
            text.push_str(&format!("  {problem}\n"));
        }
    }
    ctx.output(&doc, &text)
}

struct Node {
    path: Vec<String>,
    summary: String,
}

fn walk(command: &Command, path: &mut Vec<String>, out: &mut Vec<Node>) {
    for sub in command.get_subcommands() {
        if sub.is_hide_set() {
            continue;
        }
        path.push(sub.get_name().to_string());
        out.push(Node { path: path.clone(), summary: sub.get_about().map(|about| about.to_string()).unwrap_or_default() });
        walk(sub, path, out);
        path.pop();
    }
}

/// The plugin that adds the command at `path`, if one does.
fn plugin_of(plugins: &[PluginCli], path: &[String]) -> Option<String> {
    match path {
        [first, name] if first == "import" => plugins.iter().find(|plugin| plugin.importers.iter().any(|importer| &importer.name == name)).map(|plugin| plugin.plugin_id.clone()),
        [first, name] if first == "export" => plugins.iter().find(|plugin| plugin.exporter.as_ref().is_some_and(|exporter| &exporter.name == name)).map(|plugin| plugin.plugin_id.clone()),
        [first, ..] => plugins.iter().find(|plugin| plugin.commands.iter().any(|command| &command.name == first)).map(|plugin| plugin.plugin_id.clone()),
        [] => None,
    }
}

pub fn help(ctx: &Ctx, root: &Command, plugins: &[PluginCli], matches: &ArgMatches) -> CliResult<()> {
    if matches.get_flag("all") {
        let mut nodes = Vec::new();
        walk(root, &mut Vec::new(), &mut nodes);
        let doc = HelpDoc {
            commands: nodes.iter().map(|node| HelpEntry { command: node.path.join(" "), summary: node.summary.clone(), plugin: plugin_of(plugins, &node.path) }).collect(),
        };
        let labels: Vec<String> = nodes.iter().map(|node| format!("{}tbx {}", "  ".repeat(node.path.len() - 1), node.path.join(" "))).collect();
        let width = labels.iter().map(|label| label.chars().count()).max().unwrap_or(0);
        let mut text = String::new();
        for ((label, node), entry) in labels.iter().zip(&nodes).zip(&doc.commands) {
            let suffix = entry.plugin.as_ref().map(|plugin| format!(" [{plugin}]")).unwrap_or_default();
            text.push_str(&format!("{label:<width$}  {}{suffix}
", node.summary));
        }
        return ctx.output(&doc, &text);
    }
    let names: Vec<String> = matches.try_get_many::<String>("command").ok().flatten().map(|values| values.cloned().collect()).unwrap_or_default();
    let mut current = root.clone();
    for name in &names {
        let next = current.find_subcommand(name).cloned().ok_or_else(|| CliError::usage(format!("no such command: {}", names.join(" "))).hint("`tbx help --all` lists every command"))?;
        current = next;
    }
    current = current.bin_name(format!("tbx {}", names.join(" ")).trim().to_string());
    out::stdout(&format!("{}\n", current.render_long_help()));
    Ok(())
}

pub fn completions(root: &Command, matches: &ArgMatches) -> CliResult<()> {
    let shell = match matches.get_one::<String>("shell").map(String::as_str) {
        Some("bash") => Shell::Bash,
        Some("zsh") => Shell::Zsh,
        Some("fish") => Shell::Fish,
        Some("powershell") => Shell::PowerShell,
        other => return Err(CliError::usage(format!("unknown shell: {}", other.unwrap_or("")))),
    };
    let mut command = root.clone();
    let mut buffer = Vec::new();
    clap_complete::generate(shell, &mut command, "tbx", &mut buffer);
    out::stdout(&String::from_utf8_lossy(&buffer));
    Ok(())
}
