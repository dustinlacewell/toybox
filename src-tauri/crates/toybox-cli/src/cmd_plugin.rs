//! `plugin`, `config` and `debug`.

use std::path::PathBuf;

use clap::ArgMatches;
use serde_json::Value;
use toybox_host::plugins::PluginManifestDto;

use crate::cli::absolute;
use crate::ctx::{Ctx, Origin};
use crate::error::{CliError, CliResult};
use crate::json::*;
use crate::out::{self, truncate, Table};
use crate::plugin_runtime::PluginCli;
use crate::prompt;

fn state_of(plugin: &PluginManifestDto) -> &'static str {
    if !plugin.enabled {
        "off"
    } else if !plugin.blocked_by.is_empty() {
        "blocked"
    } else {
        "on"
    }
}

fn blocked_reasons(plugin: &PluginManifestDto) -> Vec<String> {
    plugin
        .blocked_by
        .iter()
        .map(|blocked| match (&blocked.needs, &blocked.found) {
            (Some(needs), Some(found)) => format!("{} needs {needs}, found {found}", blocked.id),
            _ => format!("{} {}", blocked.id, blocked.reason),
        })
        .collect()
}

fn provides(plugin: &PluginManifestDto, cli: Option<&PluginCli>, installed: &[PluginManifestDto]) -> Vec<String> {
    let mut out = Vec::new();
    if plugin.asset_types.as_array().is_some_and(|types| !types.is_empty()) {
        out.push("types".to_string());
    }
    if let Some(cli) = cli {
        if !cli.importers.is_empty() {
            out.push("importers".into());
        }
        if cli.exporter.is_some() {
            out.push("exporter".into());
        }
        if !cli.commands.is_empty() {
            out.push("commands".into());
        }
    }
    let ui = |name: &str| plugin.ui.get(name).is_some_and(|value| !value.is_null() && value != "");
    if ui("importPanel") {
        out.push("import panel".into());
    }
    if ui("exportPanel") {
        out.push("export panel".into());
    }
    if plugin.settings.as_array().is_some_and(|settings| !settings.is_empty()) {
        out.push("settings".into());
    }
    // A plugin that only extends others (more formats, rig profiles, a module with nothing to mount)
    // is described by what it extends.
    if out.is_empty() && !plugin.dependencies.is_empty() {
        let names: Vec<String> = plugin
            .dependencies
            .keys()
            .map(|id| installed.iter().find(|other| &other.id == id).map(|other| other.name.clone()).unwrap_or_else(|| id.clone()))
            .collect();
        out.push(format!("extends {}", names.join(", ")));
    }
    out
}

fn plugin_doc(plugin: &PluginManifestDto, cli: Option<&PluginCli>, installed: &[PluginManifestDto]) -> PluginDoc {
    PluginDoc {
        id: plugin.id.clone(),
        name: plugin.name.clone(),
        version: plugin.version.clone(),
        state: state_of(plugin).to_string(),
        blocked_by: blocked_reasons(plugin),
        provides: provides(plugin, cli, installed),
        problems: cli.map(|cli| cli.problems.clone()).unwrap_or_default(),
    }
}

pub fn plugin(ctx: &Ctx, clis: &[PluginCli], matches: &ArgMatches) -> CliResult<()> {
    match matches.subcommand() {
        Some(("list", _)) => list(ctx, clis),
        Some(("info", sub)) => info(ctx, clis, sub),
        Some(("enable", sub)) => switch(ctx, sub, true),
        Some(("disable", sub)) => switch(ctx, sub, false),
        Some(("install", sub)) => install(ctx, sub),
        Some(("remove", sub)) => remove(ctx, sub),
        Some(("settings", sub)) => settings(ctx, sub),
        _ => Err(CliError::usage("choose a plugin command").hint("see `tbx plugin --help`")),
    }
}

fn cli_of<'a>(clis: &'a [PluginCli], id: &str) -> Option<&'a PluginCli> {
    clis.iter().find(|cli| cli.plugin_id == id)
}

fn list(ctx: &Ctx, clis: &[PluginCli]) -> CliResult<()> {
    let plugins = ctx.host.list_plugins()?;
    let mut table = Table::new(&["ID", "VERSION", "STATE", "PROVIDES"]);
    let mut docs = Vec::new();
    let mut notes = String::new();
    for plugin in &plugins {
        let doc = plugin_doc(plugin, cli_of(clis, &plugin.id), &plugins);
        let state = if doc.blocked_by.is_empty() { doc.state.clone() } else { format!("blocked ({})", doc.blocked_by.join("; ")) };
        table.row(vec![doc.id.clone(), doc.version.clone(), state, doc.provides.join(", ")]);
        for problem in &doc.problems {
            notes.push_str(&format!("note: {}: {problem}\n", doc.id));
        }
        docs.push(doc);
    }
    let text = table.render() + &notes;
    ctx.output(&PluginsDoc { plugins: docs }, &text)
}

