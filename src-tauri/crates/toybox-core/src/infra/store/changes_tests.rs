//! The revision feed, the thumbnail log, tombstone retention and what a replica
//! keeps. Everything here reads or writes rows through the store only.

use rusqlite::Connection;
use serde_json::{json, Value};

use super::changes::{PAGE_FROM_CURSOR, PAGE_FROM_REVISION};
use super::tests::{catalog, manifest_doc, rooted, Tmp};
use super::*;
use crate::domain::catalog_model::{Asset, Catalog};

/// A library with the five-kind fixture committed at revision 1.
fn library(tag: &str) -> (Tmp, Store, Catalog) {
    let tmp = Tmp::new(tag);
    let store = tmp.store();
    let catalog = rooted(catalog(), &store);
    store.commit(CatalogWrite { manifest: Some(&manifest_doc()), catalog: Some(&catalog), ..Default::default() }).unwrap();
    assert_eq!(store.stamp().unwrap().revision, 1);
    (tmp, store, catalog)
}

fn query(since: i64, limit: usize) -> ChangesQuery<'static> {
    ChangesQuery { since, after: None, limit, db_id: None }
}

fn ids(page: &ChangesPage) -> Vec<String> {
    let mut ids: Vec<String> = page.assets.iter().map(|asset| asset.id.clone()).collect();
    ids.sort();
    ids
}

#[test]
fn the_feed_pages_every_change_from_zero_and_reads_through_the_revision_index() {
    let (_tmp, store, _) = library("feed");
    let all = store.changes_page(&query(0, 100)).unwrap();
    assert_eq!(ids(&all), ["clip:walk", "uid://audio", "uid://mat", "uid://model", "uid://sky"]);
    assert!(!all.more && !all.reset && all.after.is_none());
    assert_eq!((all.next, all.revision, all.since), (1, 1, 0));
    assert!(!all.db_id.is_empty());
    assert_eq!(all.rigs.iter().map(|rig| rig.id.as_str()).collect::<Vec<_>>(), ["humanoid"], "the rig the model binds travels with it");
    let model = all.assets.iter().find(|asset| asset.id == "uid://model").unwrap();
    assert_eq!(model.variants.len(), 1, "variants come inside their parent");
    assert!(!model.user.favorite && model.user.collections.is_empty() && model.user.preview_asset_id.is_none(), "no personal data in the feed");

    // Small pages walk the same rows, each exactly once, and say where they stopped.
    let mut seen: Vec<String> = Vec::new();
    let (mut since, mut after) = (0, None::<String>);
    let mut pages = 0;
    loop {
        let page = store.changes_page(&ChangesQuery { since, after: after.as_deref(), limit: 2, db_id: Some(&all.db_id) }).unwrap();
        pages += 1;
        seen.extend(page.assets.iter().map(|asset| asset.id.clone()));
        (since, after) = (page.next, page.after.clone());
        if !page.more {
            assert_eq!((page.next, page.after), (1, None));
            break;
        }
        assert!(page.after.is_some());
    }
    assert_eq!(pages, 3);
    seen.sort();
    assert_eq!(seen, ids(&all));

    // The queries use the index; neither scans the table.
    let connection = Connection::open(store.library_path()).unwrap();
    for (sql, params) in [(PAGE_FROM_REVISION, vec![json!(0), json!(10)]), (PAGE_FROM_CURSOR, vec![json!(1), json!(0), json!("uid://a"), json!(10)])] {
        let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let bound: Vec<rusqlite::types::Value> = params
            .iter()
            .map(|value| match value {
                Value::String(text) => rusqlite::types::Value::Text(text.clone()),
                other => rusqlite::types::Value::Integer(other.as_i64().unwrap()),
            })
            .collect();
        let plan: Vec<String> = statement.query_map(rusqlite::params_from_iter(bound), |row| row.get::<_, String>(3)).unwrap().map(Result::unwrap).collect();
        let plan = plan.join(" | ");
        assert!(plan.contains("changes_revision"), "{plan}");
        assert!(!plan.contains("SCAN changes") && !plan.contains("TEMP B-TREE"), "{plan}");
    }
}

