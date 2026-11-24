//! The Rust functions behind the bootstrap's `import.meta.host.call(name, ...)`: `toybox-host`
//! bound for plugin code. Library ids that reach plugins are qualified (`<library>::<id>`) exactly as in
//! the app; whatever comes back from a plugin is un-qualified here.

use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value as Json;
use toybox_core::ops::browse::ItemRef;
use toybox_core::ops::export_plugin::{placer_merge_file, transcode_image, PlacerAssetDto};
use toybox_host::{plugin_fs, plugin_services, Host};
use toybox_script::{Engine, Value};

use super::Event;

pub(super) const CANCELLED: &str = "Cancelled";

type Args = Vec<Value>;
type Reply = Result<Value, String>;

fn arg<'a>(args: &'a [Value], i: usize, name: &str) -> Result<&'a Value, String> {
    args.get(i).ok_or_else(|| format!("missing argument '{name}'"))
}

fn text<'a>(args: &'a [Value], i: usize, name: &str) -> Result<&'a str, String> {
    arg(args, i, name)?.as_str().ok_or_else(|| format!("'{name}' must be a string"))
}

fn bytes<'a>(args: &'a [Value], i: usize, name: &str) -> Result<&'a [u8], String> {
    arg(args, i, name)?.as_bytes().ok_or_else(|| format!("'{name}' must be a byte buffer"))
}

fn number(args: &[Value], i: usize, name: &str) -> Result<f64, String> {
    arg(args, i, name)?.as_f64().ok_or_else(|| format!("'{name}' must be a number"))
}

fn flag(args: &[Value], i: usize) -> bool {
    args.get(i).and_then(Value::as_bool).unwrap_or(false)
}

fn json(v: &Value) -> Result<Json, String> {
    v.to_json()
}

fn string_list(args: &[Value], i: usize, name: &str) -> Result<Vec<String>, String> {
    arg(args, i, name)?
        .as_array()
        .ok_or_else(|| format!("'{name}' must be a list"))?
        .iter()
        .map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("'{name}' must be a list of strings")))
        .collect()
}

fn to_value<T: serde::Serialize>(v: &T) -> Reply {
    serde_json::to_value(v).map(Value::from).map_err(|e| e.to_string())
}

fn strings(list: Vec<String>) -> Value {
    Value::Array(list.into_iter().map(Value::String).collect())
}

/// `<library>::<id>` into its parts; `None` when `id` is not qualified.
pub(super) fn parse_qualified(id: &str) -> Option<(&str, &str)> {
    let at = id.find("::").filter(|&at| at > 0)?;
    let library = &id[..at];
    library.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-').then(|| (library, &id[at + 2..]))
}

pub(super) fn qualify(library: &str, id: &str) -> String {
    format!("{library}::{id}")
}

fn qualify_ref(library: &str, id: &str) -> String {
    if id.starts_with("toybox:") || id.starts_with(&format!("{library}::")) {
        id.to_string()
    } else {
        qualify(library, id)
    }
}

/// `qualifyAsset` of `domain/libraryId.ts`: the id and the references to other items, variants included.
pub(super) fn qualify_asset(asset: &mut Json, library: &str) {
    let Some(map) = asset.as_object_mut() else { return };
    if let Some(id) = map.get("id").and_then(Json::as_str).map(str::to_string) {
        map.insert("id".into(), Json::String(qualify(library, &id)));
    }
    map.insert("library".into(), Json::String(library.to_string()));
    if let Some(preview) = map.get_mut("user").and_then(|u| u.get_mut("previewAssetId")) {
        if let Some(id) = preview.as_str().filter(|s| !s.is_empty()).map(str::to_string) {
            *preview = Json::String(qualify_ref(library, &id));
        }
    }
    if let Some(parent) = map.get_mut("origin").and_then(|o| o.get_mut("derivedFrom")) {
        if let Some(id) = parent.as_str().filter(|s| !s.is_empty()).map(str::to_string) {
            *parent = Json::String(qualify_ref(library, &id));
        }
    }
    if map.get("kind").and_then(Json::as_str) == Some("animation") {
        if let Some(source) = map.get_mut("clip").and_then(|c| c.get_mut("sourceId")) {
            if let Some(id) = source.as_str().filter(|s| !s.is_empty()).map(str::to_string) {
                *source = Json::String(qualify_ref(library, &id));
            }
        }
    }
    if let Some(Json::Array(variants)) = map.get_mut("variants") {
        for variant in variants {
            qualify_asset(variant, library);
        }
    }
}

