//! `tbx library ...`.

use std::path::PathBuf;

use clap::ArgMatches;
use toybox_core::infra::library::{LibraryInfo, RemoteRole};

use crate::cli::absolute;
use crate::ctx::Ctx;
use crate::error::{CliError, CliResult};
use crate::json::*;
use crate::out::{self, human_bytes, Table};
use crate::prompt;

pub fn run(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    match matches.subcommand() {
        Some(("list", _)) => list(ctx),
        Some(("info", sub)) => info(ctx, sub),
        Some(("create", sub)) => add(ctx, sub, true),
        Some(("add", sub)) => add(ctx, sub, false),
        Some(("connect", sub)) => connect(ctx, sub),
        Some(("login", sub)) => login(ctx, sub),
        Some(("rename", sub)) => rename(ctx, sub),
        Some(("default", sub)) => set_default(ctx, sub),
        Some(("remove", sub)) => remove(ctx, sub),
        Some(("scan", sub)) => scan(ctx, sub),
        Some(("check", sub)) => check(ctx, sub),
        Some(("cache", sub)) => cache(ctx, sub),
        Some(("token", sub)) => token(ctx, sub),
        _ => Err(CliError::usage("choose a library command").hint("see `tbx library --help`")),
    }
}

fn named(matches: &ArgMatches) -> Option<&str> {
    matches.try_get_one::<String>("library_arg").ok().flatten().map(String::as_str)
}

/// A line about library format migrations this build would run when the library is opened, for a local
/// library only (cheap: the journal and a read-only look at the database, nothing is written).
fn pending_line(library: &LibraryInfo) -> Option<String> {
    if library.remote.is_some() || !library.available {
        return None;
    }
    let report = toybox_core::infra::preflight::check(std::path::Path::new(&library.root)).ok()?;
    if report.newer_than_this_build {
        return Some(format!("migrated by a newer Toybox (format {}); this tbx cannot open it", report.format_version));
    }
    if report.pending.is_empty() {
        return None;
    }
    let steps: Vec<String> = report.pending.iter().map(|step| format!("{} ({})", step.version, step.name)).collect();
    Some(format!("format {} -> {}: migration {} would run on the next open (toybox-server migrate --check shows what it touches)", report.format_version, report.latest_version, steps.join(", ")))
}

fn remote_only(library: &LibraryInfo) -> CliResult<()> {
    if library.remote.is_none() {
        return Err(CliError::usage(format!("library '{}' is not a remote library", library.name)));
    }
    Ok(())
}

fn list(ctx: &Ctx) -> CliResult<()> {
    let docs: Vec<LibraryDoc> = ctx.host.list_libraries().iter().map(|info| library_doc(info, None)).collect();
    let mut table = Table::new(&["ID", "NAME", "KIND", "LOCATION", "DEFAULT", "STATE"]);
    for library in &docs {
        table.row(vec![library.id.clone(), library.name.clone(), library.kind.clone(), library.location.clone(), if library.default { "default".into() } else { String::new() }, library.state.clone()]);
    }
    ctx.output(&LibrariesDoc { libraries: docs }, &table.render())
}

