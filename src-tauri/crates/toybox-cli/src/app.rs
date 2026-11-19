//! Start-up and dispatch: find the data, open the host, parse the command line, run the
//! command and turn its outcome into an exit status.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use clap::error::ErrorKind;
use clap::{ArgMatches, ColorChoice, Command};
use toybox_host::secrets::FileStore;
use toybox_host::{Host, HostPaths};
use toybox_remote::secrets::{KeyringStore, SecretStore};

use crate::ctx::{Ctx, Globals, Origin};
use crate::error::{CliError, CliResult};
use crate::plugin_runtime::PluginCli;
use crate::{cli, cmd_edit, cmd_info, cmd_items, cmd_library, cmd_plugin, cmd_run, cmd_transfer, out};

const DEFAULT_APP_ID: &str = "com.toybox.app";

/// The few global options that are needed before the command line can be parsed: where the
/// data is (the command tree depends on the installed plugins) and how to print a usage error.
struct Prescan {
    home: Option<PathBuf>,
    app_id: String,
    json: bool,
    color: String,
}

fn prescan(args: &[OsString]) -> Prescan {
    let mut pre = Prescan {
        home: std::env::var_os("TOYBOX_HOME").filter(|value| !value.is_empty()).map(PathBuf::from),
        app_id: std::env::var("TOYBOX_APP_ID").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| DEFAULT_APP_ID.to_string()),
        json: false,
        color: "auto".to_string(),
    };
    let mut iter = args.iter().skip(1).map(|arg| arg.to_string_lossy().to_string());
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name.to_string(), Some(value.to_string())),
            _ => (arg.clone(), None),
        };
        match name.as_str() {
            "--json" => pre.json = true,
            "--home" => {
                if let Some(value) = inline.or_else(|| iter.next()) {
                    pre.home = Some(PathBuf::from(value));
                }
            }
            "--app-id" => {
                if let Some(value) = inline.or_else(|| iter.next()) {
                    pre.app_id = value;
                }
            }
            "--color" => {
                if let Some(value) = inline.or_else(|| iter.next()) {
                    pre.color = value;
                }
            }
            _ => {}
        }
    }
    pre
}

fn secret_store() -> (Arc<dyn SecretStore>, Option<PathBuf>) {
    // `TOYBOX_SECRETS=memory:<file>` keeps tokens in a plain file: for tests and automation only.
    match std::env::var("TOYBOX_SECRETS").ok().as_deref().and_then(|spec| spec.strip_prefix("memory:")) {
        Some(file) if !file.is_empty() => (Arc::new(FileStore::new(file)), Some(PathBuf::from(file))),
        _ => (Arc::new(KeyringStore), None),
    }
}

fn open_host(pre: &Prescan) -> CliResult<(Arc<Host>, Origin, Option<PathBuf>)> {
    let (paths, origin) = match &pre.home {
        Some(home) => {
            let home = std::path::absolute(home).map_err(|error| CliError::failed(format!("--home {}: {error}", home.display())))?;
            (HostPaths::for_home(&home), Origin::Home)
        }
        None => (HostPaths::for_app_id(&pre.app_id)?, Origin::AppId(pre.app_id.clone())),
    };
    let (secrets, secrets_file) = secret_store();
    let host = Host::open_lazy(paths, secrets, None)?;
    Ok((Arc::new(host), origin, secrets_file))
}

/// The plugins' command-line declarations. A manifest problem never takes `tbx` down: the
/// built-in commands still work, without the plugin's.
fn load_plugin_clis(host: &Host) -> Vec<PluginCli> {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::plugin_runtime::plugin_clis(host, cli::BUILTIN)));
    std::panic::set_hook(previous);
    match loaded {
        Ok(Ok(clis)) => clis,
        _ => Vec::new(),
    }
}

