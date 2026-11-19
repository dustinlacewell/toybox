//! The command tree: every built-in command with its options, and the commands plugins
//! declare in their manifests (built from `plugin_runtime::plugin_clis`, so help,
//! validation and completion need no plugin code).

use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command, ValueHint};
use serde_json::{Map, Value};

use crate::plugin_runtime::{CliCommand, CliOption, CliValueType, PluginCli};
use crate::selection::{add_filter_args, add_selection_args, add_selection_args_without, Ids};

/// Names of the built-in top-level commands (a plugin command may not take one).
pub const BUILTIN: &[&str] = &[
    "status", "help", "completions", "library", "search", "count", "show", "path", "types", "facets", "tag", "edit", "favorite",
    "collection", "import", "export", "copy", "move", "delete", "thumbs", "plugin", "config", "debug",
];

/// Long option names the global options use; a plugin option may not take them.
const GLOBAL_LONGS: &[&str] = &["library", "json", "quiet", "yes", "verbose", "home", "app-id", "color", "help", "version"];

fn leak(text: impl Into<String>) -> &'static str {
    Box::leak(text.into().into_boxed_str())
}

/// Accepted for old scripts and doing nothing: reads span every library unless `-L` names one.
fn hidden_all_libraries() -> Arg {
    Arg::new("all-libraries").long("all-libraries").action(ArgAction::SetTrue).hide(true).help("Does nothing (reads cover every library unless -L is given)")
}

fn dry_run() -> Arg {
    Arg::new("dry-run").long("dry-run").action(ArgAction::SetTrue).help("Print what would change and change nothing")
}

fn library_arg(help: &'static str) -> Arg {
    Arg::new("library_arg").value_name("LIBRARY").help(help)
}

fn token_stdin() -> Arg {
    Arg::new("token-stdin").long("token-stdin").action(ArgAction::SetTrue).help("Read the access token from standard input (else TOYBOX_TOKEN, else a prompt)")
}

fn tags_arg() -> Arg {
    Arg::new("tags").value_name("TAGS").required(true).allow_hyphen_values(false).help("Comma-separated tags")
}

/// The whole tree, with the plugins' commands mounted.
pub fn build(plugins: &[PluginCli], color: clap::ColorChoice) -> Command {
    let mut root = Command::new("tbx")
        .bin_name("tbx")
        .about("Toybox on the command line: libraries, items, tags, import, export and plugins")
        .long_about("Toybox on the command line. Everything the app does with libraries, items, tags, import, export and plugins,\nwithout the app and without Node.")
        .version(env!("CARGO_PKG_VERSION"))
        .color(color)
        .disable_help_subcommand(true)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .help_expected(true)
        .subcommand_help_heading("Commands")
        .after_help("Examples:\n  tbx status\n  tbx library create D:\\assets --name Main\n  tbx search crate --type model --limit 10\n  tbx search crate --type model --ids | tbx tag add props,wood -\n  tbx export copy --type model --tag props --to D:\\out\n\nRun `tbx help --all` for every command, including the ones plugins add.")
        .arg(Arg::new("library").short('L').long("library").value_name("NAME|ID").global(true).help_heading("Global options").env("TOYBOX_LIBRARY").help("The library to act on (default: the default library)"))
        .arg(Arg::new("json").long("json").global(true).help_heading("Global options").action(ArgAction::SetTrue).help("Print one JSON document on standard output in place of text"))
        .arg(Arg::new("quiet").short('q').long("quiet").global(true).help_heading("Global options").action(ArgAction::SetTrue).help("Print nothing but errors (and the result under --json)"))
        .arg(Arg::new("yes").short('y').long("yes").global(true).help_heading("Global options").action(ArgAction::SetTrue).help("Do not ask before a destructive change"))
        .arg(Arg::new("verbose").short('v').long("verbose").global(true).help_heading("Global options").action(ArgAction::SetTrue).help("Show what plugin code writes to its console"))
        .arg(Arg::new("home").long("home").value_name("DIR").global(true).help_heading("Global options").env("TOYBOX_HOME").value_hint(ValueHint::DirPath).help("Keep everything per user (settings, plugins, caches) in this folder"))
        .arg(Arg::new("app-id").long("app-id").value_name("ID").global(true).help_heading("Global options").env("TOYBOX_APP_ID").default_value("com.toybox.app").help("Use the data of the app with this identifier (a development build uses com.toybox.dev)"))
        .arg(Arg::new("color").long("color").value_name("WHEN").global(true).help_heading("Global options").value_parser(PossibleValuesParser::new(["auto", "always", "never"])).default_value("auto").help("Colour on the terminal"));
    for command in [
        status(),
        help(),
        completions(),
        library(),
        search(),
        count(),
        show(),
        path(),
        types(),
        facets(),
        tag(),
        edit(),
        favorite(),
        collection(),
        import(plugins),
        export(plugins),
        copy_move("copy", "Copy items to another library, local or remote"),
        copy_move("move", "Copy items to another library, then remove the ones that arrived from this one"),
        delete(),
        thumbs(),
        plugin(),
        config(),
        debug(),
    ] {
        root = root.subcommand(command);
    }
    for plugin in plugins {
        for declared in &plugin.commands {
            root = root.subcommand(plugin_command(declared, plugin));
        }
    }
    root
}