#[test]
fn an_edit_a_delete_and_an_addition_are_each_only_the_rows_they_touch() {
    let (_tmp, store, base) = library("rows");
    // An edit of one item's library tags.
    let mut edited = base.clone();
    edited.assets.iter_mut().find(|asset| asset.id == "uid://mat").unwrap().tags = vec!["granite".into()];
    store.save_catalog(&edited).unwrap();
    let page = store.changes_page(&query(1, 100)).unwrap();
    assert_eq!(ids(&page), ["uid://mat"]);
    assert_eq!(page.assets[0].tags, ["granite"]);
    assert!(page.deleted.is_empty());
    assert_eq!((page.next, page.revision), (2, 2));

    // A removal.
    let applied = store.apply(&Delta { remove_items: vec!["uid://audio".into()], ..Default::default() }).unwrap();
    assert_eq!(applied.committed.revision, 3);
    let page = store.changes_page(&query(2, 100)).unwrap();
    assert!(page.assets.is_empty());
    assert_eq!(page.deleted, ["uid://audio"]);

    // An addition (a copy of the sky under another id).
    let mut grown = edited.clone();
    grown.assets.retain(|asset| asset.id != "uid://audio");
    let mut extra = grown.assets.iter().find(|asset| asset.id == "uid://sky").unwrap().clone();
    extra.id = "uid://sky2".into();
    grown.assets.push(extra);
    store.save_catalog(&grown).unwrap();
    let page = store.changes_page(&query(3, 100)).unwrap();
    assert_eq!(ids(&page), ["uid://sky2"]);

    // Everything since the start: the latest state of each id (the removal is still told).
    let page = store.changes_page(&query(0, 100)).unwrap();
    assert_eq!(page.deleted, ["uid://audio"]);
    assert_eq!(page.assets.len(), 5);
}

#[test]
fn within_a_revision_puts_come_before_removals() {
    let (_tmp, store, base) = library("order");
    // One commit that adds a row whose id sorts after the removed one's, and removes another.
    let mut next = base.clone();
    next.assets.retain(|asset| asset.id != "uid://audio");
    let mut extra = next.assets.iter().find(|asset| asset.id == "uid://sky").unwrap().clone();
    extra.id = "zzz".into();
    next.assets.push(extra);
    store.save_catalog(&next).unwrap();
    let rows: Vec<(String, i64)> = {
        let connection = Connection::open(store.library_path()).unwrap();
        let mut statement = connection.prepare(PAGE_FROM_REVISION).unwrap();
        let found = statement.query_map([1, 10], |row| Ok((row.get::<_, String>(2)?, row.get::<_, i64>(1)?))).unwrap();
        found.map(Result::unwrap).collect()
    };
    assert_eq!(rows, [("zzz".to_string(), 0), ("uid://audio".to_string(), 1)], "the put of 'zzz' is read before the removal of 'uid://audio'");
}

#[test]
fn a_client_is_reset_when_the_server_cannot_answer_for_its_cursor() {
    let (_tmp, store, base) = library("reset");
    let db_id = store.db_id();
    // A deletion at revision 2, then some quiet commits.
    store.apply(&Delta { remove_items: vec!["uid://audio".into()], ..Default::default() }).unwrap();
    let mut catalog = base.clone();
    catalog.assets.retain(|asset| asset.id != "uid://audio");
    for round in 0..3 {
        catalog.assets.iter_mut().find(|asset| asset.id == "uid://mat").unwrap().tags = vec![format!("round{round}")];
        store.save_catalog(&catalog).unwrap();
    }
    assert_eq!(store.stamp().unwrap().revision, 5);

    let page = store.changes_page(&ChangesQuery { since: 1, after: None, limit: 100, db_id: Some(&db_id) }).unwrap();
    assert!(!page.reset);
    assert_eq!(page.deleted, ["uid://audio"], "still told: the tombstone is kept");

    // Another database (the library was replaced): start again.
    let foreign = store.changes_page(&ChangesQuery { since: 1, after: None, limit: 100, db_id: Some("some-other-database") }).unwrap();
    assert!(foreign.reset && foreign.assets.is_empty() && foreign.deleted.is_empty());
    assert_eq!(foreign.db_id, db_id, "the reset names the database to start from");
    // A cursor from the future (the library was restored from a backup).
    assert!(store.changes_page(&query(99, 100)).unwrap().reset);
    // A client with no database id yet is just starting.
    assert!(!store.changes_page(&query(1, 100)).unwrap().reset);

    // Tombstones are kept for a number of revisions, then pruned; older cursors are reset.
    store.prune_changes(2).unwrap();
    let count = |deleted: i64| -> i64 { Connection::open(store.library_path()).unwrap().query_row("SELECT COUNT(*) FROM changes WHERE deleted=?1", [deleted], |row| row.get(0)).unwrap() };
    assert_eq!(count(1), 0, "the tombstone from revision 2 is older than 2 revisions");
    assert_eq!(count(0), 4, "live rows are never pruned");
    let floor: i64 = store.meta("changesFloor").unwrap().parse().unwrap();
    assert_eq!(floor, 3);
    assert!(store.changes_page(&query(2, 100)).unwrap().reset, "a deletion since revision 2 might have been forgotten");
    assert!(!store.changes_page(&query(3, 100)).unwrap().reset);
    assert!(!store.changes_page(&query(0, 100)).unwrap().reset, "a full download needs no tombstones");
    assert_eq!(store.changes_page(&query(0, 100)).unwrap().assets.len(), 4);
}

