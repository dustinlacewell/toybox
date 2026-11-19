//! `tbx` end to end: the built binary against temporary homes and libraries. Nothing here
//! needs a plugin's code to run.

mod common;

use common::*;
use serde_json::{json, Value};

fn ids_of(run: &Run) -> Vec<String> {
    let mut ids = run.lines();
    ids.sort();
    ids
}

// ---- usage, errors, output rules ----------------------------------------------------

#[test]
fn wrong_usage_exits_2_with_a_one_line_error() {
    let world = World::new();
    let run = world.tbx(&["frobnicate"]).fails_with(2);
    assert!(run.err.starts_with("error: "), "{}", run.err);
    assert!(run.out.is_empty());
    let run = world.tbx(&["--json", "library", "rename"]).fails_with(2);
    let error: Value = serde_json::from_str(run.err.lines().next().unwrap()).unwrap();
    assert_eq!(error["error"]["code"], "usage");
    assert!(error["error"]["message"].as_str().unwrap().len() > 3);
    world.tbx(&["search", "--limit", "many"]).fails_with(2);
    world.tbx(&["tag", "add", "x"]).fails_with(2);
}

#[test]
fn a_command_with_nothing_registered_says_what_to_do() {
    let world = World::new();
    let run = world.tbx(&["search"]).fails_with(3);
    assert!(run.err.contains("error: no library is registered"), "{}", run.err);
    assert!(run.err.contains("hint: "), "{}", run.err);
}

#[test]
fn read_only_commands_create_nothing_on_a_machine_that_never_ran_toybox() {
    let world = World::new();
    for args in [&["status"][..], &["library", "list"], &["plugin", "list"], &["config", "list"], &["debug", "paths"], &["help", "--all"], &["completions", "bash"]] {
        world.tbx(args).ok();
    }
    world.tbx(&["search"]).fails_with(3);
    assert!(!world.home().exists(), "a read-only command created {}", world.home().display());
}

#[test]
fn the_json_flag_prints_one_document_and_quiet_prints_no_summaries() {
    let world = World::new();
    let (_, root) = world.library("Main");
    let run = world.tbx(&["--quiet", "library", "rename", "Main", "Primary"]).ok();
    assert!(run.out.is_empty() && run.err.is_empty(), "{:?} {:?}", run.out, run.err);
    let run = world.tbx(&["--json", "library", "rename", "Primary", "Main"]).ok();
    assert_eq!(run.json()["library"]["name"], "Main");
    assert!(root.is_dir());
}

// ---- libraries ----------------------------------------------------------------------

#[test]
fn libraries_are_created_listed_renamed_defaulted_and_removed() {
    let world = World::new();
    let (first, _) = world.library("Main");
    let second_dir = world.path("second");
    let run = world.tbx(&["library", "create", second_dir.to_str().unwrap(), "--name", "Second", "--default"]).ok();
    assert!(run.out.contains("Second"), "{}", run.out);
    let list = world.tbx(&["--json", "library", "list"]).ok().json();
    let libraries = list["libraries"].as_array().unwrap();
    assert_eq!(libraries.len(), 2);
    assert_eq!(libraries[1]["default"], true);
    assert_eq!(libraries[0]["kind"], "local");
    assert_eq!(libraries[0]["state"], "ok");
    let text = world.tbx(&["library", "list"]).ok();
    assert_eq!(text.lines().len(), 2);
    assert!(text.lines()[0].starts_with(&first));
    // A name works case-insensitively; an unknown one is exit 3.
    world.tbx(&["-L", "main", "library", "info"]).ok();
    world.tbx(&["-L", "nope", "library", "info"]).fails_with(3);
    world.tbx(&["library", "default", "MAIN"]).ok();
    let list = world.tbx(&["--json", "library", "list"]).ok().json();
    assert_eq!(list["libraries"][0]["default"], true);
    // Removing asks first; without a terminal it refuses unless --yes.
    let refused = world.tbx(&["library", "remove", "Second"]).fails_with(1);
    assert!(refused.err.contains("--yes"), "{}", refused.err);
    let dry = world.tbx(&["library", "remove", "Second", "--dry-run"]).ok();
    assert!(dry.out.contains("Would remove"));
    assert_eq!(world.tbx(&["--json", "library", "list"]).ok().json()["libraries"].as_array().unwrap().len(), 2);
    world.tbx(&["--yes", "library", "remove", "Second"]).ok();
    assert!(second_dir.is_dir(), "its files stay where they are");
    assert_eq!(world.tbx(&["--json", "library", "list"]).ok().json()["libraries"].as_array().unwrap().len(), 1);
    // A folder that is not a library cannot be added.
    let plain = world.path("plain");
    std::fs::create_dir_all(&plain).unwrap();
    world.tbx(&["library", "add", plain.to_str().unwrap()]).fails_with(1);
    // An existing library folder can.
    world.tbx(&["library", "add", second_dir.to_str().unwrap(), "--name", "Again"]).ok();
}

#[test]
fn library_info_scan_and_check_report_the_library() {
    let world = World::new();
    let (id, root) = world.stocked_library("Main");
    let info = world.tbx(&["--json", "library", "info"]).ok().json();
    assert_eq!(info["id"], id.as_str());
    assert_eq!(info["items"], 5);
    assert!(info["files"].as_u64().unwrap() >= 5);
    assert!(info["bytes"].as_u64().unwrap() > 0);
    let kinds: Vec<&str> = info["types"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"decal") && kinds.contains(&"model") && kinds.contains(&"audio"), "{kinds:?}");
    let text = world.tbx(&["library", "info"]).ok();
    assert!(text.out.contains("items     5"), "{}", text.out);
    let scan = world.tbx(&["--json", "library", "scan"]).ok().json();
    assert_eq!(scan["items"], 5);
    let check = world.tbx(&["--json", "library", "check"]).ok().json();
    assert_eq!(check["ok"], true);
    // Remove a file an item needs: check finds it and exits 4.
    let victim = walk(&root).into_iter().find(|path| path.file_name().is_some_and(|name| name == "Hut.bin")).expect("the model's buffer");
    std::fs::remove_file(victim).unwrap();
    let broken = world.tbx(&["--json", "library", "check"]).fails_with(4);
    let doc = broken.json();
    assert_eq!(doc["ok"], false);
    assert!(doc["problems"][0]["message"].as_str().unwrap().contains("missing file"), "{doc}");
    assert!(broken.err.contains("partial"), "{}", broken.err);
    let text = world.tbx(&["library", "check"]).fails_with(4);
    assert!(text.out.contains("missing file") && text.err.starts_with("error: 1 problem found"), "{} {}", text.out, text.err);
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() { out.extend(walk(&path)) } else { out.push(path) }
    }
    out
}