fn status() -> Command {
    Command::new("status").about("Where the data is, the libraries, the plugins that are on, and anything wrong")
        .after_help("Exit status is 0 even when something is wrong; the problems are listed.")
}

fn help() -> Command {
    Command::new("help")
        .about("Help for a command (--all: the whole command tree)")
        .arg(Arg::new("command").value_name("COMMAND").num_args(0..).help("The command, e.g. `library add`"))
        .arg(Arg::new("all").long("all").action(ArgAction::SetTrue).help("Print every command, including the ones plugins add"))
}

fn completions() -> Command {
    Command::new("completions")
        .about("Print a completion script for a shell")
        .arg(Arg::new("shell").value_name("SHELL").required(true).value_parser(PossibleValuesParser::new(["bash", "zsh", "fish", "powershell"])).help("The shell"))
        .after_help("Examples:\n  tbx completions bash > ~/.local/share/bash-completion/completions/tbx\n  tbx completions powershell | Out-String | Invoke-Expression")
}

fn library() -> Command {
    Command::new("library")
        .about("Manage the registered libraries")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("list").about("The registered libraries: id, name, kind, location, default, state"))
        .subcommand(Command::new("info").about("One library: counts by type, revision, size of its files; for a remote one the server and cache").arg(library_arg("The library (default: the one chosen with -L, else the default)")))
        .subcommand(
            Command::new("create")
                .about("Make a new library folder and register it")
                .arg(Arg::new("dir").value_name("DIR").required(true).value_hint(ValueHint::DirPath).help("The folder (made if needed; it must be empty)"))
                .arg(Arg::new("name").long("name").value_name("NAME").help("Display name (default: the folder name)"))
                .arg(Arg::new("default").long("default").action(ArgAction::SetTrue).help("Make it the default library"))
                .after_help("Example:\n  tbx library create D:\\assets --name Main --default"),
        )
        .subcommand(
            Command::new("add")
                .about("Register an existing library folder")
                .arg(Arg::new("dir").value_name("DIR").required(true).value_hint(ValueHint::DirPath).help("The library folder"))
                .arg(Arg::new("name").long("name").value_name("NAME").help("Display name (default: the folder name)"))
                .arg(Arg::new("default").long("default").action(ArgAction::SetTrue).help("Make it the default library")),
        )
        .subcommand(
            Command::new("connect")
                .about("Register a library served by toybox-server")
                .arg(Arg::new("url").value_name("URL").required(true).help("The server address, e.g. https://toybox.example.com"))
                .arg(Arg::new("name").long("name").value_name("NAME").help("Display name (default: the server's name)"))
                .arg(token_stdin())
                .after_help("The access token is never an argument: set TOYBOX_TOKEN, use --token-stdin, or type it at the prompt.\n\nExample:\n  TOYBOX_TOKEN=... tbx library connect https://toybox.example.com --name Studio"),
        )
        .subcommand(
            Command::new("login")
                .about("Replace the access token of a remote library")
                .arg(library_arg("The remote library").required(true))
                .arg(token_stdin()),
        )
        .subcommand(Command::new("rename").about("Change a library's display name")
            .arg(library_arg("The library").required(true))
            .arg(Arg::new("name").value_name("NAME").required(true).help("The new name")))
        .subcommand(Command::new("default").about("Make a library the default one").arg(library_arg("The library").required(true)))
        .subcommand(Command::new("remove").about("Forget a library (its files stay where they are)").arg(library_arg("The library").required(true)).arg(dry_run()))
        .subcommand(Command::new("scan").about("Local: rebuild the index from the manifest and the files. Remote: fetch what changed").arg(library_arg("The library (default: -L, else the default)")))
        .subcommand(
            Command::new("check")
                .about("Verify the database and that every item's files exist (exit 4 when something is wrong)")
                .arg(library_arg("The library (default: -L, else the default)"))
                .arg(Arg::new("fix").long("fix").action(ArgAction::SetTrue).help("Remove stored source files no item uses (the only thing it changes)")),
        )
        .subcommand(
            Command::new("cache")
                .about("The downloaded files of a remote library")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(Command::new("size").about("Bytes of downloaded files").arg(library_arg("The remote library (default: -L)")))
                .subcommand(Command::new("clear").about("Delete the downloaded files").arg(library_arg("The remote library (default: -L)"))),
        )
        .subcommand(
            Command::new("token")
                .about("Access tokens of a remote library (needs an admin token)")
                .subcommand_required(true)
                .arg_required_else_help(true)
                .subcommand(Command::new("list").about("The server's access tokens").arg(library_arg("The remote library (default: -L)")))
                .subcommand(
                    Command::new("create")
                        .about("Make a token; it is printed once")
                        .arg(Arg::new("name").long("name").value_name("NAME").required(true).help("A name for the token"))
                        .arg(Arg::new("role").long("role").value_name("ROLE").required(true).value_parser(PossibleValuesParser::new(["read", "write", "admin"])).help("What the token may do"))
                        .arg(library_arg("The remote library (default: -L)")),
                )
                .subcommand(
                    Command::new("revoke")
                        .about("Revoke a token at once")
                        .arg(Arg::new("token_id").value_name("TOKEN_ID").required(true).help("The token's id, as `token list` prints it"))
                        .arg(library_arg("The remote library (default: -L)")),
                ),
        )
}