fn info(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let library = ctx.library_or(named(matches))?;
    ctx.open(&library)?;
    let stats = ctx.host.library_stats(&library.id)?;
    let current = ctx.host.library_info(&library.id)?;
    let remote = current.remote.as_ref().map(|remote| RemoteDoc {
        url: remote.url.clone(),
        server_name: remote.server_name.clone(),
        role: format!("{:?}", remote.role).to_lowercase(),
        offline: remote.offline,
        cache_bytes: stats.cache_bytes.unwrap_or(0),
        server_version: remote.server_version.clone(),
        min_client_version: remote.min_client_version.clone(),
        compat: remote.compat.clone(),
        compat_message: remote.compat_message.clone(),
    });
    let mut doc = LibraryInfoDoc {
        library: library_doc(&current, Some(stats.items)),
        items: stats.items,
        revision: stats.revision,
        types: stats.types.iter().map(|count| TypeCount { id: count.value.clone(), count: count.count }).collect(),
        files: stats.files,
        bytes: stats.bytes,
        sources: stats.source_files.zip(stats.source_bytes).zip(stats.source_saved_bytes).map(|((files, bytes), saved_bytes)| SourcesDoc {
            files,
            bytes,
            saved_bytes,
            migration: stats.migration.as_ref().map(|report| MigrationDoc { items: report.items, bytes_freed: report.bytes_freed, skipped: report.skipped.len() }),
        }),
        remote,
        pending_migrations: Vec::new(),
    };
    let mut text = format!("id        {}\nname      {}\nkind      {}\nlocation  {}\nstate     {}\nitems     {}\nrevision  {}\n", doc.library.id, doc.library.name, doc.library.kind, doc.library.location, doc.library.state, doc.items, doc.revision);
    if let (Some(files), Some(bytes)) = (doc.files, doc.bytes) {
        text.push_str(&format!("files     {files} ({})\n", human_bytes(bytes)));
    }
    if let Some(sources) = &doc.sources {
        if sources.files > 0 {
            text.push_str(&format!("sources   {} kept source files, {} (sharing saves {})\n", sources.files, human_bytes(sources.bytes), human_bytes(sources.saved_bytes)));
        }
        if let Some(migration) = &sources.migration {
            text.push_str(&format!("          converted to shared storage: {} items, {} freed{}\n", migration.items, human_bytes(migration.bytes_freed), if migration.skipped > 0 { format!(", {} could not be converted", migration.skipped) } else { String::new() }));
        }
    }
    if let Some(remote) = &doc.remote {
        text.push_str(&format!("server    {} ({})\nrole      {}\ncache     {}\n", remote.server_name, if remote.offline { "offline" } else { "online" }, remote.role, human_bytes(remote.cache_bytes)));
        if let Some(version) = &remote.server_version {
            text.push_str(&format!("version   server {version}, this tbx {}{}\n", env!("CARGO_PKG_VERSION"), remote.min_client_version.as_ref().map(|min| format!(" (the server serves Toybox {min} and newer)")).unwrap_or_default()));
        }
        if let Some(message) = &remote.compat_message {
            text.push_str(&format!("          {message}\n"));
        }
    }
    if let Some(line) = pending_line(&current) {
        text.push_str(&format!("format    {line}\n"));
        doc.pending_migrations = vec![line];
    }
    if !doc.types.is_empty() {
        text.push_str("types\n");
        for count in &doc.types {
            text.push_str(&format!("  {:<14}{}\n", count.id, count.count));
        }
    }
    ctx.output(&doc, &text)
}

fn add(ctx: &Ctx, matches: &ArgMatches, create: bool) -> CliResult<()> {
    let dir = PathBuf::from(absolute(matches.get_one::<String>("dir").map(String::as_str).unwrap_or(".")));
    let name = matches.get_one::<String>("name").map(String::as_str);
    let info = if create { ctx.host.create_library(&dir, name)? } else { ctx.host.add_library(&dir, name)? };
    if matches.get_flag("default") {
        ctx.host.set_default_library(&info.id)?;
    }
    let current = ctx.host.library_info(&info.id)?;
    let doc = LibraryChangeDoc { library: library_doc(&current, None) };
    let verb = if create { "Created" } else { "Added" };
    ctx.report(&doc, &format!("{verb} library '{}' ({})\n", current.name, current.id))
}

fn connect(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let url = matches.get_one::<String>("url").cloned().unwrap_or_default();
    let name = matches.get_one::<String>("name").map(String::as_str);
    let token = prompt::read_token(matches.get_flag("token-stdin"))?;
    let info = ctx.host.add_remote_library(&url, &token, name)?;
    let doc = LibraryChangeDoc { library: library_doc(&info, None) };
    ctx.report(&doc, &format!("Connected library '{}' ({}) at {}\n", info.name, info.id, doc.library.location))
}

fn login(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let library = ctx.library_or(named(matches))?;
    remote_only(&library)?;
    let token = prompt::read_token(matches.get_flag("token-stdin"))?;
    let info = ctx.host.change_remote_token(&library.id, &token)?;
    ctx.report(&LibraryChangeDoc { library: library_doc(&info, None) }, &format!("Changed the access token of '{}'\n", info.name))
}

fn rename(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let library = ctx.library_or(named(matches))?;
    let name = matches.get_one::<String>("name").cloned().unwrap_or_default();
    let info = ctx.host.rename_library(&library.id, &name)?;
    ctx.report(&LibraryChangeDoc { library: library_doc(&info, None) }, &format!("Renamed '{}' to '{}'\n", library.name, info.name))
}

fn set_default(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let library = ctx.library_or(named(matches))?;
    ctx.host.set_default_library(&library.id)?;
    let info = ctx.host.library_info(&library.id)?;
    ctx.report(&LibraryChangeDoc { library: library_doc(&info, None) }, &format!("'{}' is now the default library\n", info.name))
}

fn remove(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let library = ctx.library_or(named(matches))?;
    let doc = LibraryRemovedDoc { removed: library_doc(&library, None), dry_run: matches.get_flag("dry-run") };
    let consequence = if library.remote.is_some() { "its access token and downloaded files are deleted; nothing on the server changes" } else { "its files stay where they are" };
    if doc.dry_run {
        return ctx.report(&doc, &format!("Would remove library '{}' ({consequence})\n", library.name));
    }
    prompt::confirm(ctx, &format!("Remove library '{}' from Toybox? ({consequence})", library.name))?;
    ctx.host.remove_library(&library.id)?;
    ctx.report(&doc, &format!("Removed library '{}'\n", library.name))
}

