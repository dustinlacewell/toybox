//! Adding a remote library, roles, plain-language errors, token management,
//! offline behaviour and where the token does (not) end up.

mod common;

use std::io::{Read, Write};
use std::net::TcpListener;

use common::*;
use toybox_core::infra::library::{remote_capabilities, RemoteRole};
use toybox_core::infra::registry::LibraryKind;
use toybox_remote::client::Client;
use toybox_remote::hub::prepare;
use toybox_remote::RemoteError;
use toybox_server::config::Role;

/// A server that answers every request with the same bytes.
fn fake_server(response: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(response);
        }
    });
    format!("http://{addr}")
}

#[test]
fn adding_a_library_shows_the_server_name_and_maps_each_role() {
    let server = TestServer::start();
    for (token, role) in [
        (server.token("friend", Role::Read), RemoteRole::Read),
        (server.token("editor", Role::Write), RemoteRole::Write),
        (server.admin.clone(), RemoteRole::Admin),
    ] {
        let prepared = prepare(&server.url, &token, None).unwrap();
        assert_eq!(prepared.probe.role, role);
        assert_eq!(prepared.probe.server_name, "Test server");
        assert_eq!(prepared.name, "Test server", "the library is named after the server unless the user chose a name");
        assert!(prepared.probe.warning.is_some(), "a plain http address on this computer carries the unencrypted warning");
    }
    assert_eq!(prepare(&server.url, &server.admin, Some("  My assets ")).unwrap().name, "My assets");
}

#[test]
fn a_wrong_token_a_bad_address_and_a_dead_server_are_each_explained() {
    let server = TestServer::start();
    let wrong = prepare(&server.url, "tbx_notatoken", None).unwrap_err();
    assert_eq!(wrong, RemoteError::BadToken);
    assert!(wrong.to_string().contains("did not accept this access token"));
    assert!(!wrong.to_string().contains("tbx_notatoken"));

    assert!(prepare(&server.url, "   ", None).unwrap_err().to_string().contains("Enter the access token"));
    assert!(prepare("ftp://example.com", "x", None).unwrap_err().to_string().contains("https://"));
    assert!(prepare("http://example.com", "x", None).unwrap_err().to_string().contains("Use an https:// address"));
    assert!(prepare("", "x", None).unwrap_err().to_string().contains("Enter the address"));

    // Nothing listens there.
    let free = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let dead = prepare(&format!("http://{free}"), "tbx_x", None).unwrap_err();
    assert!(dead.is_connectivity(), "{dead:?}");
    assert!(dead.to_string().contains("Cannot reach the server"), "{dead}");

    // Something answers, but it is not a Toybox server: never a panic, always a sentence.
    for response in [
        &b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello"[..],
        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 20\r\nConnection: close\r\n\r\n<html>bad gateway</h",
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 12\r\nConnection: close\r\n\r\n{\"name\": 12}",
        b"HTTP/1.1 301 Moved\r\nLocation: https://elsewhere.example.com/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        b"not http at all\r\n\r\n",
    ] {
        let fake = fake_server(response);
        let error = prepare(&fake, "tbx_x", None).unwrap_err();
        let text = error.to_string();
        assert!(text.len() > 10 && !text.contains("tbx_x"), "{text}");
    }
}

#[test]
fn a_locked_out_address_says_when_to_try_again() {
    // The shared test server has an effectively unlimited allowance; this one is strict.
    let mut options = toybox_server::state::Options::default();
    options.fail_delay = std::time::Duration::from_millis(1);
    options.log_requests = false;
    options.lockout.max_failures = 2;
    let dir = unique_dir("strict");
    toybox_server::data::init(&toybox_server::data::DataDir::new(&dir), "Strict").unwrap();
    let state = toybox_server::state::AppState::open(toybox_server::data::DataDir::new(&dir), options).unwrap();
    let app = toybox_server::api::router(state);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            let _ = axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await;
        });
    });
    let addr = rx.recv().unwrap();
    let client = Client::new(&format!("http://{addr}"), "tbx_wrong");
    for _ in 0..2 {
        assert_eq!(client.info().unwrap_err(), RemoteError::BadToken);
    }
    let locked = client.info().unwrap_err();
    assert!(matches!(locked, RemoteError::LockedOut { retry_after: Some(seconds) } if seconds > 0), "{locked:?}");
    assert!(locked.to_string().contains("Try again in"), "{locked}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_token_goes_to_the_credential_store_and_nowhere_else() {
    let server = TestServer::start();
    let token = server.token("me", Role::Write);
    let mut world = World::new("secrets");
    let library = world.add_loaded(&server, &token, "Remote");
    let id = library.id().to_string();

    assert_eq!(world.store.accounts(), vec![id.clone()], "the secret is filed under the library id");
    let entry = &world.settings.libraries[0];
    assert_eq!(entry.kind, LibraryKind::Remote);
    assert_eq!(entry.url.as_deref(), Some(server.url.as_str()));

    // Every file the client wrote (settings, remote.json, the catalog cache, thumbnails, the file cache).
    fn walk(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() { walk(&path, found) } else { found.push(path) }
        }
    }
    let mut files = Vec::new();
    walk(&world.dir, &mut files);
    assert!(files.iter().any(|p| p.ends_with("settings.json")) && files.iter().any(|p| p.ends_with("remote.json")));
    for file in files {
        let text = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).to_string();
        assert!(!text.contains(&token) && !text.contains("tbx_"), "{} holds a token", file.display());
    }
    let settings = std::fs::read_to_string(world.settings_path()).unwrap();
    assert!(settings.contains("\"kind\": \"remote\"") && settings.contains(&server.url));

    // Neither errors nor debug output carry it.
    let revoked = Client::new(&server.url, &token);
    let text = format!("{revoked:?} {:?}", library);
    assert!(!text.contains(&token));
    let bad = Client::new(&server.url, &format!("{token}x")).info().unwrap_err();
    assert!(!format!("{bad} {bad:?}").contains(&token));
}