#[test]
fn a_commit_prunes_old_tombstones_on_its_own() {
    let (_tmp, store, base) = library("prune");
    store.apply(&Delta { remove_items: vec!["uid://audio".into()], ..Default::default() }).unwrap();
    let connection = Connection::open(store.library_path()).unwrap();
    let tombstones = || -> i64 { connection.query_row("SELECT COUNT(*) FROM changes WHERE deleted=1", [], |row| row.get(0)).unwrap() };
    assert_eq!(tombstones(), 1);
    // Pruning looks only at tombstones, through the partial index.
    let plan: Vec<String> = connection
        .prepare("EXPLAIN QUERY PLAN DELETE FROM changes WHERE deleted=1 AND revision<=5")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(plan.join(" | ").contains("changes_tombstones"), "{plan:?}");
    // Pretend the library has seen many commits since.
    connection.execute("UPDATE meta SET value=?1 WHERE key='revision'", [(CHANGES_KEEP_REVISIONS + 5).to_string()]).unwrap();
    let mut catalog = base.clone();
    catalog.assets.retain(|asset| asset.id != "uid://audio");
    catalog.assets.iter_mut().find(|asset| asset.id == "uid://mat").unwrap().tags = vec!["after".into()];
    store.save_catalog(&catalog).unwrap();
    assert_eq!(tombstones(), 0);
    assert!(store.meta("changesFloor").unwrap().parse::<i64>().unwrap() > 2);
}

#[test]
fn a_fingerprint_that_arrives_is_a_change_of_the_rows_that_show_it() {
    let (_tmp, store, base) = library("hash");
    let model = base.assets.iter().find(|asset| asset.id == "uid://model").unwrap();
    let own = crate::ops::content_hash::storage_key(model).unwrap();
    let variant = crate::ops::content_hash::storage_key(&model.variants[0]).unwrap();
    let path = store.library_path().to_path_buf();
    let revision = || store.stamp().unwrap().revision;

    let pending: Vec<String> = store.assets_to_hash().unwrap().iter().map(|asset| asset.id.clone()).collect();
    assert!(pending.contains(&"uid://model".to_string()));
    hashes::update_keys(&path, &[(own.clone(), "own-hash".into())], &[]).unwrap();
    assert_eq!(revision(), 2, "a hash that rows show moves the library revision");
    let page = store.changes_page(&query(1, 100)).unwrap();
    assert_eq!(ids(&page), ["uid://model"]);
    assert_eq!(page.assets[0].content_hash.as_deref(), Some("own-hash"));
    assert_eq!(page.assets[0].variants[0].content_hash, None);
    assert!(store.assets_to_hash().unwrap().iter().any(|asset| asset.id == "uid://model"), "its variant still has none");

    hashes::update_keys(&path, &[(variant.clone(), "variant-hash".into())], &[]).unwrap();
    let page = store.changes_page(&query(2, 100)).unwrap();
    assert_eq!(ids(&page), ["uid://model"], "a variant's hash changes its parent's row");
    assert_eq!(page.assets[0].variants[0].content_hash.as_deref(), Some("variant-hash"));
    assert!(!store.assets_to_hash().unwrap().iter().any(|asset| asset.id == "uid://model"));

    // A key no row uses does not touch the revision; forgetting a key is a change again.
    hashes::update_keys(&path, &[("models/unused".into(), "x".into())], &[]).unwrap();
    assert_eq!(revision(), 3);
    hashes::update_keys(&path, &[], &[own]).unwrap();
    assert_eq!(revision(), 4);
    let page = store.changes_page(&query(3, 100)).unwrap();
    assert_eq!(page.assets[0].content_hash, None);
    // Pending pages cost what they return.
    let (first, next) = store.pending_hash_assets("", 1).unwrap();
    assert_eq!(first.len(), 1);
    assert!(next.is_some());
}

