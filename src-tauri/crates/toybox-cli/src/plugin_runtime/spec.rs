//! What plugins declare for the command line: the manifest's `cli` section, read
//! without running any plugin code. See docs/cli.md.

use std::collections::HashSet;

use serde_json::Value;
use toybox_core::error::AppResult;
use toybox_host::Host;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliValueType {
    String,
    Number,
    Flag,
    Choice,
    Path,
}

#[derive(Debug, Clone)]
pub struct CliOption {
    /// kebab-case, given as `--name`.
    pub name: String,
    /// The key the plugin receives: the camelCase of `name`.
    pub key: String,
    pub value_type: CliValueType,
    pub choices: Vec<String>,
    pub default: Option<Value>,
    pub required: bool,
    pub multiple: bool,
    pub help: String,
}

/// A positional argument: an option that is never a flag.
pub type CliArg = CliOption;

#[derive(Debug, Clone)]
pub struct CliImporter {
    pub name: String,
    pub summary: String,
    pub options: Vec<CliOption>,
}

#[derive(Debug, Clone)]
pub struct CliExporter {
    pub name: String,
    pub summary: String,
    /// The declared options, or the ones derived from the manifest's `fields`.
    pub options: Vec<CliOption>,
}

#[derive(Debug, Clone)]
pub struct CliCommand {
    pub name: String,
    pub summary: String,
    /// Non-empty for a group; a group has no args or options.
    pub commands: Vec<CliCommand>,
    pub args: Vec<CliArg>,
    pub options: Vec<CliOption>,
    pub selection: bool,
}

/// One active plugin's command-line contributions that are mounted.
#[derive(Debug, Clone)]
pub struct PluginCli {
    pub plugin_id: String,
    pub plugin_name: String,
    pub importers: Vec<CliImporter>,
    pub exporter: Option<CliExporter>,
    pub commands: Vec<CliCommand>,
    /// Declarations that are not mounted and why (an invalid entry, a name a
    /// built-in command or an earlier plugin already uses).
    pub problems: Vec<String>,
    /// The plugin-relative `cli.module`; empty when the manifest names none (then nothing is mounted).
    pub module: String,
}

/// Names `tbx export` takes itself.
pub const BUILTIN_EXPORTERS: &[&str] = &["copy", "glb"];
/// Option names the shared options of every command line command take.
const RESERVED_ALWAYS: &[&str] = &["help", "version", "library", "json", "verbose", "quiet"];
/// Shared options of `tbx import`.
const RESERVED_IMPORTER: &[&str] = &["tag", "source", "publisher", "license", "url"];
/// Shared options of `tbx export`.
const RESERVED_EXPORTER: &[&str] = &["to", "preserve-structure"];

/// What the earlier plugins (and the built-ins) have already taken.
#[derive(Default)]
struct Taken {
    importers: HashSet<String>,
    exporters: HashSet<String>,
    commands: HashSet<String>,
}

/// An installed, active plugin: its id, display name and raw manifest.
pub struct ActiveManifest {
    pub id: String,
    pub name: String,
    pub manifest: Value,
    /// Ids of the plugins it depends on.
    pub dependencies: Vec<String>,
    /// Ids of the switched-on plugins that depend on it (they may be named in `origin.plugin`).
    pub required_by: Vec<String>,
}

fn is_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase()) && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_option_name(s: &str) -> bool {
    is_name(s) && !s.ends_with('-') && !s.contains("--")
}