#[test]
fn cache_and_token_commands_need_a_remote_library() {
    let world = World::new();
    world.stocked_library("Main");
    world.tbx(&["library", "cache", "size"]).fails_with(2);
    world.tbx(&["library", "token", "list"]).fails_with(2);
    world.tbx(&["library", "login", "Main"]).fails_with(2);
}

// ---- finding and reading ------------------------------------------------------------

#[test]
fn search_lists_what_matches_with_ids_that_can_be_pasted_back() {
    let world = World::new();
    world.stocked_library("Main");
    // Without --type every type is searched.
    let all = world.tbx(&["search"]).ok();
    assert_eq!(all.lines().len(), 5, "{}", all.out);
    let ids = world.tbx(&["search", "--ids"]).ok();
    assert_eq!(ids_of(&ids), vec!["uid://barrel", "uid://beep", "uid://crate", "uid://hut", "uid://lamp"]);
    // Name order, case-insensitive, and --desc reverses it.
    assert_eq!(ids.lines(), vec!["uid://barrel", "uid://beep", "uid://crate", "uid://hut", "uid://lamp"]);
    let desc = world.tbx(&["search", "--ids", "--desc"]).ok();
    assert_eq!(desc.lines().first().unwrap(), "uid://lamp");
    let wood = world.tbx(&["search", "--type", "decal", "--tag", "wood", "--ids"]).ok();
    assert_eq!(wood.lines(), vec!["uid://barrel", "uid://crate"]);
    assert_eq!(world.tbx(&["search", "rat", "--ids"]).ok().lines(), vec!["uid://crate"]);
    assert_eq!(world.tbx(&["search", "--type", "decal", "--source", "Pack A", "--ids"]).ok().lines(), vec!["uid://crate"]);
    assert_eq!(world.tbx(&["search", "--format", "wav", "--ids"]).ok().lines(), vec!["uid://beep"]);
    let page = world.tbx(&["search", "--limit", "2", "--offset", "1", "--ids"]).ok();
    assert_eq!(page.lines(), vec!["uid://beep", "uid://crate"]);
    assert!(page.err.contains("showing 2 of 5"), "{}", page.err);
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "5");
    assert_eq!(world.tbx(&["count", "--type", "decal", "--tag", "wood"]).ok().out.trim(), "2");
    // The id column is exactly the id.
    let table = world.tbx(&["search", "Crate"]).ok();
    let row = table.lines().into_iter().next().unwrap();
    let mut cells = row.split_whitespace();
    assert_eq!(cells.next().unwrap(), "uid://crate");
    assert_eq!(cells.next().unwrap(), "decal");
    assert!(row.contains("Pack A") && row.contains("wood,prop"), "{row}");
}

#[test]
fn search_json_has_the_documented_shape() {
    let world = World::new();
    let (id, _) = world.stocked_library("Main");
    let doc = world.tbx(&["--json", "search", "--type", "decal", "--limit", "2"]).ok().json();
    assert_eq!(doc["total"], 3);
    assert_eq!(doc["exact"], true);
    assert_eq!(doc["offset"], 0);
    let items = doc["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["library"], id.as_str());
    assert_eq!(items[0]["asset"]["id"], "uid://barrel");
    assert_eq!(items[0]["asset"]["kind"], "plugin");
    assert_eq!(items[0]["asset"]["tags"], json!(["wood"]));
    assert_eq!(world.tbx(&["--json", "count", "--type", "model"]).ok().json(), json!({ "count": 1 }));
}

#[test]
fn reads_cover_every_library_unless_one_is_named() {
    let world = World::new();
    let (_, _) = world.stocked_library("Main");
    let (other, other_root) = world.library("Other");
    world.add_items(&other, &other_root, &[seed("uid://extra", "Extra", Kind::Decal, &["wood"], None)]);
    // No -L: everything, with a library column because there is more than one library.
    let all = world.tbx(&["search"]).ok();
    assert_eq!(all.lines().len(), 6, "{}", all.out);
    assert!(all.lines().iter().any(|line| line.starts_with("Other ")), "{}", all.out);
    assert!(all.lines().iter().any(|line| line.starts_with("Main ")), "{}", all.out);
    assert_eq!(world.tbx(&["search", "--ids", "extra"]).ok().lines(), vec!["uid://extra"], "a match in a library that is not the default is found");
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "6");
    assert_eq!(world.tbx(&["count", "--type", "decal", "--tag", "wood"]).ok().out.trim(), "3");
    // -L narrows, and the column stays while several libraries are registered.
    let one = world.tbx(&["-L", "Other", "search"]).ok();
    assert_eq!(one.lines().len(), 1);
    assert!(one.lines()[0].starts_with("Other "), "{}", one.out);
    assert_eq!(world.tbx(&["-L", "Main", "count"]).ok().out.trim(), "5");
    // Types, facets, tags and collections span them too.
    let types = world.tbx(&["--json", "types"]).ok().json();
    assert_eq!(types["types"].as_array().unwrap().iter().find(|t| t["id"] == "decal").unwrap()["count"], 4);
    assert_eq!(world.tbx(&["--json", "facets", "--type", "decal"]).ok().json()["total"], 4);
    assert_eq!(world.tbx(&["--json", "-L", "Other", "facets", "--type", "decal"]).ok().json()["total"], 1);
    let tags = world.tbx(&["--json", "tag", "list", "--type", "decal"]).ok().json();
    assert_eq!(tags["tags"].as_array().unwrap().iter().find(|t| t["value"] == "wood").unwrap()["count"], 3);
    world.tbx(&["-L", "Other", "favorite", "add", "uid://extra"]).ok();
    world.tbx(&["-L", "Other", "collection", "add", "Mine", "uid://extra"]).ok();
    assert_eq!(world.tbx(&["--json", "collection", "list"]).ok().json()["collections"], json!([{ "name": "Mine", "count": 1 }]));
    // The old flag is still accepted, hidden, and changes nothing.
    assert_eq!(world.tbx(&["search", "--all-libraries", "--ids"]).ok().lines().len(), 6);
    assert_eq!(world.tbx(&["--json", "count", "--all-libraries"]).ok().json()["count"], 6);
    assert!(!world.tbx(&["search", "--help"]).ok().out.contains("all-libraries"));
    // A library column is not shown with a single library.
    let single = World::new();
    single.stocked_library("Only");
    assert!(single.tbx(&["search", "Crate"]).ok().out.starts_with("uid://crate"));
}

