//! Runs the command-line side of plugins: loads the `cli.module` of every active
//! plugin in the embedded engine (`toybox-script`) and binds `host`, `fs` and
//! `services` to `toybox-host`. The rest of `tbx` only uses what this file exports.
//! See docs/cli.md, "Plugins and the command line".
//!
//! The JavaScript half is `bootstrap.js`; the Rust functions it calls are in `bridge.rs`.
// Not every item is used by the rest of tbx yet.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde_json::{Map, Value};
use toybox_core::error::{AppError, AppResult};
use toybox_host::Host;
use toybox_script::{Config, Engine, Handle, LogLevel};

mod bridge;
pub mod spec;

#[allow(unused_imports)]
pub use spec::{CliArg, CliCommand, CliExporter, CliImporter, CliOption, CliValueType, PluginCli};
#[allow(unused_imports)]
pub use spec::plugin_clis;
use spec::{active_manifests, clis_of, kinds_of, ActiveManifest};

include!(concat!(env!("OUT_DIR"), "/sdk_files.rs"));

const BOOTSTRAP: &str = include_str!("bootstrap.js");

/// Something a plugin said while running.
pub enum Event {
    /// `run.print(text)`: the command's own output (standard output).
    Print(String),
    /// `run.log(message)`: progress (standard error).
    Log(String),
    /// `run.warn(message)` (standard error).
    Warn(String),
    /// `console.*` from plugin code (standard error; shown with `--verbose`).
    Console(String),
}

/// Tags and details given with the shared options of `tbx import`; applied to
/// every committed entry that does not set its own.
#[derive(Default, Clone)]
pub struct ImportDefaults {
    pub tags: Vec<String>,
    pub source: Option<String>,
    pub publisher: Option<String>,
    pub license: Option<String>,
    pub url: Option<String>,
}