/// `flip-green` -> `flipGreen`.
pub fn camel_case(name: &str) -> String {
    let mut out = String::new();
    let mut up = false;
    for c in name.chars() {
        if c == '-' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `subDir` -> `sub-dir`.
fn kebab_case(key: &str) -> String {
    let mut out = String::new();
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else if c == '_' || c == ' ' {
            out.push('-');
        } else {
            out.push(c);
        }
    }
    out
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn parse_type(raw: &str) -> Option<CliValueType> {
    Some(match raw {
        "string" => CliValueType::String,
        "number" => CliValueType::Number,
        "flag" => CliValueType::Flag,
        "choice" => CliValueType::Choice,
        "path" => CliValueType::Path,
        _ => return None,
    })
}

/// One option or argument. `arg`: a positional argument (never a flag).
fn parse_option(v: &Value, arg: bool, reserved: &[&str]) -> Result<CliOption, String> {
    let name = v.get("name").and_then(Value::as_str).ok_or("it has no name")?;
    if !is_option_name(name) {
        return Err(format!("'{name}' is not a kebab-case name"));
    }
    if !arg && (RESERVED_ALWAYS.contains(&name) || reserved.contains(&name)) {
        return Err(format!("'--{name}' is used by tbx itself"));
    }
    let raw_type = v.get("type").and_then(Value::as_str).ok_or_else(|| format!("'{name}' has no type"))?;
    let value_type = parse_type(raw_type).ok_or_else(|| format!("'{name}' has the unknown type '{raw_type}'"))?;
    if arg && value_type == CliValueType::Flag {
        return Err(format!("the argument '{name}' cannot be a flag"));
    }
    let choices: Vec<String> = match v.get("choices") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|c| c.as_str().map(str::to_string).ok_or_else(|| format!("'{name}': choices must be strings")))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(format!("'{name}': choices must be a list")),
    };
    if value_type == CliValueType::Choice && choices.is_empty() {
        return Err(format!("the choice '{name}' lists no choices"));
    }
    let default = match v.get("default") {
        None | Some(Value::Null) => None,
        Some(d) => {
            let ok = match value_type {
                CliValueType::String | CliValueType::Path => d.is_string(),
                CliValueType::Number => d.is_number(),
                CliValueType::Flag => d.is_boolean(),
                CliValueType::Choice => d.as_str().is_some_and(|s| choices.iter().any(|c| c == s)),
            };
            if !ok {
                return Err(format!("the default of '{name}' does not fit its type"));
            }
            Some(d.clone())
        }
    };
    Ok(CliOption {
        key: camel_case(name),
        name: name.to_string(),
        value_type,
        choices,
        default,
        required: v.get("required").and_then(Value::as_bool).unwrap_or(false),
        multiple: v.get("multiple").and_then(Value::as_bool).unwrap_or(false),
        help: text(v, "help"),
    })
}

fn parse_options(list: Option<&Value>, arg: bool, reserved: &[&str], what: &str, problems: &mut Vec<String>) -> Option<Vec<CliOption>> {
    let items = match list {
        None | Some(Value::Null) => return Some(Vec::new()),
        Some(Value::Array(items)) => items,
        Some(_) => {
            problems.push(format!("{what}: its {} must be a list", if arg { "args" } else { "options" }));
            return None;
        }
    };
    let mut out: Vec<CliOption> = Vec::new();
    for item in items {
        match parse_option(item, arg, reserved) {
            Ok(option) => {
                if out.iter().any(|o| o.name == option.name) {
                    problems.push(format!("{what}: '{}' is declared twice", option.name));
                    return None;
                }
                out.push(option);
            }
            Err(why) => {
                problems.push(format!("{what}: {why}"));
                return None;
            }
        }
    }
    Some(out)
}

/// The options of an exporter that declares none: one per manifest field.
fn options_from_fields(fields: Option<&Value>) -> Vec<CliOption> {
    let mut out = Vec::new();
    for field in fields.and_then(Value::as_array).into_iter().flatten() {
        let Some(key) = field.get("key").and_then(Value::as_str) else { continue };
        let kind = field.get("type").and_then(Value::as_str).unwrap_or("");
        let (value_type, choices) = match kind {
            "text" => (CliValueType::String, Vec::new()),
            "checkbox" => (CliValueType::Flag, Vec::new()),
            "select" => (
                CliValueType::Choice,
                field.get("options").and_then(Value::as_array).into_iter().flatten().filter_map(|o| o.get("value").and_then(Value::as_str)).map(str::to_string).collect(),
            ),
            "directory" | "saveFile" => (CliValueType::Path, Vec::new()),
            _ => continue,
        };
        let name = kebab_case(key);
        if !is_option_name(&name) || RESERVED_ALWAYS.contains(&name.as_str()) || RESERVED_EXPORTER.contains(&name.as_str()) {
            continue;
        }
        let label = text(field, "label");
        let hint = text(field, "hint");
        out.push(CliOption {
            name,
            key: key.to_string(),
            value_type,
            choices,
            default: field.get("default").filter(|d| !d.is_null()).cloned(),
            required: field.get("required").and_then(Value::as_bool).unwrap_or(false),
            multiple: false,
            help: if hint.is_empty() { label } else { format!("{label}. {hint}") },
        });
    }
    out
}