#[test]
fn show_and_path_look_ids_up_across_libraries() {
    let world = World::new();
    let (_, _) = world.stocked_library("Main");
    let (other, other_root) = world.library("Other");
    world.add_items(&other, &other_root, &[seed("uid://extra", "Extra", Kind::Decal, &[], None)]);
    // An id in one library, no -L needed.
    let doc = world.tbx(&["--json", "show", "uid://extra"]).ok().json();
    assert_eq!(doc["items"][0]["asset"]["name"], "Extra");
    assert_eq!(doc["items"][0]["library"], other.as_str());
    assert!(world.tbx(&["show", "uid://extra"]).ok().out.contains("library      Other"));
    let path = world.tbx(&["path", "uid://extra"]).ok();
    assert!(std::path::PathBuf::from(path.out.trim()).starts_with(&other_root));
    // An id in two libraries is shown for each, labelled.
    world.tbx(&["copy", "uid://crate", "--to", "Other"]).ok();
    let both = world.tbx(&["--json", "show", "uid://crate"]).ok().json();
    let libraries: Vec<&str> = both["items"].as_array().unwrap().iter().map(|item| item["library"].as_str().unwrap()).collect();
    assert_eq!(libraries.len(), 2, "{both}");
    assert!(libraries.contains(&other.as_str()));
    let text = world.tbx(&["show", "uid://crate"]).ok();
    assert!(text.out.contains("library      Main") && text.out.contains("library      Other"), "{}", text.out);
    assert_eq!(world.tbx(&["--json", "path", "uid://crate"]).ok().json()["items"].as_array().unwrap().len(), 2);
    // -L picks one.
    assert_eq!(world.tbx(&["--json", "-L", "Main", "show", "uid://crate"]).ok().json()["items"].as_array().unwrap().len(), 1);
    // An id nowhere is exit 3, naming it.
    let missing = world.tbx(&["show", "uid://extra", "uid://nowhere"]).fails_with(3);
    assert!(missing.err.contains("uid://nowhere") && !missing.err.contains("uid://extra"), "{}", missing.err);
}

#[test]
fn a_change_acts_on_one_library_and_says_where_a_missing_id_is() {
    let world = World::new();
    let (_, _) = world.stocked_library("Main");
    let (other, other_root) = world.library("Other");
    world.add_items(&other, &other_root, &[seed("uid://extra", "Extra", Kind::Decal, &[], None)]);
    let run = world.tbx(&["tag", "add", "x", "uid://extra"]).fails_with(3);
    assert!(run.err.contains("error: no such item in library 'Main': uid://extra"), "{}", run.err);
    assert!(run.err.contains("hint: it is in library 'Other'; add -L Other"), "{}", run.err);
    world.tbx(&["-L", "Other", "tag", "add", "x", "uid://extra"]).ok();
    let nowhere = world.tbx(&["delete", "uid://nope", "--yes"]).fails_with(3);
    assert!(nowhere.err.contains("hint: ids are printed by `tbx search`"), "{}", nowhere.err);
}

#[test]
fn a_search_with_no_match_says_so_on_standard_error() {
    let world = World::new();
    world.stocked_library("Main");
    let run = world.tbx(&["search", "zzzzz"]).ok();
    assert_eq!(run.out, "");
    assert_eq!(run.err.trim(), "No items match.");
    let ids = world.tbx(&["search", "zzzzz", "--ids"]).ok();
    assert_eq!((ids.out.as_str(), ids.err.trim()), ("", "No items match."));
    let json = world.tbx(&["--json", "search", "zzzzz"]).ok();
    assert_eq!(json.err, "");
    assert_eq!(json.json()["total"], 0);
    assert_eq!(world.tbx(&["-q", "search", "zzzzz"]).ok().err, "");
}