fn search_args(command: Command) -> Command {
    let command = command.arg(Arg::new("text").value_name("TEXT").help("Part of the name or of a tag"));
    add_filter_args(command, false)
        .arg(hidden_all_libraries())
        .arg(Arg::new("include-hidden").long("include-hidden").action(ArgAction::SetTrue).help("Include items the app hides because their plugin is off"))
}

fn search() -> Command {
    search_args(Command::new("search").about("List matching items (default limit 50)"))
        .arg(Arg::new("sort").long("sort").value_name("FIELD").value_parser(PossibleValuesParser::new(["name"])).default_value("name").help("Sort field"))
        .arg(Arg::new("desc").long("desc").action(ArgAction::SetTrue).help("Sort in descending order"))
        .arg(Arg::new("limit").long("limit").value_name("N").value_parser(clap::value_parser!(i64).range(0..)).default_value("50").help("Most items to list"))
        .arg(Arg::new("offset").long("offset").value_name("N").value_parser(clap::value_parser!(i64).range(0..)).default_value("0").help("Items to skip"))
        .arg(Arg::new("ids").long("ids").action(ArgAction::SetTrue).help("Print only ids, one per line, for piping"))
        .after_help("Without --type every type is searched.\n\nExamples:\n  tbx search crate --type model\n  tbx search --type audio --tag ui --limit 20 --json\n  tbx search --type model --favorites --ids | tbx export copy - --to out")
}