#[test]
fn a_credential_store_that_does_not_work_blocks_adding_with_a_clear_message() {
    let server = TestServer::start();
    let mut world = World::new("nostore");
    *world.store.broken.lock().unwrap() = true;
    let prepared = prepare(&server.url, &server.admin, None).unwrap();
    let error = world.hub.register(&mut world.settings, &prepared, &server.admin).unwrap_err();
    assert!(error.to_string().contains("credential store"), "{error}");
    assert!(world.settings.libraries.is_empty(), "nothing is registered without a safe place for the token");
    assert!(world.store.accounts().is_empty());
}

#[test]
fn what_a_library_may_do_follows_its_role_and_whether_the_server_is_reachable() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("caps");
    let read = server.token("read", Role::Read);
    let write = server.token("write", Role::Write);
    let reader = world.add_loaded(&server, &read, "Read");
    let writer = world.add_loaded(&server, &write, "Write");
    let admin = world.add_loaded(&server, &server.admin.clone(), "Admin");
    assert_eq!(remote_capabilities(&reader.status()), (true, false, false));
    assert_eq!(remote_capabilities(&writer.status()), (true, true, false));
    assert_eq!(remote_capabilities(&admin.status()), (true, true, false), "files are never edited in place on a remote library");

    // As the interface is told about them.
    let registry = toybox_core::infra::library::Registry { libraries: world.settings.libraries.clone(), default: None };
    let infos = registry.list_with(&|entry| Some(world.hub.status(entry)));
    let json = serde_json::to_value(&infos).unwrap();
    assert_eq!(json[0]["remote"]["role"], "read");
    assert_eq!(json[1]["remote"]["role"], "write");
    assert_eq!(json[2]["remote"]["role"], "admin");
    assert_eq!(json[0]["remote"]["serverName"], "Test server");
    assert_eq!(json[0]["canWrite"], false);
    assert_eq!(json[1]["canWrite"], true);
    assert_eq!(json[1]["canEditFiles"], false);
    assert_eq!(json[0]["kind"], "remote");
    assert_eq!(json[0]["offline"], false);

    // The server going away makes everything read-only, but the library stays usable.
    drop(server);
    let status = writer.connect();
    assert!(status.offline && status.offline_reason.as_deref().is_some_and(|r| r.contains("Cannot reach the server")));
    let infos = registry.list_with(&|entry| Some(world.hub.status(entry)));
    let json = serde_json::to_value(&infos).unwrap();
    assert_eq!(json[1]["available"], true);
    assert_eq!(json[1]["offline"], true);
    assert_eq!(json[1]["canWrite"], false);
    assert_eq!(json[1]["remote"]["role"], "write", "the last known role is kept");
    assert!(json[1]["offlineReason"].as_str().unwrap().contains("Cannot reach"));
}

#[test]
fn offline_uses_the_cached_catalog_and_coming_back_restores_writing() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("offline");
    let token = server.token("me", Role::Write);
    let library = world.add_loaded(&server, &token, "Remote");
    let before: Vec<String> = library.refresh_catalog(false).unwrap().assets.iter().map(|a| a.id.clone()).collect();
    assert_eq!(before.len(), 5);

    let mut server = server;
    server.shutdown();
    let catalog = library.refresh_catalog(false).expect("the cache is served while offline");
    assert_eq!(catalog.assets.len(), 5);
    assert!(library.is_offline() && !library.can_write());
    assert!(library.require_write().unwrap_err().to_string().contains("offline"));
    // A file that was never downloaded cannot be had.
    let gltf = catalog.assets.iter().find(|a| a.id == "uid://barrel").unwrap().fileset().unwrap().gltf.clone();
    let error = library.handle().ensure_files(&[gltf]).unwrap_err().to_string();
    assert!(error.contains("Cannot reach the server"), "{error}");

    let server = server.restart();
    let status = library.connect();
    assert!(!status.offline, "{status:?}");
    assert!(library.can_write());
    assert_eq!(library.refresh_catalog(false).unwrap().assets.len(), 5);
    drop(server);
}