fn parse_command(v: &Value, path: &str, problems: &mut Vec<String>) -> Option<CliCommand> {
    let name = v.get("name").and_then(Value::as_str).unwrap_or("");
    let what = format!("command '{path}'");
    let groups = v.get("commands");
    let is_group = groups.is_some_and(|g| !g.is_null());
    if is_group {
        if v.get("args").is_some() || v.get("options").is_some() || v.get("selection").is_some() {
            problems.push(format!("{what}: a group has no args, options or selection"));
            return None;
        }
        let Some(items) = groups.and_then(Value::as_array).filter(|a| !a.is_empty()) else {
            problems.push(format!("{what}: a group needs a non-empty commands list"));
            return None;
        };
        let mut children: Vec<CliCommand> = Vec::new();
        for child in items {
            let child_name = child.get("name").and_then(Value::as_str).unwrap_or("");
            let child_path = format!("{path} {child_name}");
            if !is_name(child_name) {
                problems.push(format!("command '{child_path}': '{child_name}' is not a lower-case word"));
                continue;
            }
            if children.iter().any(|c| c.name == child_name) {
                problems.push(format!("command '{child_path}': declared twice"));
                continue;
            }
            if let Some(c) = parse_command(child, &child_path, problems) {
                children.push(c);
            }
        }
        if children.is_empty() {
            return None;
        }
        return Some(CliCommand { name: name.into(), summary: text(v, "summary"), commands: children, args: Vec::new(), options: Vec::new(), selection: false });
    }
    let args = parse_options(v.get("args"), true, &[], &what, problems)?;
    let options = parse_options(v.get("options"), false, &[], &what, problems)?;
    if let Some(i) = args.iter().position(|a| a.multiple) {
        if i + 1 != args.len() {
            problems.push(format!("{what}: only the last argument can take several values"));
            return None;
        }
    }
    if let Some(dup) = args.iter().find(|a| options.iter().any(|o| o.key == a.key)) {
        problems.push(format!("{what}: '{}' is both an argument and an option", dup.name));
        return None;
    }
    Some(CliCommand { name: name.into(), summary: text(v, "summary"), commands: Vec::new(), args, options, selection: v.get("selection").and_then(Value::as_bool).unwrap_or(false) })
}

fn module_is_valid(module: &str) -> bool {
    !module.is_empty() && !module.starts_with('/') && !module.starts_with('\\') && !module.contains(':') && !module.split(['/', '\\']).any(|part| part == "..")
}