fn count() -> Command {
    search_args(Command::new("count").about("How many items match"))
}

fn show() -> Command {
    add_selection_args(Command::new("show").about("Everything about items: names, type, tags, details, files, variants, origin"), Ids::Positional)
}

fn path() -> Command {
    add_selection_args(Command::new("path").about("The absolute path of each item's main file (files of a remote library are downloaded first)"), Ids::Positional)
        .arg(Arg::new("all").long("all").action(ArgAction::SetTrue).help("Print every file of each item"))
}

fn types() -> Command {
    Command::new("types").about("The asset types with their counts and the plugin that declares each")
        .arg(hidden_all_libraries())
}

fn facets() -> Command {
    Command::new("facets")
        .about("The values that can be filtered on, with counts")
        .arg(Arg::new("type").long("type").value_name("TYPE").required(true).help("The asset type"))
        .arg(Arg::new("tags").long("tags").value_name("N").value_parser(clap::value_parser!(usize)).default_value("20").help("Most tags to list"))
        .arg(hidden_all_libraries())
}

fn tag() -> Command {
    let change = |name: &'static str, about: &'static str| add_selection_args(Command::new(name).about(about).arg(tags_arg()), Ids::Positional).arg(dry_run());
    Command::new("tag")
        .about("Library tags (shared with everyone who uses the library)")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("list")
                .about("Tags in use, with counts")
                .arg(Arg::new("type").long("type").value_name("TYPE").help("Only this asset type"))
                .arg(Arg::new("search").long("search").value_name("TEXT").help("Only tags containing this text"))
                .arg(Arg::new("limit").long("limit").value_name("N").value_parser(clap::value_parser!(usize)).default_value("50").help("Most tags to list")),
        )
        .subcommand(change("add", "Add tags to items").after_help("Example:\n  tbx tag add wood,prop --type model --text crate"))
        .subcommand(change("remove", "Remove tags from items"))
        .subcommand(change("set", "Replace the tags of items (an empty string removes them all)"))
}

fn edit() -> Command {
    add_selection_args_without(Command::new("edit").about("Change the details of items; an empty value clears the field"), Ids::Positional, &["source", "publisher", "license"])
        .arg(Arg::new("set-source").long("source").value_name("TEXT").help("Where the item came from"))
        .arg(Arg::new("set-publisher").long("publisher").value_name("TEXT").help("Who published it"))
        .arg(Arg::new("set-license").long("license").value_name("TEXT").help("Its license"))
        .arg(Arg::new("set-url").long("url").value_name("URL").help("A web address (http:// or https://)"))
        .arg(dry_run())
}

fn favorite() -> Command {
    let change = |name: &'static str, about: &'static str| add_selection_args(Command::new(name).about(about), Ids::Positional).arg(dry_run());
    Command::new("favorite")
        .about("Favorites (personal to this computer)")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(change("add", "Mark items as favorites"))
        .subcommand(change("remove", "Unmark items"))
}

fn collection() -> Command {
    let change = |name: &'static str, about: &'static str| {
        add_selection_args(Command::new(name).about(about).arg(Arg::new("name").value_name("NAME").required(true).help("The collection")), Ids::Positional).arg(dry_run())
    };
    Command::new("collection")
        .about("Collections (personal to this computer; one exists while it has items)")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("list").about("Collections with their sizes").arg(Arg::new("type").long("type").value_name("TYPE").help("Only this asset type")))
        .subcommand(change("add", "Put items into a collection"))
        .subcommand(change("remove", "Take items out of a collection"))
}

fn shared_import_options(command: Command) -> Command {
    command
        .arg(Arg::new("shared-tag").long("tag").value_name("TAGS").help("Comma-separated tags for everything imported"))
        .arg(Arg::new("shared-source").long("source").value_name("TEXT").help("Source for everything imported"))
        .arg(Arg::new("shared-publisher").long("publisher").value_name("TEXT").help("Publisher for everything imported"))
        .arg(Arg::new("shared-license").long("license").value_name("TEXT").help("License for everything imported"))
        .arg(Arg::new("shared-url").long("url").value_name("URL").help("Web address for everything imported"))
        .arg(dry_run())
}