#[test]
fn show_describes_items_and_reads_ids_from_standard_input() {
    let world = World::new();
    world.stocked_library("Main");
    let run = world.tbx(&["show", "uid://crate"]).ok();
    for expected in ["uid://crate", "Crate", "decal", "wood, prop", "Pack A", "a.png"] {
        assert!(run.out.contains(expected), "{expected} missing from\n{}", run.out);
    }
    let doc = world.tbx_stdin(&["--json", "show", "-"], "uid://crate\nuid://hut\n").ok().json();
    let items = doc["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1]["asset"]["id"], "uid://hut");
    assert!(items[0]["files"][0]["size"].as_u64().unwrap() > 0);
    // Unknown ids are exit 3 and are named.
    let missing = world.tbx(&["show", "uid://crate", "uid://nope", "uid://also"]).fails_with(3);
    assert!(missing.err.contains("uid://nope") && missing.err.contains("uid://also"), "{}", missing.err);
    assert!(missing.out.is_empty());
}

#[test]
fn path_prints_absolute_file_paths() {
    let world = World::new();
    let (_, root) = world.stocked_library("Main");
    let run = world.tbx(&["path", "uid://hut"]).ok();
    let path = std::path::PathBuf::from(run.out.trim());
    assert!(path.is_absolute() && path.is_file(), "{}", run.out);
    assert!(path.extension().is_some_and(|ext| ext == "gltf"));
    assert!(path.starts_with(&root));
    let all = world.tbx(&["path", "uid://hut", "--all"]).ok();
    assert!(all.lines().len() >= 2);
    let doc = world.tbx(&["--json", "path", "uid://crate", "uid://beep"]).ok().json();
    assert_eq!(doc["items"].as_array().unwrap().len(), 2);
}

#[test]
fn types_and_facets_count_what_is_there() {
    let world = World::new();
    world.stocked_library("Main");
    let types = world.tbx(&["--json", "types"]).ok().json();
    let by_id = |id: &str| types["types"].as_array().unwrap().iter().find(|t| t["id"] == id).cloned().unwrap_or_else(|| panic!("{id} missing from {types}"));
    assert_eq!(by_id("model")["count"], 1);
    assert_eq!(by_id("decal")["count"], 3);
    assert_eq!(by_id("audio")["count"], 1);
    assert_eq!(by_id("material")["count"], 0);
    let text = world.tbx(&["types"]).ok();
    assert!(text.out.contains("decal") && text.out.contains('3'), "{}", text.out);
    let facets = world.tbx(&["--json", "facets", "--type", "decal"]).ok().json();
    assert_eq!(facets["type"], "decal");
    assert_eq!(facets["total"], 3);
    let tags = facets["tags"].as_array().unwrap();
    let wood = tags.iter().find(|tag| tag["value"] == "wood").expect("wood");
    assert_eq!(wood["count"], 2);
    assert!(world.tbx(&["facets", "--type", "decal"]).ok().out.contains("sources"));
    world.tbx(&["facets"]).fails_with(2);
}

// ---- tags, details, favourites, collections -----------------------------------------

#[test]
fn tags_can_be_added_removed_set_and_listed() {
    let world = World::new();
    world.stocked_library("Main");
    let added = world.tbx(&["tag", "add", "old, Worn", "uid://crate", "uid://lamp"]).ok();
    assert!(added.out.contains("Tagged 2 items"), "{}", added.out);
    let show = world.tbx(&["--json", "show", "uid://crate"]).ok().json();
    assert_eq!(show["items"][0]["asset"]["tags"], json!(["wood", "prop", "old", "worn"]));
    // Already as asked is reported, not an error.
    let again = world.tbx(&["--json", "tag", "add", "wood", "uid://crate"]).ok().json();
    assert_eq!(again["changed"], json!([]));
    assert_eq!(again["unchanged"], json!(["uid://crate"]));
    world.tbx(&["tag", "remove", "old", "uid://crate"]).ok();
    assert_eq!(world.tbx(&["--json", "show", "uid://crate"]).ok().json()["items"][0]["asset"]["tags"], json!(["wood", "prop", "worn"]));
    world.tbx(&["tag", "set", "solo", "uid://crate"]).ok();
    assert_eq!(world.tbx(&["--json", "show", "uid://crate"]).ok().json()["items"][0]["asset"]["tags"], json!(["solo"]));
    world.tbx(&["tag", "set", "", "uid://crate"]).ok();
    // An item with no tags has no `tags` field.
    assert!(world.tbx(&["--json", "show", "uid://crate"]).ok().json()["items"][0]["asset"]["tags"].is_null());
    // By filter, and listing.
    world.tbx(&["tag", "add", "furniture", "--type", "decal", "--tag", "wood"]).ok();
    assert_eq!(world.tbx(&["search", "--tag", "furniture", "--ids"]).ok().lines(), vec!["uid://barrel"]);
    let list = world.tbx(&["--json", "tag", "list", "--type", "decal"]).ok().json();
    assert!(list["tags"].as_array().unwrap().iter().any(|t| t["value"] == "furniture"));
    assert!(world.tbx(&["tag", "list"]).ok().out.contains("wood"));
    world.tbx(&["tag", "add", ",", "uid://crate"]).fails_with(2);
}

#[test]
fn a_dry_run_changes_nothing() {
    let world = World::new();
    world.stocked_library("Main");
    let dry = world.tbx(&["tag", "add", "x", "uid://crate", "--dry-run"]).ok();
    assert!(dry.out.starts_with("Would tag 1 item"), "{}", dry.out);
    let doc = world.tbx(&["--json", "tag", "add", "x", "uid://crate", "--dry-run"]).ok().json();
    assert_eq!(doc["dryRun"], true);
    assert_eq!(doc["changed"], json!(["uid://crate"]));
    assert_eq!(world.tbx(&["search", "--tag", "x", "--ids"]).ok().out, "");
    world.tbx(&["delete", "uid://crate", "--dry-run"]).ok();
    world.tbx(&["edit", "uid://crate", "--source", "Z", "--dry-run"]).ok();
    world.tbx(&["favorite", "add", "uid://crate", "--dry-run"]).ok();
    world.tbx(&["collection", "add", "Set", "uid://crate", "--dry-run"]).ok();
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "5");
    assert_eq!(world.tbx(&["--json", "search", "--type", "decal", "--source", "Z"]).ok().json()["total"], 0);
    assert_eq!(world.tbx(&["search", "--favorites", "--type", "decal", "--ids"]).ok().out, "");
}

#[test]
fn details_can_be_edited_and_cleared() {
    let world = World::new();
    world.stocked_library("Main");
    world.tbx(&["edit", "uid://lamp", "--source", "Lamps Inc", "--publisher", "Pub", "--license", "CC0", "--url", "https://example.test/lamp"]).ok();
    let asset = world.tbx(&["--json", "show", "uid://lamp"]).ok().json()["items"][0]["asset"].clone();
    assert_eq!(asset["user"]["provenance"]["source"], "Lamps Inc");
    assert_eq!(asset["user"]["provenance"]["publisher"], "Pub");
    assert_eq!(asset["user"]["provenance"]["license"], "CC0");
    assert_eq!(asset["user"]["provenance"]["url"], "https://example.test/lamp");
    assert_eq!(world.tbx(&["search", "--source", "Lamps Inc", "--ids"]).ok().lines(), vec!["uid://lamp"]);
    world.tbx(&["edit", "uid://lamp", "--source", ""]).ok();
    let asset = world.tbx(&["--json", "show", "uid://lamp"]).ok().json()["items"][0]["asset"].clone();
    assert!(asset["user"]["provenance"].get("source").is_none(), "{asset}");
    assert_eq!(asset["user"]["provenance"]["publisher"], "Pub");
    world.tbx(&["edit", "uid://lamp"]).fails_with(2);
    world.tbx(&["edit", "uid://lamp", "--url", "not a url"]).fails_with(1);
    // `--source` here is the new value, so it cannot also filter; the other filters work.
    world.tbx(&["edit", "--type", "decal", "--tag", "wood", "--license", "MIT"]).ok();
    assert_eq!(world.tbx(&["search", "--license", "MIT", "--ids"]).ok().lines(), vec!["uid://barrel", "uid://crate"]);
}