#[test]
fn the_thumbnail_log_is_apart_from_the_library_revision() {
    let (_tmp, store, _) = library("thumbs");
    assert_eq!(store.thumbs_since(0).unwrap(), (vec![], 0));
    assert_eq!(store.note_thumbs(&["uid://model".into(), "uid://mat".into()]).unwrap(), 1);
    assert_eq!(store.note_thumbs(&["uid://model".into()]).unwrap(), 2);
    assert_eq!(store.stamp().unwrap().revision, 1, "a picture is not a library change");
    let (since_zero, revision) = store.thumbs_since(0).unwrap();
    assert_eq!((since_zero.len(), revision), (2, 2));
    assert_eq!(store.thumbs_since(1).unwrap().0, ["uid://model"], "only what was saved after revision 1");
    assert_eq!(store.status().unwrap().thumb_revision, 2);
    // Removing the item drops its row.
    store.apply(&Delta { remove_items: vec!["uid://mat".into()], ..Default::default() }).unwrap();
    assert_eq!(store.thumbs_since(0).unwrap().0, ["uid://model"]);
}

#[test]
fn a_replica_can_be_emptied_and_gets_its_personal_data_back_by_id() {
    let tmp = Tmp::new("replica");
    let local = tmp.store();
    let replica = Store::remote_at(&tmp.root(), &tmp.0.join("replica/catalog.json"));
    let served = {
        let mut catalog = rooted(catalog(), &local);
        catalog.assets.truncate(1);
        let mut model = catalog.assets.remove(0);
        model.user = Default::default();
        model.thumb = Default::default();
        model.variants.iter_mut().for_each(|variant| {
            variant.user = Default::default();
            variant.thumb = Default::default();
        });
        model
    };
    let mine = |favorite: bool| -> Asset {
        let mut asset = served.clone();
        asset.user.favorite = favorite;
        asset.user.collections = vec!["Zoo".into()];
        asset.user.tags = vec!["mine".into()];
        asset.variants[0].user.favorite = true;
        asset
    };
    replica.apply(&Delta { puts: vec![ItemPut { id: served.id.clone(), entry: None, assets: vec![mine(true)], rigs: vec![] }], set_meta: vec![("catalogSchema".into(), crate::config::SCHEMA_VERSION.to_string())], ..Default::default() }).unwrap();
    assert!(replica.catalog_exists());
    assert!(replica.clear_replica().is_ok());
    assert!(!replica.catalog_exists() && replica.asset_count().unwrap() == 0);
    assert!(local.clear_replica().is_err(), "a library with a manifest is not a replica");

    // What a server sends has none of it; the replica's user database has it by id.
    let back = replica.overlay_personal(vec![served.clone()]).unwrap();
    assert!(back[0].user.favorite && back[0].user.collections == ["Zoo"] && back[0].user.tags.contains(&"mine".to_string()));
    assert!(back[0].variants[0].user.favorite, "variants too");
}

#[test]
fn the_ids_with_a_picture_are_found_from_the_file_names_alone() {
    let (_tmp, store, _) = library("thumb-names");
    let all = store.all_ids().unwrap();
    assert!(all.len() >= 5);
    // The names of the pictures are the sanitised ids: looking the names up gives the ids back (variants included), in id order.
    let names: Vec<String> = all.iter().map(|id| crate::infra::appdata::sanitize_id(id)).collect();
    assert_eq!(store.ids_by_thumb_name(&names).unwrap(), all);
    assert_eq!(store.ids_by_thumb_name(&names[..2]).unwrap().len(), 2);
    assert!(store.ids_by_thumb_name(&["no-such-picture".to_string()]).unwrap().is_empty());
    assert!(store.ids_by_thumb_name(&[]).unwrap().is_empty());
}