/// Run `tbx` and return its exit status.
pub fn run() -> i32 {
    let args: Vec<OsString> = std::env::args_os().collect();
    toybox_remote::client::set_product("tbx");
    // `tbx self ...` is about this program, not the libraries: it needs no data and runs from an installer.
    if args.get(1).is_some_and(|word| word == "self") {
        return crate::cmd_self::run(&args[2..]);
    }
    let pre = prescan(&args);
    let color = out::color_wanted(&pre.color);
    let (host, origin, secrets_file) = match open_host(&pre) {
        Ok(opened) => opened,
        Err(error) => {
            out::print_error(&error, pre.json, color);
            return error.kind.exit_status();
        }
    };
    let plugins = load_plugin_clis(&host);
    let choice = match pre.color.as_str() {
        "always" => ColorChoice::Always,
        "never" => ColorChoice::Never,
        _ => ColorChoice::Auto,
    };
    let command = cli::build(&plugins, choice);
    let matches = match command.clone().try_get_matches_from(&args) {
        Ok(matches) => matches,
        Err(error) => {
            return match error.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => error.exit(),
                _ => {
                    let rendered = error.render().to_string();
                    let first = rendered.lines().next().unwrap_or("invalid command line").trim_start_matches("error: ").to_string();
                    let usage = CliError::usage(first).hint("see `tbx --help`, or `tbx help <command>`");
                    out::print_error(&usage, pre.json, color);
                    usage.kind.exit_status()
                }
            };
        }
    };
    let globals = Globals::from_matches(&matches);
    let ctx = Ctx::new(globals, host.clone(), origin, secrets_file);
    let result = dispatch(&ctx, &command, &plugins, &matches);
    host.tools.stop_all();
    match result {
        Ok(()) => 0,
        Err(error) => {
            out::print_error(&error, ctx.g.json, ctx.g.color);
            error.kind.exit_status()
        }
    }
}

fn dispatch(ctx: &Ctx, root: &Command, plugins: &[PluginCli], matches: &ArgMatches) -> CliResult<()> {
    let Some((name, sub)) = matches.subcommand() else {
        return Err(CliError::usage("no command given").hint("see `tbx --help`"));
    };
    match name {
        "status" => cmd_info::status(ctx, plugins),
        "self" => {
            if sub.subcommand_name().is_none() {
                return Err(CliError::usage("choose install, uninstall or status").hint("see `tbx help self`"));
            }
            let mut words = vec![OsString::from(sub.subcommand_name().unwrap_or(""))];
            if ctx.g.json {
                words.push(OsString::from("--json"));
            }
            match crate::cmd_self::run(&words) {
                0 => Ok(()),
                _ => Err(CliError::failed("tbx self failed")),
            }
        }
        "help" => cmd_info::help(ctx, root, plugins, sub),
        "completions" => cmd_info::completions(root, sub),
        "library" => cmd_library::run(ctx, sub),
        "search" => cmd_items::search(ctx, sub),
        "count" => cmd_items::count(ctx, sub),
        "show" => cmd_items::show(ctx, sub),
        "path" => cmd_items::path(ctx, sub),
        "types" => cmd_items::types(ctx, sub),
        "facets" => cmd_items::facets(ctx, sub),
        "tag" => cmd_edit::tag(ctx, sub),
        "edit" => cmd_edit::edit(ctx, sub),
        "favorite" => cmd_edit::favorite(ctx, sub),
        "collection" => cmd_edit::collection(ctx, sub),
        "import" => cmd_run::import(ctx, plugins, sub),
        "export" => cmd_run::export(ctx, plugins, sub),
        "copy" => cmd_transfer::transfer(ctx, sub, false),
        "move" => cmd_transfer::transfer(ctx, sub, true),
        "delete" => cmd_transfer::delete(ctx, sub),
        "thumbs" => cmd_transfer::thumbs(ctx, sub),
        "plugin" => cmd_plugin::plugin(ctx, plugins, sub),
        "config" => cmd_plugin::config(ctx, sub),
        "debug" => cmd_plugin::debug(ctx, sub),
        other => cmd_run::plugin_command(ctx, plugins, other, sub),
    }
}