#[test]
fn favorites_and_collections_are_personal_and_filterable() {
    let world = World::new();
    world.stocked_library("Main");
    world.tbx(&["favorite", "add", "uid://crate", "uid://hut"]).ok();
    assert_eq!(world.tbx(&["search", "--favorites", "--ids"]).ok().lines(), vec!["uid://crate", "uid://hut"]);
    world.tbx(&["favorite", "remove", "uid://hut"]).ok();
    assert_eq!(world.tbx(&["search", "--type", "model", "--favorites", "--ids"]).ok().out, "");
    world.tbx(&["collection", "add", "Kitchen", "uid://crate", "uid://barrel"]).ok();
    let list = world.tbx(&["--json", "collection", "list"]).ok().json();
    assert_eq!(list["collections"], json!([{ "name": "Kitchen", "count": 2 }]));
    assert_eq!(world.tbx(&["search", "--collection", "Kitchen", "--ids"]).ok().lines(), vec!["uid://barrel", "uid://crate"]);
    assert!(world.tbx(&["collection", "list", "--type", "decal"]).ok().out.contains("Kitchen"));
    world.tbx(&["collection", "remove", "Kitchen", "--type", "decal"]).ok();
    assert_eq!(world.tbx(&["--json", "collection", "list"]).ok().json()["collections"], json!([]));
    world.tbx(&["collection", "add", "  ", "uid://crate"]).fails_with(2);
}

#[test]
fn a_filter_needs_a_type_and_no_selection_is_a_usage_error() {
    let world = World::new();
    world.stocked_library("Main");
    world.tbx(&["tag", "add", "x", "--tag", "wood"]).fails_with(2);
    world.tbx(&["delete"]).fails_with(2);
    world.tbx(&["tag", "add", "x"]).fails_with(2);
    // With ids a filter narrows them.
    world.tbx(&["tag", "add", "narrow", "uid://crate", "uid://lamp", "--tag", "wood"]).ok();
    assert_eq!(world.tbx(&["search", "--tag", "narrow", "--ids"]).ok().lines(), vec!["uid://crate"]);
}

// ---- copy, move, delete -------------------------------------------------------------

#[test]
fn copy_and_move_between_libraries() {
    let world = World::new();
    world.stocked_library("Main");
    let (_, _) = world.library("Other");
    let dry = world.tbx(&["copy", "uid://crate", "--to", "Other", "--dry-run"]).ok();
    assert!(dry.out.contains("Would copy 1 item"), "{}", dry.out);
    assert_eq!(world.tbx(&["-L", "Other", "count"]).ok().out.trim(), "0");
    let run = world.tbx(&["--json", "copy", "uid://crate", "uid://hut", "--to", "Other"]).ok().json();
    assert_eq!(run["copied"], 2);
    assert_eq!(run["failed"], 0);
    assert_eq!(run["mode"], "copy");
    assert_eq!(world.tbx(&["-L", "Other", "search", "--ids"]).ok().lines(), vec!["uid://crate", "uid://hut"]);
    assert_eq!(world.tbx(&["-L", "Main", "count"]).ok().out.trim(), "5", "a copy leaves the source alone");
    // Copying again skips what is there; --on-conflict replace replaces it.
    let again = world.tbx(&["--json", "copy", "uid://crate", "--to", "Other"]).ok().json();
    assert_eq!((again["copied"].as_u64(), again["skipped"].as_u64()), (Some(0), Some(1)));
    let replaced = world.tbx(&["--json", "copy", "uid://crate", "--to", "Other", "--on-conflict", "replace"]).ok().json();
    assert_eq!(replaced["replaced"], 1);
    world.tbx(&["copy", "uid://crate", "--to", "Main"]).fails_with(2);
    world.tbx(&["copy", "uid://crate", "--to", "Nowhere"]).fails_with(3);
    // A move asks first; --yes goes ahead and removes only what arrived.
    world.tbx(&["move", "uid://lamp", "--to", "Other"]).fails_with(1);
    assert_eq!(world.tbx(&["-L", "Main", "count"]).ok().out.trim(), "5");
    let moved = world.tbx(&["--yes", "--json", "move", "uid://lamp", "--to", "Other"]).ok().json();
    assert_eq!((moved["copied"].as_u64(), moved["removed"].as_u64()), (Some(1), Some(1)));
    assert_eq!(world.tbx(&["-L", "Main", "count"]).ok().out.trim(), "4");
    assert_eq!(world.tbx(&["-L", "Other", "count"]).ok().out.trim(), "3");
}

#[test]
fn delete_asks_first_and_removes_items_and_files() {
    let world = World::new();
    let (_, root) = world.stocked_library("Main");
    let refused = world.tbx(&["delete", "uid://crate"]).fails_with(1);
    assert!(refused.err.contains("--yes"), "{}", refused.err);
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "5");
    let dry = world.tbx(&["--json", "delete", "uid://crate", "--dry-run"]).ok().json();
    assert_eq!(dry["deleted"], json!(["uid://crate"]));
    assert_eq!(dry["dryRun"], true);
    let files_before = walk(&root).len();
    let done = world.tbx(&["--yes", "--json", "delete", "uid://crate", "uid://hut"]).ok().json();
    assert_eq!(done["deleted"], json!(["uid://crate", "uid://hut"]));
    assert_eq!(done["remaining"], 3);
    assert!(walk(&root).len() < files_before);
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "3");
    world.tbx(&["--yes", "delete", "uid://crate"]).fails_with(3);
    // By filter, ids from standard input.
    world.tbx_stdin(&["--yes", "delete", "-"], "uid://lamp\n").ok();
    world.tbx(&["--yes", "delete", "--type", "decal"]).ok();
    assert_eq!(world.tbx(&["search", "--ids"]).ok().lines(), vec!["uid://beep"]);
}