fn find(ctx: &Ctx, id: &str) -> CliResult<PluginManifestDto> {
    ctx.host.list_plugins()?.into_iter().find(|plugin| plugin.id == id).ok_or_else(|| CliError::not_found(format!("no plugin '{id}' is installed")).hint("see `tbx plugin list`"))
}

fn setting_docs(ctx: &Ctx, plugin: &PluginManifestDto) -> CliResult<Vec<PluginSettingDoc>> {
    let values = ctx.host.plugin_settings(&plugin.id)?;
    Ok(plugin
        .settings
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|setting| {
            let key = setting["key"].as_str()?.to_string();
            Some(PluginSettingDoc {
                kind: setting["type"].as_str().unwrap_or("text").to_string(),
                label: setting["label"].as_str().unwrap_or(&key).to_string(),
                value: values.get(&key).cloned().unwrap_or(Value::Null),
                default: setting.get("default").cloned().unwrap_or(Value::Null),
                key,
            })
        })
        .collect())
}

fn info(ctx: &Ctx, clis: &[PluginCli], matches: &ArgMatches) -> CliResult<()> {
    let id = matches.get_one::<String>("id").cloned().unwrap_or_default();
    let plugin = find(ctx, &id)?;
    let cli = cli_of(clis, &id);
    let types: Vec<String> = plugin.asset_types.as_array().into_iter().flatten().filter_map(|declared| declared["id"].as_str().map(String::from)).collect();
    let importers: Vec<String> = cli.map(|cli| cli.importers.iter().map(|importer| importer.name.clone()).collect()).unwrap_or_default();
    let exporter = cli.and_then(|cli| cli.exporter.as_ref().map(|exporter| exporter.name.clone()));
    let commands: Vec<String> = cli.map(|cli| cli.commands.iter().map(|command| command.name.clone()).collect()).unwrap_or_default();
    let doc = PluginInfoDoc {
        plugin: plugin_doc(&plugin, cli, &ctx.host.list_plugins()?),
        description: plugin.description.clone(),
        path: ctx.host.paths().plugins_dir().join(&plugin.id).to_string_lossy().to_string(),
        dependencies: serde_json::to_value(&plugin.dependencies)?,
        required_by: plugin.required_by.clone(),
        types,
        importers,
        exporter,
        commands,
        permissions: plugin.permissions.clone(),
        settings: setting_docs(ctx, &plugin)?,
    };
    let mut text = format!("id           {}\nname         {}\nversion      {}\nstate        {}\n", doc.plugin.id, doc.plugin.name, doc.plugin.version, doc.plugin.state);
    let mut field = |name: &str, value: String| {
        if !value.is_empty() {
            text.push_str(&format!("{name:<13}{value}\n"));
        }
    };
    field("description", doc.description.clone().unwrap_or_default());
    field("path", doc.path.clone());
    field("blocked by", doc.plugin.blocked_by.join("; "));
    field("needs", plugin.dependencies.iter().map(|(id, range)| format!("{id} {range}")).collect::<Vec<_>>().join(", "));
    field("needed by", doc.required_by.join(", "));
    field("types", doc.types.join(", "));
    field("importers", doc.importers.iter().map(|name| format!("tbx import {name}")).collect::<Vec<_>>().join(", "));
    field("exporter", doc.exporter.iter().map(|name| format!("tbx export {name}")).collect::<Vec<_>>().join(", "));
    field("commands", doc.commands.iter().map(|name| format!("tbx {name}")).collect::<Vec<_>>().join(", "));
    field("permissions", doc.permissions.as_object().map(|map| map.iter().filter(|(_, value)| value.as_bool() == Some(true)).map(|(name, _)| name.clone()).collect::<Vec<_>>().join(", ")).unwrap_or_default());
    field("not mounted", doc.plugin.problems.join("; "));
    if !doc.settings.is_empty() {
        text.push_str("settings\n");
        for setting in &doc.settings {
            text.push_str(&format!("  {:<22}{}\n", setting.key, if setting.value.is_null() { "(not set)".to_string() } else { value_text(&setting.value) }));
        }
    }
    ctx.output(&doc, &text)
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn switch(ctx: &Ctx, matches: &ArgMatches, enable: bool) -> CliResult<()> {
    let id = matches.get_one::<String>("id").cloned().unwrap_or_default();
    find(ctx, &id)?;
    let changed = ctx.host.set_plugin_enabled(&id, enable)?;
    let text = if changed.is_empty() {
        format!("{id} is already {}\n", if enable { "on" } else { "off" })
    } else {
        format!("Turned {} {}\n", if enable { "on" } else { "off" }, changed.join(", "))
    };
    ctx.report(&PluginSwitchDoc { id, enabled: enable, changed }, &text)
}

fn install(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let dir = PathBuf::from(absolute(matches.get_one::<String>("dir").map(String::as_str).unwrap_or(".")));
    let installed = ctx.host.install_plugin(&dir, matches.get_flag("link"))?;
    let doc = PluginInstalledDoc { id: installed.id.clone(), name: installed.name.clone(), version: installed.version.clone(), path: installed.path.to_string_lossy().to_string(), linked: installed.linked };
    ctx.report(&doc, &format!("Installed {} {} ({})\n", installed.id, installed.version, if installed.linked { "linked" } else { "copied" }))
}

fn remove(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let id = matches.get_one::<String>("id").cloned().unwrap_or_default();
    find(ctx, &id)?;
    prompt::confirm(ctx, &format!("Remove plugin '{id}'? Its settings are forgotten."))?;
    ctx.host.remove_plugin(&id)?;
    ctx.report(&PluginRemovedDoc { id: id.clone() }, &format!("Removed {id}\n"))
}

fn settings(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let id = matches.get_one::<String>("id").cloned().unwrap_or_default();
    let plugin = find(ctx, &id)?;
    let Some(key) = matches.get_one::<String>("key").cloned() else {
        let docs = setting_docs(ctx, &plugin)?;
        let mut table = Table::new(&["KEY", "VALUE", "DEFAULT"]);
        for setting in &docs {
            table.row(vec![setting.key.clone(), truncate(&value_text(&setting.value), 60), truncate(&value_text(&setting.default), 40)]);
        }
        return ctx.output(&serde_json::json!({ "plugin": id, "settings": docs }), &table.render());
    };
    let declared = plugin.settings.as_array().into_iter().flatten().find(|setting| setting["key"] == key.as_str()).cloned();
    let Some(declared) = declared else {
        return Err(CliError::not_found(format!("plugin '{id}' has no setting '{key}'")).hint(format!("see `tbx plugin settings {id}`")));
    };
    if matches.get_flag("unset") {
        ctx.host.set_plugin_setting(&id, &key, Value::Null)?;
        return ctx.report(&PluginSettingValueDoc { plugin: id.clone(), key: key.clone(), value: Value::Null }, &format!("{key} of {id} is back to its default\n"));
    }
    match matches.get_one::<String>("value") {
        None => {
            let value = ctx.host.plugin_settings(&id)?.get(&key).cloned().unwrap_or(Value::Null);
            ctx.output(&PluginSettingValueDoc { plugin: id, key, value: value.clone() }, &format!("{}\n", value_text(&value)))
        }
        Some(text) => {
            let value = match declared["type"].as_str() {
                Some("checkbox" | "boolean") => match text.to_lowercase().as_str() {
                    "true" | "yes" | "on" | "1" => Value::Bool(true),
                    "false" | "no" | "off" | "0" => Value::Bool(false),
                    _ => return Err(CliError::usage(format!("'{key}' is on or off, not '{text}'"))),
                },
                Some("number") => text.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).ok_or_else(|| CliError::usage(format!("'{key}' is a number, not '{text}'")))?,
                _ => Value::String(text.clone()),
            };
            ctx.host.set_plugin_setting(&id, &key, value.clone())?;
            ctx.report(&PluginSettingValueDoc { plugin: id.clone(), key: key.clone(), value }, &format!("Set {key} of {id}\n"))
        }
    }
}