fn import(plugins: &[PluginCli]) -> Command {
    let mut command = Command::new("import")
        .about("Bring files into the library with an importer (no importer: list them)")
        .subcommand_help_heading("Importers")
        .after_help("Examples:\n  tbx import\n  tbx import audio D:\\sounds --tag sfx --source \"Sound pack\"");
    for plugin in plugins {
        for importer in &plugin.importers {
            let mut sub = Command::new(leak(&importer.name))
                .about(leak(importer.summary.as_str()))
                .arg(Arg::new("paths").value_name("PATH").num_args(1..).required(true).value_hint(ValueHint::AnyPath).help("Files or folders to import"));
            sub = add_plugin_options(sub, &importer.options, &["tag", "source", "publisher", "license", "url", "dry-run"]);
            command = command.subcommand(shared_import_options(sub));
        }
    }
    command
}

fn export_args(command: Command) -> Command {
    add_selection_args(command, Ids::Positional)
        .arg(Arg::new("to").long("to").value_name("DIR").required(true).value_hint(ValueHint::DirPath).help("The folder to write into"))
        .arg(Arg::new("preserve-structure").long("preserve-structure").action(ArgAction::SetTrue).help("Keep the pack and category folders instead of flattening"))
}

fn export(plugins: &[PluginCli]) -> Command {
    let mut command = Command::new("export")
        .about("Send items out of the library (no exporter: list them)")
        .subcommand_help_heading("Exporters")
        .subcommand(export_args(Command::new("copy").about("Copy the items' files out as loose glTF filesets")).arg(dry_run()))
        .subcommand(export_args(Command::new("glb").about("Write each model as one self-contained .glb")).arg(dry_run()))
        .after_help("Examples:\n  tbx export copy --type model --tag props --to D:\\out\n  tbx export glb <ID> --to D:\\out --preserve-structure");
    for plugin in plugins {
        let Some(exporter) = &plugin.exporter else { continue };
        let mut sub = export_args(Command::new(leak(&exporter.name)).about(leak(exporter.summary.as_str())));
        sub = add_plugin_options(sub, &exporter.options, &["to", "preserve-structure", "variants", "preview", "type", "tag", "source", "publisher", "license", "format", "category", "collection", "favorites", "text", "id"]);
        sub = sub
            .arg(Arg::new("variants").long("variants").value_name("POLICY").value_parser(PossibleValuesParser::new(["all", "best"])).default_value("all").help("Export every variant or only the best one of each item"))
            .arg(Arg::new("preview").long("preview").action(ArgAction::SetTrue).help("Only plan: print the paths that would be written"));
        command = command.subcommand(sub);
    }
    command
}

fn copy_move(name: &'static str, about: &'static str) -> Command {
    add_selection_args(Command::new(name).about(about), Ids::Positional)
        .arg(Arg::new("to").long("to").value_name("LIBRARY").required(true).help("The target library"))
        .arg(Arg::new("on-conflict").long("on-conflict").value_name("WHAT").value_parser(PossibleValuesParser::new(["skip", "replace"])).default_value("skip").help("An item already in the target is left alone (skip) or replaced"))
        .arg(Arg::new("with-clips").long("with-clips").action(ArgAction::SetTrue).help("Also take the animation clips cut from the selected models"))
        .arg(dry_run())
}

fn delete() -> Command {
    add_selection_args(Command::new("delete").about("Delete items and their files from the library"), Ids::Positional).arg(dry_run())
        .after_help("Asks first on a terminal; without one it needs --yes.\n\nExample:\n  tbx delete --type audio --tag temp --dry-run")
}