#[test]
fn thumbs_status_and_clear() {
    let world = World::new();
    world.stocked_library("Main");
    let status = world.tbx(&["--json", "thumbs", "status"]).ok().json();
    assert_eq!(status["items"], 5);
    assert_eq!(status["pending"], 5);
    world.tbx(&["thumbs", "clear"]).fails_with(1);
    assert!(world.tbx(&["thumbs", "clear", "--dry-run"]).ok().out.contains("Would discard every thumbnail"));
    world.tbx(&["--yes", "thumbs", "clear", "uid://crate"]).ok();
    world.tbx(&["--yes", "thumbs", "clear"]).ok();
}

// ---- export -------------------------------------------------------------------------

#[test]
fn export_copy_writes_files_and_lists_them() {
    let world = World::new();
    world.stocked_library("Main");
    let out = world.path("out");
    let dry = world.tbx(&["export", "copy", "uid://hut", "--to", out.to_str().unwrap(), "--dry-run"]).ok();
    assert!(dry.lines().iter().any(|line| line.ends_with(".gltf")), "{}", dry.out);
    assert!(!out.exists());
    let run = world.tbx(&["--json", "export", "copy", "uid://hut", "uid://crate", "--to", out.to_str().unwrap()]).ok().json();
    let written = run["written"].as_array().unwrap();
    assert!(written.len() >= 3, "{run}");
    for path in written {
        assert!(out.join(path.as_str().unwrap()).is_file(), "{path}");
    }
    assert_eq!(run["preview"], false);
    // A bad id is not exported at all.
    world.tbx(&["export", "copy", "uid://nope", "--to", out.to_str().unwrap()]).fails_with(3);
    world.tbx(&["export", "copy", "uid://hut"]).fails_with(2);
    // The exporters are listed.
    let list = world.tbx(&["--json", "export"]).ok().json();
    let names: Vec<&str> = list["exporters"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"copy") && names.contains(&"glb"), "{names:?}");
    assert!(world.tbx(&["export"]).ok().out.contains("glb"));
}

#[test]
fn export_glb_packs_a_model() {
    let world = World::new();
    world.stocked_library("Main");
    let out = world.path("out-glb");
    let run = world.tbx(&["--json", "export", "glb", "uid://hut", "--to", out.to_str().unwrap()]).ok().json();
    let written = run["written"].as_array().unwrap();
    assert_eq!(written.len(), 1, "{run}");
    assert!(written[0].as_str().unwrap().ends_with(".glb"));
    assert!(out.join(written[0].as_str().unwrap()).is_file());
    // A non-model cannot be a .glb.
    world.tbx(&["export", "glb", "uid://beep", "--to", out.to_str().unwrap()]).fails_with(1);
}

// ---- import (listing only: running an importer needs plugin code) -------------------

#[test]
fn import_without_an_importer_lists_them() {
    let world = World::new();
    world.stocked_library("Main");
    let run = world.tbx(&["--json", "import"]).ok().json();
    assert!(run["importers"].is_array());
    world.tbx(&["import", "no-such-importer", "somewhere"]).fails_with(2);
}

// ---- plugins and settings -----------------------------------------------------------

#[test]
fn plugins_can_be_installed_switched_and_removed() {
    let world = World::new();
    let fixture = world.plugin_fixture("test.decals");
    let run = world.tbx(&["--json", "plugin", "install", fixture.to_str().unwrap()]).ok().json();
    assert_eq!(run["id"], "test.decals");
    assert_eq!(run["linked"], false);
    assert!(world.home().join("plugins").join("test.decals").join("manifest.json").is_file());
    world.tbx(&["plugin", "install", fixture.to_str().unwrap()]).fails_with(1);
    let list = world.tbx(&["--json", "plugin", "list"]).ok().json();
    let plugins = list["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0]["state"], "on");
    assert_eq!(plugins[0]["version"], "1.2.3");
    assert!(plugins[0]["provides"].as_array().unwrap().iter().any(|p| p == "types"));
    assert!(world.tbx(&["plugin", "list"]).ok().out.starts_with("test.decals"));
    let info = world.tbx(&["--json", "plugin", "info", "test.decals"]).ok().json();
    assert_eq!(info["types"], json!(["decal"]));
    assert_eq!(info["settings"][0]["key"], "engine");
    assert!(world.tbx(&["plugin", "info", "test.decals"]).ok().out.contains("Decals for tests"));
    world.tbx(&["plugin", "info", "nope"]).fails_with(3);
    let off = world.tbx(&["--json", "plugin", "disable", "test.decals"]).ok().json();
    assert_eq!(off["changed"], json!(["test.decals"]));
    assert_eq!(world.tbx(&["--json", "plugin", "list"]).ok().json()["plugins"][0]["state"], "off");
    assert!(world.tbx(&["plugin", "disable", "test.decals"]).ok().out.contains("already off"));
    world.tbx(&["plugin", "enable", "nope"]).fails_with(3);
    world.tbx(&["plugin", "enable", "test.decals"]).ok();
    world.tbx(&["plugin", "remove", "test.decals"]).fails_with(1);
    world.tbx(&["--yes", "plugin", "remove", "test.decals"]).ok();
    assert!(!world.home().join("plugins").join("test.decals").exists());
    assert_eq!(world.tbx(&["--json", "plugin", "list"]).ok().json()["plugins"], json!([]));
    world.tbx(&["plugin", "install", world.path("missing").to_str().unwrap()]).fails_with(1);
}

#[test]
fn a_linked_plugin_leaves_its_folder_alone_when_removed() {
    let world = World::new();
    let fixture = world.plugin_fixture("test.linked");
    world.tbx(&["plugin", "install", fixture.to_str().unwrap(), "--link"]).ok();
    assert!(world.tbx(&["--json", "plugin", "list"]).ok().json()["plugins"][0]["id"] == "test.linked");
    world.tbx(&["--yes", "plugin", "remove", "test.linked"]).ok();
    assert!(fixture.join("manifest.json").is_file(), "the folder a link pointed at must survive");
}

#[test]
fn items_of_a_plugin_that_is_off_are_hidden_as_in_the_app() {
    let world = World::new();
    world.stocked_library("Main");
    let fixture = world.plugin_fixture("test.decals");
    world.tbx(&["plugin", "install", fixture.to_str().unwrap()]).ok();
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "5");
    world.tbx(&["plugin", "disable", "test.decals"]).ok();
    // The decal type is hidden: its items are not listed or counted.
    assert_eq!(world.tbx(&["search", "--ids"]).ok().lines(), vec!["uid://beep", "uid://hut"]);
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "2");
    assert_eq!(world.tbx(&["search", "--include-hidden", "--ids"]).ok().lines().len(), 5);
    assert_eq!(world.tbx(&["count", "--include-hidden"]).ok().out.trim(), "5");
    let types = world.tbx(&["--json", "types"]).ok().json();
    let decal = types["types"].as_array().unwrap().iter().find(|t| t["id"] == "decal").unwrap().clone();
    assert_eq!(decal["hidden"], true);
    assert_eq!(decal["plugin"], "test.decals");
    // Selecting by filter sees what the app shows.
    world.tbx(&["--yes", "delete", "--type", "decal", "--dry-run"]).ok();
    world.tbx(&["plugin", "enable", "test.decals"]).ok();
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "5");
}