/// The library and raw id of an id a plugin hands back; an unqualified id is in `default`.
fn split<'a>(id: &'a str, default: &'a str) -> (&'a str, &'a str) {
    parse_qualified(id).unwrap_or((default, id))
}

/// Writes may only go below `jail` when a run has one (an export's target folder).
fn check_jail(jail: Option<&Value>, root: &str) -> Result<(), String> {
    match jail.and_then(Value::as_str) {
        Some(j) => plugin_fs::jail(Path::new(j), root).map(|_| ()).map_err(|e| format!("{e} (this run may only write below {j})")),
        None => Ok(()),
    }
}

pub(super) fn register(engine: &Engine, host: &Arc<Host>, events: &Rc<dyn Fn(Event)>, cancel: &Arc<AtomicBool>) {
    let def = |name: &str, f: Box<dyn Fn(Args) -> Reply>| {
        let cancel = cancel.clone();
        engine.register_host_fn(name, move |args| {
            if cancel.load(Ordering::Relaxed) {
                return Err(CANCELLED.to_string());
            }
            f(args)
        });
    };
    macro_rules! with_host {
        ($name:expr, |$h:ident, $a:ident| $body:expr) => {{
            let $h = host.clone();
            def($name, Box::new(move |$a: Args| -> Reply { $body }));
        }};
    }

    // Events are never refused: a cancelled run still reports what it was doing.
    {
        let events = events.clone();
        engine.register_host_fn("event", move |args| {
            let kind = text(&args, 0, "kind")?;
            let message = text(&args, 1, "text")?.to_string();
            events(match kind {
                "print" => Event::Print(message),
                "warn" => Event::Warn(message),
                _ => Event::Log(message),
            });
            Ok(Value::Null)
        });
    }

    // --- the jailed filesystem (services/pluginHost.ts buildFsApi) ---
    with_host!("fs.readDir", |_h, a| {
        let entries = plugin_fs::read_dir_under(Path::new(text(&a, 0, "sourceRoot")?), text(&a, 1, "path")?).map_err(|e| e.to_string())?;
        to_value(&entries)
    });
    with_host!("fs.readBytes", |_h, a| {
        plugin_fs::read_bytes_under(Path::new(text(&a, 0, "sourceRoot")?), text(&a, 1, "path")?).map(Value::Bytes).map_err(|e| e.to_string())
    });
    with_host!("fs.hashFile", |_h, a| {
        plugin_fs::hash_file_under(Path::new(text(&a, 0, "sourceRoot")?), text(&a, 1, "path")?).map(Value::String).map_err(|e| e.to_string())
    });
    with_host!("fs.fileSize", |_h, a| {
        plugin_fs::file_size_under(Path::new(text(&a, 0, "sourceRoot")?), text(&a, 1, "path")?).map(|n| Value::Number(n as f64)).map_err(|e| e.to_string())
    });
    with_host!("fs.readImageThumb", |_h, a| {
        let max = number(&a, 2, "maxSize")?.max(1.0) as u32;
        plugin_fs::image_thumb_under(Path::new(text(&a, 0, "sourceRoot")?), text(&a, 1, "path")?, max).map(Value::Bytes).map_err(|e| e.to_string())
    });
    with_host!("fs.copyFile", |_h, a| {
        let authorized = text(&a, 2, "authorizedRoot")?;
        check_jail(a.get(4), authorized)?;
        plugin_fs::copy_file_under(Path::new(text(&a, 0, "sourceRoot")?), text(&a, 1, "path")?, Path::new(authorized), text(&a, 3, "target")?)
            .map(|_| Value::Null)
            .map_err(|e| e.to_string())
    });
    with_host!("fs.writeBytes", |_h, a| {
        let authorized = text(&a, 0, "authorizedRoot")?;
        check_jail(a.get(3), authorized)?;
        plugin_fs::write_bytes_under(Path::new(authorized), text(&a, 1, "path")?, bytes(&a, 2, "bytes")?).map(|_| Value::Null).map_err(|e| e.to_string())
    });
    with_host!("fs.writeText", |_h, a| {
        let authorized = text(&a, 0, "authorizedRoot")?;
        check_jail(a.get(3), authorized)?;
        plugin_fs::write_text_under(Path::new(authorized), text(&a, 1, "path")?, text(&a, 2, "text")?).map(|_| Value::Null).map_err(|e| e.to_string())
    });

    with_host!("fs.removeTree", |_h, a| {
        let authorized = text(&a, 0, "authorizedRoot")?;
        check_jail(a.get(2), authorized)?;
        plugin_fs::remove_tree_under(Path::new(authorized), text(&a, 1, "path")?).map(|_| Value::Null).map_err(|e| e.to_string())
    });

    // --- libraries and items ---
    with_host!("assets.get", |h, a| {
        let default = text(&a, 0, "library")?.to_string();
        let items: Vec<ItemRef> = string_list(&a, 1, "ids")?
            .iter()
            .map(|id| {
                let (library, raw) = split(id, &default);
                ItemRef { library: library.to_string(), id: raw.to_string() }
            })
            .collect();
        let rows = h.get_assets(&items).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            let mut asset = serde_json::to_value(&row.asset).map_err(|e| e.to_string())?;
            qualify_asset(&mut asset, &row.library);
            out.push(Value::from(asset));
        }
        Ok(Value::Array(out))
    });
    with_host!("assets.derived", |h, a| {
        // The assets made from one (their `origin.derivedFrom` is its raw id), in its library.
        let default = text(&a, 0, "library")?.to_string();
        let (library, raw) = split(text(&a, 1, "id")?, &default);
        let rows = h.derived_assets(library, raw).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            let mut asset = serde_json::to_value(&row.asset).map_err(|e| e.to_string())?;
            qualify_asset(&mut asset, &row.library);
            out.push(Value::from(asset));
        }
        Ok(Value::Array(out))
    });
    with_host!("assets.ensureFiles", |h, a| {
        // The files of these items (with what they keep and the assets they were made from) on this
        // computer: a download for a remote library, nothing for a local one.
        let default = text(&a, 0, "library")?.to_string();
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for id in string_list(&a, 1, "ids")? {
            let (library, raw) = split(&id, &default);
            match groups.iter_mut().find(|(l, _)| l == library) {
                Some((_, ids)) => ids.push(raw.to_string()),
                None => groups.push((library.to_string(), vec![raw.to_string()])),
            }
        }
        for (library, ids) in groups {
            h.ensure_asset_files(&library, &ids, true).map_err(|e| e.to_string())?;
        }
        Ok(Value::Null)
    });
    with_host!("library.info", |h, a| {
        let info = h.library_info(text(&a, 0, "library")?).map_err(|e| e.to_string())?;
        Ok(Value::object([("id", info.id.into()), ("name", info.name.into()), ("root", info.root.into())]))
    });
    with_host!("library.merge", |h, a| {
        let entries = match json(arg(&a, 1, "entries")?)? {
            Json::Array(list) => list,
            _ => return Err("entries must be a list".into()),
        };
        h.merge_entries(text(&a, 0, "library")?, entries).map(|report| strings(report.ids)).map_err(|e| e.to_string())
    });
    with_host!("library.convert", |h, a| {
        h.convert_to_gltf(text(&a, 0, "library")?, text(&a, 1, "srcPath")?, text(&a, 2, "pack")?, text(&a, 3, "category")?, text(&a, 4, "stem")?)
            .map(strings)
            .map_err(|e| e.to_string())
    });

    // --- the export primitives (services/pluginHost.ts buildHostApi) ---
    with_host!("export.preview", |h, a| {
        let format = text(&a, 1, "format")?.to_string();
        let preserve = flag(&a, 2);
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for id in string_list(&a, 0, "assetIds")? {
            let (library, raw) = parse_qualified(&id).ok_or_else(|| format!("Not a library item id: {id}"))?;
            match groups.iter_mut().find(|(l, _)| l == library) {
                Some((_, ids)) => ids.push(raw.to_string()),
                None => groups.push((library.to_string(), vec![raw.to_string()])),
            }
        }
        let mut out: Vec<String> = Vec::new();
        for (library, ids) in groups {
            for path in h.preview_export(&library, &ids, &format, preserve).map_err(|e| e.to_string())? {
                if !out.contains(&path) {
                    out.push(path);
                }
            }
        }
        Ok(strings(out))
    });
    with_host!("export.gltf", |h, a| {
        let id = text(&a, 0, "assetId")?;
        let (library, raw) = parse_qualified(id).ok_or_else(|| format!("Not a library item id: {id}"))?;
        h.read_asset_gltf(library, raw).map(Value::from).map_err(|e| e.to_string())
    });
    with_host!("export.glb", |h, a| {
        let id = text(&a, 0, "assetId")?;
        let (library, raw) = parse_qualified(id).ok_or_else(|| format!("Not a library item id: {id}"))?;
        h.assemble_glb_for_asset(library, raw).map(Value::Bytes).map_err(|e| e.to_string())
    });
    with_host!("export.copy", |h, a| {
        let id = text(&a, 0, "assetId")?;
        let (library, raw) = parse_qualified(id).ok_or_else(|| format!("Not a library item id: {id}"))?;
        let target = text(&a, 1, "targetDir")?;
        check_jail(a.get(4), target)?;
        h.perform_asset_copy(library, raw, target, text(&a, 2, "stem")?, flag(&a, 3)).map(strings).map_err(|e| e.to_string())
    });
    with_host!("export.placerMerge", |_h, a| {
        let assets: Vec<PlacerAssetDto> = serde_json::from_value(json(arg(&a, 2, "assets")?)?).map_err(|e| format!("bad placer assets: {e}"))?;
        placer_merge_file(text(&a, 0, "libraryJsonPath")?, text(&a, 1, "subDirRes")?, assets).map(|_| Value::Null).map_err(|e| e.to_string())
    });
    with_host!("export.transcode", |_h, a| {
        let out = transcode_image(text(&a, 0, "fileName")?, bytes(&a, 1, "bytes")?.to_vec()).map_err(|e| e.to_string())?;
        Ok(Value::object([("mime", Value::String(out.mime)), ("bytes", Value::Bytes(out.bytes))]))
    });

    // --- services (services/pluginServices.ts) ---
    with_host!("svc.settingsGet", |h, a| {
        h.plugin_settings(text(&a, 0, "pluginId")?).map(|m| Value::from(Json::Object(m))).map_err(|e| e.to_string())
    });
    with_host!("svc.settingsSet", |h, a| {
        h.set_plugin_setting(text(&a, 0, "pluginId")?, text(&a, 1, "key")?, json(arg(&a, 2, "value")?)?).map(|_| Value::Null).map_err(|e| e.to_string())
    });
    with_host!("svc.toolLaunch", |h, a| {
        h.launch_plugin_tool(text(&a, 0, "pluginId")?, text(&a, 1, "setting")?, &string_list(&a, 2, "args")?).map(|_| Value::Null).map_err(|e| e.to_string())
    });
    with_host!("svc.toolRun", |h, a| {
        let timeout = number(&a, 3, "timeoutMs")?.max(0.0) as u64;
        let result = h.run_plugin_tool(text(&a, 0, "pluginId")?, text(&a, 1, "setting")?, &string_list(&a, 2, "args")?, timeout).map_err(|e| e.to_string())?;
        to_value(&result)
    });
    with_host!("svc.hostProcessId", |_h, _a| Ok(Value::from(plugin_services::host_process_id())));
    with_host!("svc.localOpen", |h, a| {
        h.plugin_local_port_open(text(&a, 0, "pluginId")?, number(&a, 1, "port")? as u16).map(Value::Bool).map_err(|e| e.to_string())
    });
    with_host!("svc.localJson", |h, a| {
        h.plugin_local_json(text(&a, 0, "pluginId")?, number(&a, 1, "port")? as u16, &json(arg(&a, 2, "message")?)?, number(&a, 3, "timeoutMs")? as u64)
            .map(Value::from)
            .map_err(|e| e.to_string())
    });
    with_host!("svc.scratch", |h, a| h.plugin_scratch_dir(text(&a, 0, "pluginId")?, flag(&a, 1)).map(Value::String).map_err(|e| e.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn qualified_ids_split_at_the_first_separator() {
        assert_eq!(parse_qualified("lib-1::uid://a::b"), Some(("lib-1", "uid://a::b")));
        assert_eq!(parse_qualified("::x"), None);
        assert_eq!(parse_qualified("plain"), None);
        assert_eq!(parse_qualified("bad lib::x"), None);
        assert_eq!(split("raw", "d"), ("d", "raw"));
    }

    #[test]
    fn assets_are_qualified_like_the_app_does() {
        let mut asset = json!({ "id": "a", "user": { "previewAssetId": "m" }, "kind": "animation", "clip": { "sourceId": "toybox:mannequin" },
            "origin": { "plugin": "p", "derivedFrom": "uid://substance_1" },
            "variants": [{ "id": "a2", "user": {}, "kind": "animation", "clip": { "sourceId": "s" }, "origin": { "plugin": "p", "derivedFrom": "L::uid://s" } }] });
        qualify_asset(&mut asset, "L");
        assert_eq!(asset["id"], "L::a");
        assert_eq!(asset["library"], "L");
        assert_eq!(asset["user"]["previewAssetId"], "L::m");
        assert_eq!(asset["clip"]["sourceId"], "toybox:mannequin");
        assert_eq!(asset["variants"][0]["id"], "L::a2");
        assert_eq!(asset["variants"][0]["clip"]["sourceId"], "L::s");
        assert_eq!(asset["origin"]["derivedFrom"], "L::uid://substance_1");
        assert_eq!(asset["variants"][0]["origin"]["derivedFrom"], "L::uid://s", "already qualified");
    }
}