fn thumbs() -> Command {
    Command::new("thumbs")
        .about("Thumbnails (the app draws them when it opens the library)")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("status").about("How many items still wait for a thumbnail"))
        .subcommand(add_selection_args(Command::new("clear").about("Discard thumbnails so the app draws them again (no selection: all)"), Ids::Positional).arg(dry_run()))
}

fn plugin() -> Command {
    let id = |help: &'static str| Arg::new("id").value_name("ID").required(true).help(help);
    Command::new("plugin")
        .about("Installed plugins")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("list").about("Installed plugins: id, version, on/off/blocked, what each provides"))
        .subcommand(Command::new("info").about("One plugin: dependencies, settings, and the commands it adds").arg(id("The plugin id")))
        .subcommand(Command::new("enable").about("Turn a plugin on, with what it needs").arg(id("The plugin id")))
        .subcommand(Command::new("disable").about("Turn a plugin off (refused while something needs it)").arg(id("The plugin id")))
        .subcommand(
            Command::new("install")
                .about("Install a plugin folder")
                .arg(Arg::new("dir").value_name("DIR").required(true).value_hint(ValueHint::DirPath).help("The plugin folder (it holds manifest.json)"))
                .arg(Arg::new("link").long("link").action(ArgAction::SetTrue).help("Link the folder instead of copying it (for development)")),
        )
        .subcommand(Command::new("remove").about("Remove an installed plugin").arg(id("The plugin id")))
        .subcommand(
            Command::new("settings")
                .about("Show, read or set a plugin's declared settings")
                .arg(id("The plugin id"))
                .arg(Arg::new("key").value_name("KEY").help("The setting"))
                .arg(Arg::new("value").value_name("VALUE").requires("key").help("The new value"))
                .arg(Arg::new("unset").long("unset").action(ArgAction::SetTrue).requires("key").conflicts_with("value").help("Return the setting to its default")),
        )
}

fn config() -> Command {
    let key = || Arg::new("key").value_name("KEY").required(true).help("The setting (fbx2gltf-path)");
    Command::new("config")
        .about("Settings that are not a plugin's")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("list").about("Every setting and its value"))
        .subcommand(Command::new("get").about("Print one setting").arg(key()))
        .subcommand(Command::new("set").about("Change a setting").arg(key()).arg(Arg::new("value").value_name("VALUE").required(true).help("The new value")))
        .subcommand(Command::new("unset").about("Clear a setting").arg(key()))
}

fn debug() -> Command {
    Command::new("debug")
        .about("Look inside")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("paths").about("Every folder and file tbx reads or writes"))
        .subcommand(
            Command::new("sql")
                .about("Run one read-only SQL query on a library's database")
                .arg(Arg::new("query").value_name("QUERY").required(true).help("One SELECT statement"))
                .arg(Arg::new("limit").long("limit").value_name("N").value_parser(clap::value_parser!(usize)).default_value("1000").help("Most rows to print (0: all)"))
                .after_help("The schema is in docs/sqlite-storage.md and is not a stable interface.\n\nExample:\n  tbx debug sql \"select kind, count(*) from assets group by kind\""),
        )
}

// ---- plugin commands -----------------------------------------------------------------

/// The `--name` / positional arguments a plugin declared, skipping the ones whose name a
/// built-in option already uses.
pub fn add_plugin_options(mut command: Command, options: &[CliOption], reserved: &[&str]) -> Command {
    for option in options {
        if reserved.contains(&option.name.as_str()) || GLOBAL_LONGS.contains(&option.name.as_str()) {
            continue;
        }
        command = command.arg(plugin_arg(option, false));
    }
    command
}