#[test]
fn plugin_settings_can_be_shown_set_and_unset() {
    let world = World::new();
    let fixture = world.plugin_fixture("test.decals");
    world.tbx(&["plugin", "install", fixture.to_str().unwrap()]).ok();
    assert_eq!(world.tbx(&["plugin", "settings", "test.decals", "engine"]).ok().out.trim(), "none");
    world.tbx(&["plugin", "settings", "test.decals", "engine", "D:/engine.exe"]).ok();
    assert_eq!(world.tbx(&["plugin", "settings", "test.decals", "engine"]).ok().out.trim(), "D:/engine.exe");
    let all = world.tbx(&["--json", "plugin", "settings", "test.decals"]).ok().json();
    assert_eq!(all["settings"][0]["value"], "D:/engine.exe");
    assert_eq!(all["settings"][0]["default"], "none");
    world.tbx(&["plugin", "settings", "test.decals", "engine", "--unset"]).ok();
    assert_eq!(world.tbx(&["plugin", "settings", "test.decals", "engine"]).ok().out.trim(), "none");
    world.tbx(&["plugin", "settings", "test.decals", "nonsense", "x"]).fails_with(3);
    world.tbx(&["plugin", "settings", "nope"]).fails_with(3);
}

#[test]
fn config_keys_can_be_set_read_and_cleared() {
    let world = World::new();
    assert_eq!(world.tbx(&["config", "get", "fbx2gltf-path"]).ok().out, "");
    world.tbx(&["config", "set", "fbx2gltf-path", "D:/tools/FBX2glTF.exe"]).ok();
    assert_eq!(world.tbx(&["config", "get", "fbx2gltf-path"]).ok().out.trim(), "D:/tools/FBX2glTF.exe");
    assert_eq!(world.tbx(&["--json", "config", "list"]).ok().json(), json!({ "settings": { "fbx2gltf-path": "D:/tools/FBX2glTF.exe" } }));
    world.tbx(&["config", "unset", "fbx2gltf-path"]).ok();
    assert_eq!(world.tbx(&["--json", "config", "get", "fbx2gltf-path"]).ok().json(), json!({ "key": "fbx2gltf-path", "value": null }));
    world.tbx(&["config", "get", "nonsense"]).fails_with(2);
}

// ---- status, help, debug ------------------------------------------------------------

#[test]
fn status_summarises_data_libraries_plugins_and_problems() {
    let world = World::new();
    let (_, root) = world.stocked_library("Main");
    let doc = world.tbx(&["--json", "status"]).ok().json();
    assert_eq!(doc["libraries"].as_array().unwrap().len(), 1);
    assert_eq!(doc["libraries"][0]["items"], 5);
    assert_eq!(doc["problems"], json!([]));
    assert!(doc["paths"]["exists"].as_bool().unwrap());
    let text = world.tbx(&["status"]).ok();
    assert!(text.out.contains("Main") && text.out.contains("libraries"), "{}", text.out);
    // A library whose folder went away is reported, and status still succeeds.
    std::fs::rename(&root, world.path("moved")).unwrap();
    let doc = world.tbx(&["--json", "status"]).ok().json();
    assert_eq!(doc["libraries"][0]["state"], "missing");
    assert!(doc["problems"][0].as_str().unwrap().contains("missing"));
    world.tbx(&["search"]).fails_with(1);
}

#[test]
fn help_covers_every_command_and_every_command_has_clean_help() {
    let world = World::new();
    let all = world.tbx(&["--json", "help", "--all"]).ok().json();
    let commands: Vec<(String, String)> = all["commands"].as_array().unwrap().iter().map(|c| (c["command"].as_str().unwrap().to_string(), c["summary"].as_str().unwrap().to_string())).collect();
    for expected in ["status", "library add", "library token revoke", "search", "tag add", "edit", "import", "export copy", "export glb", "copy", "move", "delete", "thumbs clear", "plugin install", "config set", "debug sql", "completions"] {
        assert!(commands.iter().any(|(name, _)| name == expected), "{expected} missing from help --all");
    }
    for (name, summary) in &commands {
        assert!(!summary.is_empty(), "{name} has no summary");
        assert!(!summary.ends_with('.') || summary.matches('.').count() > 1, "{name}: summaries are one line without a final full stop: {summary}");
        let mut args: Vec<&str> = vec!["help"];
        args.extend(name.split(' '));
        let help = world.tbx(&args).ok();
        assert!(help.out.contains("Usage"), "{name}: {}", help.out);
        let mut args: Vec<&str> = name.split(' ').collect();
        args.push("--help");
        world.tbx(&args).ok();
    }
    let text = world.tbx(&["help", "--all"]).ok();
    assert!(text.out.contains("tbx library add"), "{}", text.out);
    world.tbx(&["help", "nonsense"]).fails_with(2);
    world.tbx(&["--help"]).ok();
    world.tbx(&["--version"]).ok();
}