#[test]
fn with_no_cache_an_unreachable_server_is_an_error_not_an_empty_library() {
    let mut server = TestServer::start();
    let mut world = World::new("nocache");
    let library = world.add(&server, &server.admin.clone(), "Remote");
    server.shutdown();
    let error = library.refresh_catalog(false).unwrap_err();
    assert!(error.is_connectivity(), "{error:?}");
    let caps = remote_capabilities(&library.status());
    assert!(!caps.0, "never reached, nothing cached: not available");
}

#[test]
fn an_admin_manages_tokens_and_the_last_admin_is_protected() {
    let server = TestServer::start();
    let mut world = World::new("tokens");
    let admin = world.add(&server, &server.admin.clone(), "Admin");
    let client = admin.client();

    let listed = client.tokens().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].name.as_str(), listed[0].role), ("admin", RemoteRole::Admin));
    assert!(listed[0].created_at > 0);

    let made = client.create_token("alice laptop", RemoteRole::Read).unwrap();
    assert!(made.token.starts_with("tbx_") && made.view.role == RemoteRole::Read);
    assert!(!format!("{made:?}").contains(&made.token), "the secret is not printed");
    let names: Vec<String> = client.tokens().unwrap().into_iter().map(|t| t.name).collect();
    assert_eq!(names, vec!["admin", "alice laptop"]);
    // The new token works, with the role it was made with.
    assert_eq!(prepare(&server.url, &made.token, None).unwrap().probe.role, RemoteRole::Read);
    // And what the first request did shows as "last used".
    assert!(client.tokens().unwrap().iter().any(|t| t.name == "alice laptop" && t.last_used_at.is_some()));

    // Names are unique; a read token cannot manage tokens.
    let duplicate = client.create_token("Alice Laptop", RemoteRole::Read).unwrap_err();
    assert_eq!(duplicate.code(), "name_taken", "{duplicate:?}");
    let friend = Client::new(&server.url, &made.token);
    let refused = friend.tokens().unwrap_err();
    assert!(matches!(refused, RemoteError::Forbidden(_)) && refused.to_string().contains("admin role"), "{refused}");
    assert!(friend.create_token("x", RemoteRole::Read).is_err());

    // Revoking stops the token at once.
    client.revoke_token(&made.view.id).unwrap();
    assert_eq!(prepare(&server.url, &made.token, None).unwrap_err(), RemoteError::BadToken);
    assert!(client.revoke_token(&made.view.id).is_err(), "already gone");

    // The only admin token cannot be revoked; the reason is stated.
    let only = client.tokens().unwrap().remove(0);
    let last = client.revoke_token(&only.id).unwrap_err();
    assert_eq!(last.code(), "last_admin");
    assert!(last.to_string().to_lowercase().contains("admin"), "{last}");
    // With a second admin it can go.
    let second = client.create_token("backup", RemoteRole::Admin).unwrap();
    client.revoke_token(&only.id).unwrap();
    assert_eq!(client.info().unwrap_err(), RemoteError::BadToken, "the revoked token no longer works");
    assert_eq!(Client::new(&server.url, &second.token).tokens().unwrap().len(), 1);
}

#[test]
fn changing_the_token_keeps_the_library_and_a_revoked_token_is_reported() {
    let server = TestServer::start();
    server.seed();
    let mut world = World::new("change");
    let old = server.token("old", Role::Write);
    let library = world.add_loaded(&server, &old, "Remote");
    // The owner revokes it.
    let admin = Client::new(&server.url, &server.admin);
    let id = admin.tokens().unwrap().into_iter().find(|t| t.name == "old").unwrap().id;
    admin.revoke_token(&id).unwrap();

    let status = library.connect();
    assert!(status.offline && status.offline_reason.unwrap().contains("did not accept this access token"));
    assert!(!library.can_write());
    assert_eq!(library.refresh_catalog(false).unwrap().assets.len(), 5, "the cached catalog is still shown");

    // A new token, checked before it is kept.
    let fresh = server.token("new", Role::Read);
    assert!(prepare(&server.url, "tbx_wrong", None).is_err());
    world.hub.replace_token(library.id(), &fresh).unwrap();
    let status = library.connect();
    assert!(!status.offline);
    assert_eq!(library.role(), RemoteRole::Read, "the role now is the new token's");
    assert_eq!(world.store.accounts().len(), 1);
}