// ---- config ----------------------------------------------------------------------------

const CONFIG_KEYS: &[&str] = &["fbx2gltf-path"];

fn config_key(matches: &ArgMatches) -> CliResult<String> {
    let key = matches.get_one::<String>("key").cloned().unwrap_or_default();
    if CONFIG_KEYS.contains(&key.as_str()) {
        Ok(key)
    } else {
        Err(CliError::usage(format!("unknown setting '{key}'")).hint(format!("settings: {}", CONFIG_KEYS.join(", "))))
    }
}

fn config_value(ctx: &Ctx, key: &str) -> CliResult<Value> {
    Ok(match key {
        "fbx2gltf-path" => ctx.host.fbx2gltf_path()?.map(Value::String).unwrap_or(Value::Null),
        _ => Value::Null,
    })
}

pub fn config(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    match matches.subcommand() {
        Some(("list", _)) => {
            let mut map = serde_json::Map::new();
            let mut table = Table::new(&["KEY", "VALUE"]);
            for key in CONFIG_KEYS {
                let value = config_value(ctx, key)?;
                table.row(vec![key.to_string(), value_text(&value)]);
                map.insert(key.to_string(), value);
            }
            ctx.output(&ConfigDoc { settings: map }, &table.render())
        }
        Some(("get", sub)) => {
            let key = config_key(sub)?;
            let value = config_value(ctx, &key)?;
            let text = if value.is_null() { String::new() } else { format!("{}\n", value_text(&value)) };
            ctx.output(&ConfigValueDoc { key, value }, &text)
        }
        Some(("set", sub)) => {
            let key = config_key(sub)?;
            let value = sub.get_one::<String>("value").cloned().unwrap_or_default();
            match key.as_str() {
                "fbx2gltf-path" => ctx.host.set_fbx2gltf_path(Some(value.clone()))?,
                _ => {}
            }
            let stored = config_value(ctx, &key)?;
            ctx.report(&ConfigValueDoc { key: key.clone(), value: stored }, &format!("Set {key}\n"))
        }
        Some(("unset", sub)) => {
            let key = config_key(sub)?;
            if key == "fbx2gltf-path" {
                ctx.host.set_fbx2gltf_path(None)?;
            }
            ctx.report(&ConfigValueDoc { key: key.clone(), value: Value::Null }, &format!("Cleared {key}\n"))
        }
        _ => Err(CliError::usage("choose list, get, set or unset")),
    }
}