#[test]
fn completions_exist_for_every_shell() {
    let world = World::new();
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let run = world.tbx(&["completions", shell]).ok();
        assert!(run.out.contains("tbx") && run.out.len() > 500, "{shell}");
    }
    world.tbx(&["completions", "tcsh"]).fails_with(2);
}

#[test]
fn debug_paths_and_sql() {
    let world = World::new();
    world.stocked_library("Main");
    let paths = world.tbx(&["--json", "debug", "paths"]).ok().json();
    assert!(paths["appData"].as_str().unwrap().ends_with("home"));
    assert!(paths["libraries"][0]["database"].as_str().unwrap().ends_with("library.db"));
    assert!(paths["secrets"].as_str().unwrap().contains("TOYBOX_SECRETS"));
    let sql = world.tbx(&["--json", "debug", "sql", "select count(*) as n from assets"]).ok().json();
    assert_eq!(sql["columns"], json!(["n"]));
    assert_eq!(sql["rows"], json!([[5]]));
    let text = world.tbx(&["debug", "sql", "select kind, count(*) from assets group by kind order by kind"]).ok();
    assert!(text.out.contains("plugin"), "{}", text.out);
    world.tbx(&["debug", "sql", "delete from assets"]).fails_with(1);
    world.tbx(&["debug", "sql", "select 1; select 2"]).fails_with(1);
    assert_eq!(world.tbx(&["count"]).ok().out.trim(), "5");
}

#[test]
fn the_library_can_come_from_the_environment() {
    let world = World::new();
    world.stocked_library("Main");
    let (other, other_root) = world.library("Other");
    world.add_items(&other, &other_root, &[seed("uid://solo", "Solo", Kind::Decal, &[], None)]);
    let mut command = world.command(&["search", "--ids"]);
    command.env("TOYBOX_LIBRARY", "Other");
    assert_eq!(world.run(command, None).ok().lines(), vec!["uid://solo"]);
}

// ---- kept source files ----------------------------------------------------------------

fn material_with_archive(root: &std::path::Path, name: &str, archive: &[u8]) -> serde_json::Value {
    put(root, &format!("library/_import_s/materials/{name}/color.png"), name.as_bytes());
    put(root, &format!("library/_import_s/materials/{name}/stone.sbsar"), archive);
    json!({"id": format!("uid://{name}"), "kind": "material", "pack": "_import_s", "category": "stone", "name": name,
        "maps": [{"role": "albedo", "file": "color.png"}],
        "origin": {"plugin": "com.example.sbsar", "files": ["stone.sbsar"], "meta": {"sbsar": "stone.sbsar", "preset": name}}})
}

#[test]
fn kept_sources_are_stored_once_listed_apart_counted_and_checked() {
    let world = World::new();
    let (id, root) = world.library("Main");
    let archive = vec![42u8; 50_000];
    let entries = vec![material_with_archive(&root, "Green", &archive), material_with_archive(&root, "Red", &archive)];
    world.host().merge_seed_entries(&id, entries).unwrap();

    // `show` keeps the item's own files apart from the sources, and both items name one stored copy.
    let green = world.tbx(&["--json", "show", "uid://Green"]).ok().json();
    let item = &green["items"][0];
    let sources = item["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    let path = sources[0]["path"].as_str().unwrap();
    assert!(path.starts_with("library/materials/_sources/") && path.ends_with("/stone.sbsar"), "{path}");
    assert_eq!(sources[0]["size"], 50_000);
    assert!(item["files"].as_array().unwrap().iter().all(|file| !file["path"].as_str().unwrap().contains("_sources")));
    let red = world.tbx(&["--json", "show", "uid://Red"]).ok().json();
    assert_eq!(red["items"][0]["sources"][0]["path"], path);
    let text = world.tbx(&["show", "uid://Green"]).ok();
    assert!(text.out.contains("\nsources ") && text.out.contains("stone.sbsar"), "{}", text.out);

    // `library info` says how much the sharing saves.
    let info = world.tbx(&["--json", "library", "info"]).ok().json();
    assert_eq!(info["sources"]["files"], 1);
    assert_eq!(info["sources"]["bytes"], 50_000);
    assert_eq!(info["sources"]["savedBytes"], 50_000);
    assert!(world.tbx(&["library", "info"]).ok().out.contains("1 kept source files"));
    assert_eq!(world.tbx(&["--json", "library", "check"]).ok().json()["ok"], true);

    // A source nobody keeps is reported, and `--fix` removes it.
    let stray = root.join("library/materials/_sources").join("a".repeat(64));
    std::fs::create_dir_all(&stray).unwrap();
    std::fs::write(stray.join("old.sbsar"), b"left behind").unwrap();
    let check = world.tbx(&["--json", "library", "check"]).ok().json();
    assert_eq!(check["ok"], true);
    assert_eq!(check["orphanSources"].as_array().unwrap().len(), 1);
    let fixed = world.tbx(&["--json", "library", "check", "--fix"]).ok().json();
    assert_eq!(fixed["removedSources"].as_array().unwrap().len(), 1);
    assert!(!stray.exists());
    assert!(root.join(path).is_file(), "a source in use stays");

    // A missing source is a problem; a damaged one too.
    std::fs::write(root.join(path), b"damaged").unwrap();
    let damaged = world.tbx(&["--json", "library", "check"]).fails_with(4).json();
    assert!(damaged["problems"][0]["message"].as_str().unwrap().contains("damaged"), "{damaged}");
    std::fs::remove_file(root.join(path)).unwrap();
    let missing = world.tbx(&["--json", "library", "check"]).fails_with(4).json();
    assert!(missing["problems"][0]["message"].as_str().unwrap().contains("missing source file"), "{missing}");
}