fn plugin_arg(option: &CliOption, positional: bool) -> Arg {
    let id = leak(format!("opt:{}", option.key));
    let help = if option.help.is_empty() { leak(option.name.clone()) } else { leak(option.help.clone()) };
    let mut arg = Arg::new(id).help(help);
    if !positional {
        arg = arg.long(leak(option.name.clone()));
    } else {
        arg = arg.value_name(leak(option.name.to_uppercase()));
    }
    match option.value_type {
        CliValueType::Flag => return arg.action(ArgAction::SetTrue),
        CliValueType::Number => arg = arg.value_parser(clap::value_parser!(f64)).value_name("NUMBER"),
        CliValueType::Choice => arg = arg.value_parser(PossibleValuesParser::new(option.choices.iter().map(|choice| leak(choice.clone())).collect::<Vec<&'static str>>())),
        CliValueType::Path => arg = arg.value_hint(ValueHint::AnyPath).value_name("PATH"),
        CliValueType::String => {}
    }
    if !positional && option.value_type != CliValueType::Number && option.value_type != CliValueType::Path {
        arg = arg.value_name(leak(option.name.to_uppercase()));
    }
    if option.multiple {
        arg = if positional { arg.num_args(1..) } else { arg.action(ArgAction::Append) };
    }
    if option.required {
        arg = arg.required(true);
    }
    if let Some(default) = &option.default {
        let text = match default {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        arg = arg.default_value(leak(text));
    }
    arg
}

fn plugin_command(declared: &CliCommand, plugin: &PluginCli) -> Command {
    let mut command = Command::new(leak(&declared.name)).about(leak(declared.summary.as_str()));
    if !declared.commands.is_empty() {
        command = command.subcommand_required(true).arg_required_else_help(true);
        for child in &declared.commands {
            command = command.subcommand(plugin_command(child, plugin));
        }
        return command;
    }
    let mut seen_optional = false;
    for arg in &declared.args {
        // A required positional cannot follow an optional one, nor anything follow one that takes the rest.
        if arg.required && seen_optional {
            continue;
        }
        seen_optional |= !arg.required;
        let index = command.get_positionals().count() + 1;
        command = command.arg(plugin_arg(arg, true).index(index));
        if arg.multiple {
            break;
        }
    }
    let selection_longs: &[&str] = if declared.selection { &["id", "text", "type", "tag", "source", "publisher", "license", "format", "category", "collection", "favorites"] } else { &[] };
    command = add_plugin_options(command, &declared.options, selection_longs);
    if declared.selection {
        let ids = if declared.args.is_empty() { Ids::Positional } else { Ids::Flag };
        command = add_selection_args(command, ids);
    }
    command
}

// ---- reading the parsed values -------------------------------------------------------

/// The values of a plugin's options, keyed by `key`; a relative path is made absolute.
pub fn plugin_values(matches: &ArgMatches, options: &[CliOption]) -> Map<String, Value> {
    let mut out = Map::new();
    for option in options {
        let id = format!("opt:{}", option.key);
        if !matches.try_contains_id(&id).unwrap_or(false) {
            continue;
        }
        let value = match option.value_type {
            CliValueType::Flag => Value::Bool(matches.try_get_one::<bool>(&id).ok().flatten().copied().unwrap_or(false)),
            CliValueType::Number => {
                let numbers: Vec<Value> = matches.try_get_many::<f64>(&id).ok().flatten().map(|values| values.map(|n| number(*n)).collect()).unwrap_or_default();
                if option.multiple { Value::Array(numbers) } else if let Some(first) = numbers.into_iter().next() { first } else { continue }
            }
            _ => {
                let texts: Vec<String> = matches.try_get_many::<String>(&id).ok().flatten().map(|values| values.cloned().collect()).unwrap_or_default();
                let texts: Vec<Value> = texts.into_iter().map(|text| Value::String(if option.value_type == CliValueType::Path { absolute(&text) } else { text })).collect();
                if option.multiple { Value::Array(texts) } else if let Some(first) = texts.into_iter().next() { first } else { continue }
            }
        };
        out.insert(option.key.clone(), value);
    }
    out
}

fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() < 9.0e15 {
        Value::from(value as i64)
    } else {
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

/// `text` made absolute against the current folder (nothing is touched on disk).
pub fn absolute(text: &str) -> String {
    std::path::absolute(text).map(|path| path.to_string_lossy().to_string()).unwrap_or_else(|_| text.to_string())
}