fn parse_plugin(id: &str, name: &str, manifest: &Value, builtin: &[&str], taken: &mut Taken) -> Option<PluginCli> {
    let cli = manifest.get("cli").filter(|c| !c.is_null())?;
    let mut problems = Vec::new();
    let mut out = PluginCli {
        plugin_id: id.into(),
        plugin_name: name.into(),
        importers: Vec::new(),
        exporter: None,
        commands: Vec::new(),
        problems: Vec::new(),
        module: String::new(),
    };
    let module = cli.get("module").and_then(Value::as_str).unwrap_or("");
    if !module_is_valid(module) {
        problems.push("cli.module must be a plugin-relative path to an ES module".to_string());
        out.problems = problems;
        return Some(out);
    }
    out.module = module.to_string();

    for item in cli.get("importers").and_then(Value::as_array).into_iter().flatten() {
        let n = item.get("name").and_then(Value::as_str).unwrap_or("");
        if !is_name(n) {
            problems.push(format!("importer '{n}': not a lower-case word"));
            continue;
        }
        let what = format!("importer '{n}'");
        let Some(options) = parse_options(item.get("options"), false, RESERVED_IMPORTER, &what, &mut problems) else { continue };
        if !taken.importers.insert(n.to_string()) {
            problems.push(format!("{what}: another plugin already provides `tbx import {n}`"));
            continue;
        }
        out.importers.push(CliImporter { name: n.into(), summary: text(item, "summary"), options });
    }

    if let Some(item) = cli.get("exporter").filter(|e| !e.is_null()) {
        let n = item.get("name").and_then(Value::as_str).unwrap_or("");
        let what = format!("exporter '{n}'");
        if !is_name(n) {
            problems.push(format!("exporter '{n}': not a lower-case word"));
        } else if let Some(mut options) = parse_options(item.get("options"), false, RESERVED_EXPORTER, &what, &mut problems) {
            if options.is_empty() {
                options = options_from_fields(manifest.get("fields"));
            }
            if BUILTIN_EXPORTERS.contains(&n) {
                problems.push(format!("{what}: `tbx export {n}` is built in"));
            } else if !taken.exporters.insert(n.to_string()) {
                problems.push(format!("{what}: another plugin already provides `tbx export {n}`"));
            } else {
                out.exporter = Some(CliExporter { name: n.into(), summary: text(item, "summary"), options });
            }
        }
    }

    let mut seen_here: HashSet<String> = HashSet::new();
    for item in cli.get("commands").and_then(Value::as_array).into_iter().flatten() {
        let n = item.get("name").and_then(Value::as_str).unwrap_or("");
        if !is_name(n) {
            problems.push(format!("command '{n}': not a lower-case word"));
            continue;
        }
        if !seen_here.insert(n.to_string()) {
            problems.push(format!("command '{n}': declared twice"));
            continue;
        }
        let Some(command) = parse_command(item, n, &mut problems) else { continue };
        if builtin.contains(&n) {
            problems.push(format!("command '{n}': `tbx {n}` is built in"));
        } else if taken.commands.contains(n) {
            problems.push(format!("command '{n}': another plugin already provides `tbx {n}`"));
        } else {
            taken.commands.insert(n.to_string());
            out.commands.push(command);
        }
    }
    out.problems = problems;
    Some(out)
}

/// The command-line contributions of the given plugins, ordered by plugin id.
/// Plugins without a `cli` section are left out.
pub fn clis_of(plugins: &[ActiveManifest], builtin: &[&str]) -> Vec<PluginCli> {
    let mut sorted: Vec<&ActiveManifest> = plugins.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut taken = Taken::default();
    sorted.into_iter().filter_map(|p| parse_plugin(&p.id, &p.name, &p.manifest, builtin, &mut taken)).collect()
}

/// The active plugins (switched on, dependencies usable) with their raw manifests.
pub fn active_manifests(host: &Host) -> AppResult<Vec<ActiveManifest>> {
    let mut out = Vec::new();
    for plugin in host.list_plugins()? {
        if !plugin.enabled || !plugin.blocked_by.is_empty() {
            continue;
        }
        let manifest = host.plugin_manifest(&plugin.id)?;
        out.push(ActiveManifest {
            id: plugin.id,
            name: plugin.name,
            manifest,
            dependencies: plugin.dependencies.keys().cloned().collect(),
            required_by: plugin.required_by,
        });
    }
    Ok(out)
}

/// The command-line contributions of every active plugin, ordered by plugin id.
/// `builtin` are the names of the built-in top-level commands, which plugin
/// commands may not take (importer names live under `tbx import`, exporter names
/// under `tbx export`, where `copy` and `glb` are taken).
pub fn plugin_clis(host: &Host, builtin: &[&str]) -> AppResult<Vec<PluginCli>> {
    Ok(clis_of(&active_manifests(host)?, builtin))
}