// ---- debug -----------------------------------------------------------------------------

pub fn debug(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    match matches.subcommand() {
        Some(("paths", _)) => debug_paths(ctx),
        Some(("sql", sub)) => debug_sql(ctx, sub),
        _ => Err(CliError::usage("choose paths or sql")),
    }
}

fn debug_paths(ctx: &Ctx) -> CliResult<()> {
    let paths = ctx.host.paths();
    let secrets = match &ctx.secrets_file {
        Some(file) => format!("file {} (TOYBOX_SECRETS)", file.display()),
        None => "the system credential store".to_string(),
    };
    let text_of = |path: &std::path::Path| path.to_string_lossy().to_string();
    let mut libraries = Vec::new();
    for info in ctx.host.list_libraries() {
        let Ok(handle) = ctx.host.resolve(&info.id) else { continue };
        libraries.push(DebugLibraryPaths {
            id: info.id.clone(),
            name: info.name.clone(),
            root: text_of(&handle.root),
            database: text_of(&handle.library_db_path()),
            user_database: text_of(&handle.user_db_path()),
            app_data: text_of(&handle.data_dir()),
            thumbnails: text_of(&handle.thumbs_dir),
        });
    }
    let doc = DebugPathsDoc {
        app_data: text_of(&paths.app_data),
        cache: text_of(&paths.cache),
        settings: text_of(&paths.settings_path()),
        plugins: text_of(&paths.plugins_dir()),
        remote_files: text_of(&paths.remote_files_dir()),
        plugin_scratch: text_of(&paths.plugin_scratch_root()),
        browse_index: text_of(&paths.app_data.join("browse-index.db")),
        secrets,
        libraries,
    };
    let source = match &ctx.origin {
        Origin::Home => "--home / TOYBOX_HOME".to_string(),
        Origin::AppId(id) => format!("app id {id}"),
    };
    let mut text = format!(
        "source          {source}\napp data        {}\ncache           {}\nsettings        {}\nplugins         {}\nremote files    {}\nplugin scratch  {}\nbrowse index    {}\ntokens          {}\n",
        doc.app_data, doc.cache, doc.settings, doc.plugins, doc.remote_files, doc.plugin_scratch, doc.browse_index, doc.secrets
    );
    for library in &doc.libraries {
        text.push_str(&format!("\nlibrary {} ({})\n  folder       {}\n  database     {}\n  user data    {}\n  app data     {}\n  thumbnails   {}\n", library.name, library.id, library.root, library.database, library.user_database, library.app_data, library.thumbnails));
    }
    ctx.output(&doc, &text)
}

fn debug_sql(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let library = ctx.library()?;
    let query = matches.get_one::<String>("query").cloned().unwrap_or_default();
    let limit = matches.get_one::<usize>("limit").copied().unwrap_or(1000);
    ctx.open(&library)?;
    let table = ctx.host.query_sql(&library.id, &query, limit)?;
    let mut text_table = Table::with_headers(&table.columns);
    for row in &table.rows {
        text_table.row(row.iter().map(value_text).collect());
    }
    let text = text_table.render();
    if table.truncated {
        out::stderr(&format!("showing the first {} rows (use --limit 0 for all)\n", table.rows.len()));
    }
    ctx.output(&SqlDoc { columns: table.columns, rows: table.rows, truncated: table.truncated }, &text)
}