pub struct ImportRequest {
    pub library: String,
    /// Absolute files and folders.
    pub paths: Vec<String>,
    /// Keyed by the camelCase of each option's name.
    pub options: Map<String, Value>,
    pub defaults: ImportDefaults,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportOutcome {
    /// Ids of the items that were committed, in order (as stored in the library: not qualified).
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct ExportRequest {
    pub library: String,
    pub asset_ids: Vec<String>,
    pub target_dir: String,
    pub preserve_structure: bool,
    /// Keyed by the camelCase of each option's name (or the `fields` key).
    pub options: Map<String, Value>,
    /// Only plan: return the paths `preview` reports and write nothing.
    pub preview_only: bool,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ExportOutcome {
    pub written: Vec<String>,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct CommandRequest {
    pub library: String,
    /// The command's names joined with spaces, as in `cli.commands` of the module.
    pub path: String,
    pub args: Map<String, Value>,
    pub options: Map<String, Value>,
    /// The selection, when the command declares `"selection": true`.
    pub asset_ids: Option<Vec<String>>,
}

pub struct CommandOutcome {
    /// Whether the command called `print` at least once.
    pub printed: bool,
    /// What the handler returned (`Null` for nothing).
    pub result: Value,
}

/// The loaded plugins. Created once per `tbx` run, and only by commands that run plugin code.
/// Not `Send`: the engine lives on the thread that made it.
pub struct PluginRuntime {
    // The engine must go before anything it may still reference.
    engine: Engine,
    host: Arc<Host>,
    namespace: Handle,
    problems: Vec<(String, String)>,
    clis: Vec<PluginCli>,
    manifests: HashMap<String, Value>,
    cancel: Arc<AtomicBool>,
}

fn script_error(e: toybox_script::Error) -> AppError {
    if e.message.is_empty() {
        AppError::msg(if e.name.is_empty() { "the plugin failed".to_string() } else { e.name })
    } else {
        AppError::msg(e.message)
    }
}

fn to_script(v: Value) -> toybox_script::Value {
    toybox_script::Value::from(v)
}

fn option_specs(options: &[CliOption]) -> toybox_script::Value {
    let type_name = |t: CliValueType| match t {
        CliValueType::String => "string",
        CliValueType::Number => "number",
        CliValueType::Flag => "flag",
        CliValueType::Choice => "choice",
        CliValueType::Path => "path",
    };
    toybox_script::Value::Array(
        options
            .iter()
            .map(|o| {
                toybox_script::Value::object([
                    ("key", toybox_script::Value::String(o.key.clone())),
                    ("type", toybox_script::Value::String(type_name(o.value_type).into())),
                    ("default", o.default.clone().map(to_script).unwrap_or(toybox_script::Value::Null)),
                ])
            })
            .collect(),
    )
}

/// Dependencies first; plugins that do not depend on each other by id.
fn dependency_order(plugins: &[&ActiveManifest]) -> Vec<String> {
    let ids: BTreeMap<&str, &ActiveManifest> = plugins.iter().map(|p| (p.id.as_str(), *p)).collect();
    let mut done: Vec<String> = Vec::new();
    fn visit(id: &str, ids: &BTreeMap<&str, &ActiveManifest>, done: &mut Vec<String>, stack: &mut Vec<String>) {
        if done.iter().any(|d| d == id) || stack.iter().any(|s| s == id) {
            return;
        }
        let Some(plugin) = ids.get(id) else { return };
        stack.push(id.to_string());
        let mut deps = plugin.dependencies.clone();
        deps.sort();
        for dep in deps {
            visit(&dep, ids, done, stack);
        }
        stack.pop();
        done.push(id.to_string());
    }
    for id in ids.keys() {
        visit(id, &ids, &mut done, &mut Vec::new());
    }
    done
}

impl PluginRuntime {
    /// Load the `cli.module` of every active plugin that has one. A module that
    /// fails to load is recorded in `problems` and the others still load.
    pub fn load(host: Arc<Host>, events: Box<dyn Fn(Event)>) -> AppResult<PluginRuntime> {
        let active = active_manifests(&host)?;
        let clis = clis_of(&active, &[]);
        let events: Rc<dyn Fn(Event)> = Rc::from(events);
        let cancel = Arc::new(AtomicBool::new(false));

        let sink = events.clone();
        let engine = Engine::new(Config {
            console: Some(Box::new(move |level, text| {
                sink(Event::Console(match level {
                    LogLevel::Warn => format!("warn: {text}"),
                    LogLevel::Error => format!("error: {text}"),
                    _ => text.to_string(),
                }))
            })),
            import_hint: "Plugins run on the command line without a window: they can import their own files and @ldlework/toybox-sdk only.".into(),
            ..Config::default()
        })
        .map_err(script_error)?;
        engine.register_folder("toybox-sdk", SDK_FILES.iter().copied());
        engine.map_bare_module("@ldlework/toybox-sdk", "toybox-sdk/index.js");
        bridge::register(&engine, &host, &events, &cancel);

        // The plugins that have a command-line module, dependencies first.
        let loadable: Vec<&ActiveManifest> = active.iter().filter(|p| clis.iter().any(|c| c.plugin_id == p.id && !c.module.is_empty())).collect();
        let plugins_dir = host.paths().plugins_dir();
        let mut specs = Vec::new();
        let mut problems: Vec<(String, String)> = Vec::new();
        for id in dependency_order(&loadable) {
            let plugin = loadable.iter().find(|p| p.id == id).expect("ordered from the same list");
            let module = clis.iter().find(|c| c.plugin_id == id).map(|c| c.module.clone()).unwrap_or_default();
            let folder = plugins_dir.join(&id);
            if let Err(e) = engine.allow_root(&folder) {
                problems.push((id.clone(), e.message));
                continue;
            }
            let path: PathBuf = folder.join(&module);
            specs.push(toybox_script::Value::object([
                ("id", toybox_script::Value::String(id.clone())),
                ("path", toybox_script::Value::String(path.to_string_lossy().into_owned())),
                ("permissions", plugin.manifest.get("permissions").cloned().map(to_script).unwrap_or_else(|| toybox_script::Value::object::<&str>([]))),
                ("requiredBy", toybox_script::Value::Array(plugin.required_by.iter().cloned().map(toybox_script::Value::String).collect())),
            ]));
        }

        let namespace = engine.eval_module("toybox:bootstrap", BOOTSTRAP).map_err(script_error)?;
        let load = engine.get(&namespace, "loadPlugins").map_err(script_error)?;
        let reported = engine.call(&load, vec![toybox_script::Value::Array(specs)]).map_err(script_error)?;
        for entry in reported.as_array().unwrap_or(&[]) {
            if let Some([id, message]) = entry.as_array() {
                problems.push((id.as_str().unwrap_or("").to_string(), message.as_str().unwrap_or("").to_string()));
            }
        }
        let manifests = active.into_iter().map(|p| (p.id, p.manifest)).collect();
        Ok(PluginRuntime { engine, host, namespace, problems, clis, manifests, cancel })
    }

    /// `(plugin id, what went wrong)` for each module that did not load.
    pub fn problems(&self) -> &[(String, String)] {
        &self.problems
    }

    /// Set to ask the run in progress to stop: its `signal` aborts when it next calls the host, and
    /// every host call then fails with "Cancelled". Wire it to Ctrl-C.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// Set to stop script that is running right now (even a busy loop) with an interrupt error.
    pub fn interrupt_flag(&self) -> Arc<AtomicBool> {
        self.engine.interrupt_flag()
    }

    fn call_bootstrap(&self, function: &str, request: toybox_script::Value) -> AppResult<serde_json::Value> {
        let f = self.engine.get(&self.namespace, function).map_err(script_error)?;
        let out = self.engine.call(&f, vec![request]).map_err(script_error)?;
        out.to_json().map_err(AppError::msg)
    }

    /// Prepare the library (migrations, a catalog, a remote library brought up to date) and check it exists.
    fn open(&self, library: &str) -> AppResult<()> {
        self.host.open_library(library, false).map(|_| ())
    }

    fn library_value(&self, id: &str) -> AppResult<toybox_script::Value> {
        let info = self.host.library_info(id)?;
        Ok(toybox_script::Value::object([("id", info.id.into()), ("name", info.name.into())]))
    }

    /// Ids as the plugins see them: `<library>::<id>`. An id that is not qualified is in `library`.
    fn qualified(&self, library: &str, ids: &[String]) -> Vec<toybox_script::Value> {
        let known: Vec<String> = self.host.list_libraries().into_iter().map(|l| l.id).collect();
        ids.iter()
            .map(|id| {
                let id = match bridge::parse_qualified(id) {
                    Some((lib, _)) if lib == library || known.iter().any(|k| k == lib) => id.clone(),
                    _ => bridge::qualify(library, id),
                };
                toybox_script::Value::String(id)
            })
            .collect()
    }

    fn plugin_cli(&self, plugin_id: &str) -> AppResult<&PluginCli> {
        self.clis.iter().find(|c| c.plugin_id == plugin_id).ok_or_else(|| AppError::msg(format!("plugin '{plugin_id}' adds nothing to the command line")))
    }

    pub fn run_importer(&self, plugin_id: &str, name: &str, request: ImportRequest) -> AppResult<ImportOutcome> {
        let importer = self
            .plugin_cli(plugin_id)?
            .importers
            .iter()
            .find(|i| i.name == name)
            .ok_or_else(|| AppError::msg(format!("plugin '{plugin_id}' has no importer '{name}'")))?;
        self.open(&request.library)?;
        let d = &request.defaults;
        let provenance: Vec<(&str, &Option<String>)> = vec![("source", &d.source), ("publisher", &d.publisher), ("license", &d.license), ("url", &d.url)];
        let defaults = toybox_script::Value::object([
            ("tags", toybox_script::Value::Array(d.tags.iter().cloned().map(toybox_script::Value::String).collect())),
            (
                "provenance",
                toybox_script::Value::object(provenance.into_iter().filter_map(|(k, v)| v.clone().map(|v| (k, toybox_script::Value::String(v))))),
            ),
        ]);
        let reply = self.call_bootstrap(
            "runImporter",
            toybox_script::Value::object([
                ("pluginId", plugin_id.into()),
                ("name", name.into()),
                ("library", self.library_value(&request.library)?),
                ("paths", toybox_script::Value::Array(request.paths.into_iter().map(toybox_script::Value::String).collect())),
                ("options", to_script(Value::Object(request.options))),
                ("optionSpecs", option_specs(&importer.options)),
                ("defaults", defaults),
            ]),
        )?;
        Ok(serde_json::from_value(reply).map_err(|e| AppError::msg(format!("the importer returned an unusable result: {e}")))?)
    }

    pub fn run_exporter(&self, plugin_id: &str, request: ExportRequest) -> AppResult<ExportOutcome> {
        let exporter = self.plugin_cli(plugin_id)?.exporter.as_ref().ok_or_else(|| AppError::msg(format!("plugin '{plugin_id}' has no exporter")))?;
        let manifest = self.manifests.get(plugin_id).cloned().unwrap_or(Value::Null);
        self.open(&request.library)?;
        let reply = self.call_bootstrap(
            "runExporter",
            toybox_script::Value::object([
                ("pluginId", plugin_id.into()),
                ("library", self.library_value(&request.library)?),
                ("assetIds", toybox_script::Value::Array(self.qualified(&request.library, &request.asset_ids))),
                ("assetKinds", toybox_script::Value::Array(kinds_of(&manifest).into_iter().map(toybox_script::Value::String).collect())),
                ("targetDir", request.target_dir.into()),
                ("preserveStructure", request.preserve_structure.into()),
                ("options", to_script(Value::Object(request.options))),
                ("optionSpecs", option_specs(&exporter.options)),
                ("previewOnly", request.preview_only.into()),
            ]),
        )?;
        Ok(serde_json::from_value(reply).map_err(|e| AppError::msg(format!("the exporter returned an unusable result: {e}")))?)
    }

    pub fn run_command(&self, plugin_id: &str, request: CommandRequest) -> AppResult<CommandOutcome> {
        let cli = self.plugin_cli(plugin_id)?;
        let mut level: &[CliCommand] = &cli.commands;
        let mut found: Option<&CliCommand> = None;
        for part in request.path.split(' ') {
            found = level.iter().find(|c| c.name == part);
            match found {
                Some(c) => level = &c.commands,
                None => return Err(AppError::msg(format!("plugin '{plugin_id}' has no command '{}'", request.path))),
            }
        }
        let command = found.filter(|c| c.commands.is_empty()).ok_or_else(|| AppError::msg(format!("'{}' is a group of commands, not a command", request.path)))?;
        self.open(&request.library)?;
        let asset_ids = match &request.asset_ids {
            Some(ids) => toybox_script::Value::Array(self.qualified(&request.library, ids)),
            None => toybox_script::Value::Null,
        };
        let reply = self.call_bootstrap(
            "runCommand",
            toybox_script::Value::object([
                ("pluginId", plugin_id.into()),
                ("path", request.path.as_str().into()),
                ("library", self.library_value(&request.library)?),
                ("args", to_script(Value::Object(request.args))),
                ("options", to_script(Value::Object(request.options))),
                ("argSpecs", option_specs(&command.args)),
                ("optionSpecs", option_specs(&command.options)),
                ("assetIds", asset_ids),
            ]),
        )?;
        let printed = reply.get("printed").and_then(Value::as_bool).unwrap_or(false);
        Ok(CommandOutcome { printed, result: reply.get("result").cloned().unwrap_or(Value::Null) })
    }
}