/// The library types a plugin works with: its `assetKinds`, then the types it declares; models when neither.
pub fn kinds_of(manifest: &Value) -> Vec<String> {
    let mut kinds: Vec<String> = manifest.get("assetKinds").and_then(Value::as_array).into_iter().flatten().filter_map(|k| k.as_str().map(str::to_string)).collect();
    for declared in manifest.get("assetTypes").and_then(Value::as_array).into_iter().flatten() {
        if let Some(id) = declared.get("id").and_then(Value::as_str) {
            if !kinds.iter().any(|k| k == id) {
                kinds.push(id.to_string());
            }
        }
    }
    if kinds.is_empty() {
        kinds.push("model".into());
    }
    kinds
}

/// Whether a plugin handles assets of `kind`; `"*"` accepts every type.
pub fn accepts_kind(manifest: &Value, kind: &str) -> bool {
    kinds_of(manifest).iter().any(|k| k == "*" || k == kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plugin(id: &str, manifest: Value) -> ActiveManifest {
        ActiveManifest { id: id.into(), name: id.into(), manifest, dependencies: Vec::new(), required_by: Vec::new() }
    }

    fn parse(manifests: Vec<ActiveManifest>) -> Vec<PluginCli> {
        clis_of(&manifests, &["import", "export", "library", "plugin"])
    }

    #[test]
    fn the_documented_example_parses() {
        let m = json!({
            "cli": {
                "module": "dist/cli.js",
                "importers": [{ "name": "audio", "summary": "Import WAV.", "options": [
                    { "name": "category", "type": "string", "default": "sounds", "help": "Category." },
                    { "name": "flip-green", "type": "flag" } ] }],
                "exporter": { "name": "placer", "summary": "Publish.", "options": [] },
                "commands": [{ "name": "sbsar", "summary": "Work.", "commands": [
                    { "name": "inputs", "summary": "List.", "args": [{ "name": "file", "type": "path", "required": true, "help": "The file." }] } ] }]
            },
            "fields": [
                { "key": "subDir", "type": "text", "label": "Sub dir", "default": "assets" },
                { "key": "format", "type": "select", "label": "Format", "options": [{ "value": "glb", "label": "GLB" }, { "value": "copy", "label": "Copy" }], "default": "glb" },
                { "key": "keep", "type": "checkbox", "label": "Keep" },
                { "key": "libraryJson", "type": "saveFile", "label": "Library file", "required": true },
                { "key": "outDir", "type": "directory", "label": "Out" }
            ]
        });
        let all = parse(vec![plugin("com.a", m)]);
        let p = &all[0];
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        assert_eq!(p.module, "dist/cli.js");
        assert_eq!(p.importers[0].name, "audio");
        assert_eq!(p.importers[0].options[1].key, "flipGreen");
        assert_eq!(p.importers[0].options[0].default, Some(json!("sounds")));
        let ex = p.exporter.as_ref().unwrap();
        let by: Vec<_> = ex.options.iter().map(|o| (o.name.as_str(), o.key.as_str(), o.value_type)).collect();
        assert_eq!(by, [
            ("sub-dir", "subDir", CliValueType::String), ("format", "format", CliValueType::Choice), ("keep", "keep", CliValueType::Flag),
            ("library-json", "libraryJson", CliValueType::Path), ("out-dir", "outDir", CliValueType::Path),
        ]);
        assert_eq!(ex.options[1].choices, ["glb", "copy"]);
        assert!(ex.options[3].required);
        let group = &p.commands[0];
        assert_eq!(group.commands[0].name, "inputs");
        assert!(group.commands[0].args[0].required);
    }

    #[test]
    fn declared_exporter_options_win_over_fields() {
        let m = json!({ "cli": { "module": "a.js", "exporter": { "name": "x", "summary": "", "options": [{ "name": "only", "type": "flag" }] } },
            "fields": [{ "key": "other", "type": "checkbox", "label": "o" }] });
        let all = parse(vec![plugin("p", m)]);
        let names: Vec<_> = all[0].exporter.as_ref().unwrap().options.iter().map(|o| o.name.clone()).collect();
        assert_eq!(names, ["only"]);
    }

    #[test]
    fn collisions_go_to_the_earlier_plugin_by_id() {
        let mk = |cmd: &str| json!({ "cli": { "module": "a.js",
            "importers": [{ "name": "audio", "summary": "" }],
            "exporter": { "name": "copy", "summary": "" },
            "commands": [{ "name": cmd, "summary": "", "args": [] }] } });
        let all = parse(vec![plugin("com.z", mk("tool")), plugin("com.a", mk("tool")), plugin("com.m", mk("library"))]);
        assert_eq!(all.iter().map(|p| p.plugin_id.as_str()).collect::<Vec<_>>(), ["com.a", "com.m", "com.z"]);
        assert_eq!(all[0].importers.len(), 1);
        assert_eq!(all[0].commands.len(), 1);
        assert!(all[0].exporter.is_none() && all[0].problems.iter().any(|p| p.contains("built in")));
        assert!(all[1].importers.is_empty() && all[1].problems.iter().any(|p| p.contains("`tbx import audio`")));
        assert!(all[1].commands.is_empty() && all[1].problems.iter().any(|p| p.contains("`tbx library` is built in")));
        assert!(all[2].commands.is_empty() && all[2].problems.iter().any(|p| p.contains("`tbx tool`")));
    }

    #[test]
    fn invalid_entries_are_reported_and_the_rest_still_mounts() {
        let m = json!({ "cli": { "module": "a.js",
            "importers": [
                { "name": "Bad_Name", "summary": "" },
                { "name": "ok", "summary": "" },
                { "name": "typo", "summary": "", "options": [{ "name": "x", "type": "colour" }] },
                { "name": "reserved", "summary": "", "options": [{ "name": "tag", "type": "string" }] },
                { "name": "nochoices", "summary": "", "options": [{ "name": "c", "type": "choice" }] },
                { "name": "baddefault", "summary": "", "options": [{ "name": "n", "type": "number", "default": "five" }] },
                { "name": "twice", "summary": "", "options": [{ "name": "a", "type": "flag" }, { "name": "a", "type": "flag" }] }
            ],
            "commands": [
                { "name": "mixed", "summary": "", "commands": [{ "name": "x", "summary": "" }], "args": [] },
                { "name": "emptygroup", "summary": "", "commands": [] },
                { "name": "flagarg", "summary": "", "args": [{ "name": "f", "type": "flag" }] },
                { "name": "restfirst", "summary": "", "args": [{ "name": "a", "type": "string", "multiple": true }, { "name": "b", "type": "string" }] },
                { "name": "fine", "summary": "", "args": [{ "name": "a", "type": "string" }], "options": [{ "name": "b", "type": "number", "default": 3 }], "selection": true }
            ] } });
        let all = parse(vec![plugin("p", m)]);
        let p = &all[0];
        assert_eq!(p.importers.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["ok"]);
        assert_eq!(p.commands.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["fine"]);
        assert!(p.commands[0].selection);
        assert_eq!(p.problems.len(), 10, "{:#?}", p.problems);
    }

    #[test]
    fn a_bad_module_mounts_nothing_and_no_cli_means_no_entry() {
        let all = parse(vec![
            plugin("a", json!({ "cli": { "module": "../out.js", "importers": [{ "name": "x", "summary": "" }] } })),
            plugin("b", json!({ "cli": { "importers": [{ "name": "y", "summary": "" }] } })),
            plugin("c", json!({ "entry": "dist/index.js" })),
        ]);
        assert_eq!(all.len(), 2);
        assert!(all.iter().all(|p| p.importers.is_empty() && p.problems.len() == 1));
    }

    #[test]
    fn accepts_kind_follows_the_app() {
        assert!(accepts_kind(&json!({}), "model"));
        assert!(!accepts_kind(&json!({}), "audio"));
        assert!(accepts_kind(&json!({ "assetKinds": ["material"], "assetTypes": [{ "id": "skybox" }] }), "skybox"));
        assert!(accepts_kind(&json!({ "assetKinds": ["*"] }), "anything"));
    }

    #[test]
    fn camel_and_kebab() {
        assert_eq!(camel_case("flip-green-x"), "flipGreenX");
        assert_eq!(kebab_case("subDir"), "sub-dir");
        assert_eq!(kebab_case("libraryJSON"), "library-j-s-o-n");
    }
}