fn scan(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let library = ctx.library_or(named(matches))?;
    if !library.available {
        return Err(CliError::failed(format!("library '{}' cannot be read", library.name)));
    }
    ctx.log(&format!("Scanning '{}'...", library.name));
    let summary = ctx.host.scan_library(&library.id)?;
    ctx.report(&ScanDoc { library: library.id.clone(), items: summary.count, revision: summary.revision }, &format!("'{}': {} items (revision {})\n", library.name, summary.count, summary.revision))
}

fn check(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let library = ctx.library_or(named(matches))?;
    if !library.available {
        return Err(CliError::failed(format!("library '{}' cannot be read", library.name)));
    }
    let report = ctx.host.check_library(&library.id, matches.get_flag("fix"))?;
    let doc = CheckDoc {
        library: library.id.clone(),
        ok: report.problems.is_empty(),
        items: report.items,
        files_checked: report.files_checked,
        problems: report.problems.iter().map(|problem| ProblemDoc { item: problem.item.clone(), message: problem.message.clone() }).collect(),
        orphan_sources: report.orphan_sources.clone(),
        removed_sources: report.removed_sources.clone(),
        pending_migrations: pending_line(&library).into_iter().collect(),
    };
    let mut text = String::new();
    for problem in &doc.problems {
        text.push_str(&format!("{}\n", problem.message));
    }
    for orphan in &doc.orphan_sources {
        text.push_str(&format!("unused source file {orphan} (tbx library check --fix removes it)\n"));
    }
    for removed in &doc.removed_sources {
        text.push_str(&format!("removed unused source file {removed}\n"));
    }
    for line in &doc.pending_migrations {
        text.push_str(&format!("{line}\n"));
    }
    if doc.ok {
        text.push_str(&format!("'{}' is fine: {} items, {} files checked\n", library.name, doc.items, doc.files_checked));
    }
    ctx.output(&doc, &text)?;
    if doc.ok {
        Ok(())
    } else {
        Err(CliError::partial(format!("{} found in '{}'", out::plural(doc.problems.len(), "problem", "problems"), library.name)))
    }
}

fn cache(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let (action, sub) = matches.subcommand().ok_or_else(|| CliError::usage("choose size or clear"))?;
    let library = ctx.library_or(named(sub))?;
    remote_only(&library)?;
    if action == "size" {
        let bytes = ctx.host.remote_cache_size(&library.id)?;
        return ctx.output(&CacheDoc { library: library.id.clone(), bytes }, &format!("{}\n", human_bytes(bytes)));
    }
    let freed = ctx.host.clear_remote_cache(&library.id)?;
    ctx.report(&CacheClearedDoc { library: library.id.clone(), freed_bytes: freed }, &format!("Freed {}\n", human_bytes(freed)))
}

fn role_text(role: RemoteRole) -> &'static str {
    match role {
        RemoteRole::Read => "read",
        RemoteRole::Write => "write",
        RemoteRole::Admin => "admin",
    }
}

fn token(ctx: &Ctx, matches: &ArgMatches) -> CliResult<()> {
    let (action, sub) = matches.subcommand().ok_or_else(|| CliError::usage("choose list, create or revoke"))?;
    let library = ctx.library_or(named(sub))?;
    remote_only(&library)?;
    match action {
        "list" => {
            let tokens = ctx.host.list_remote_tokens(&library.id)?;
            let mut table = Table::new(&["ID", "NAME", "ROLE"]);
            for token in &tokens {
                table.row(vec![token.id.clone(), token.name.clone(), role_text(token.role).to_string()]);
            }
            ctx.output(&TokensDoc { tokens }, &table.render())
        }
        "create" => {
            let name = sub.get_one::<String>("name").cloned().unwrap_or_default();
            let role = match sub.get_one::<String>("role").map(String::as_str) {
                Some("admin") => RemoteRole::Admin,
                Some("write") => RemoteRole::Write,
                _ => RemoteRole::Read,
            };
            let created = ctx.host.create_remote_token(&library.id, &name, role)?;
            if !ctx.g.json && !ctx.g.quiet {
                out::stderr(&format!("Token '{}' ({}); it is shown only now:\n", created.view.name, created.view.id));
            }
            ctx.output(&created, &format!("{}\n", created.token))
        }
        _ => {
            let id = sub.get_one::<String>("token_id").cloned().unwrap_or_default();
            ctx.host.revoke_remote_token(&library.id, &id)?;
            ctx.report(&serde_json::json!({ "revoked": id }), &format!("Revoked token {id}\n"))
        }
    }
}
