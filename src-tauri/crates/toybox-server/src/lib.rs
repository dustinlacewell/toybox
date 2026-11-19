//! The headless Toybox library server: one Toybox library folder served over
//! HTTP with token roles. The binary (`toybox-server`) is a thin command line
//! around this library; the tests drive the same router in process.
//!
//! Layout: `data` (the data directory), `config` (server.json and tokens),
//! `auth` + `limiter` (bearer tokens, lockout), `library` (the revision feed and
//! serialised writes through `toybox-core`), `uploads` (staging area),
//! `api` (routes), `cli` (commands).

pub mod api;
pub mod auth;
pub mod cli;
pub mod compat;
pub mod config;
pub mod data;
pub mod error;
pub mod library;
pub mod limiter;
pub mod state;
pub mod timeutil;
pub mod update;
pub mod uploads;

/// Version of the HTTP API (the `/api/v1` prefix is the route family; this number is
/// what a client compares). 2 added the revision feed (`GET /changes`) and the
/// write replies that carry what changed; clients that send no `X-Toybox-Api`
/// header still get the version 1 answers.
pub const API_VERSION: u32 = 2;
/// Version of this server program.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
