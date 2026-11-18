//! The HTTP API, under `/api/v1`. Version 2 (the revision feed) is negotiated per
//! request with the `X-Toybox-Api` header; without it a client gets the version 1
//! answers it was written for. See docs/server.md and docs/sqlite-storage.md.

use std::time::{Instant, UNIX_EPOCH};

use axum::body::{Body, Bytes};
use axum::extract::rejection::QueryRejection;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put, MethodRouter};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use toybox_core::ops::files::{self, RelPathError};

use crate::auth::{self, Principal, TokenName};
use crate::config::{hex, random_hex, ConfigError, Role, TokenRecord};
use crate::error::{ApiError, ApiJson};
use crate::library::{path_error, MAX_ENTRIES};
use toybox_core::infra::store::ChangesPage;
use crate::state::AppState;
use crate::timeutil::fmt_utc;
use crate::{API_VERSION, SERVER_VERSION};

/// Largest JSON body for a commit (entries can be numerous).
const COMMIT_BODY_LIMIT: usize = 32 * 1024 * 1024;
const MAX_DELETE_IDS: usize = 10_000;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// Rows in a page of `/changes` when the client names no limit, and the most it may ask for.
const DEFAULT_PAGE: usize = 1000;
const MAX_PAGE: usize = 5000;
/// Most changed rows a write's reply carries; beyond it the client pulls the rest.
const REPLY_ROWS: usize = 500;

/// The API level a request speaks, from the `X-Toybox-Api` header. Clients that
/// predate the revision feed send none and get the version 1 shapes (string
/// revisions, a catalog in the answer to a delete); version 2 gets integer
/// revisions and the rows a write changed.
#[derive(Debug, Clone, Copy)]
struct Level(u32);

impl Level {
    fn feed(self) -> bool {
        self.0 >= 2
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Level {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let level = parts.headers.get("x-toybox-api").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(1);
        Ok(Level(level))
    }
}

pub fn router(state: AppState) -> Router {
    let authed = Router::new()
        .route("/api/v1/info", get(info))
        .route("/api/v1/catalog", get(catalog))
        .route("/api/v1/changes", get(changes))
        .route("/api/v1/files/{*path}", get(get_file))
        .route("/api/v1/thumbs", get(list_thumbs))
        .route(
            "/api/v1/thumbs/{id}",
            get(get_thumb).merge(guarded(Role::Write, put(put_thumb).layer(DefaultBodyLimit::disable()))),
        )
        .route("/api/v1/uploads", guarded(Role::Write, post(create_upload)))
        .route("/api/v1/uploads/{id}", guarded(Role::Write, delete(delete_upload)))
        .route(
            "/api/v1/uploads/{id}/files/{*path}",
            guarded(Role::Write, put(put_upload_file).layer(DefaultBodyLimit::disable())),
        )
        .route(
            "/api/v1/uploads/{id}/commit",
            guarded(Role::Write, post(commit_upload).layer(DefaultBodyLimit::max(COMMIT_BODY_LIMIT))),
        )
        .route("/api/v1/assets/delete", guarded(Role::Write, post(delete_assets)))
        .route(
            "/api/v1/assets/metadata",
            guarded(Role::Write, post(update_metadata).layer(DefaultBodyLimit::max(COMMIT_BODY_LIMIT))),
        )
        .route("/api/v1/assets/export", guarded(Role::Read, post(export_assets)))
        .route(
            "/api/v1/uploads/{id}/package/{*path}",
            guarded(Role::Write, put(put_package_file).layer(DefaultBodyLimit::disable())),
        )
        .route(
            "/api/v1/uploads/{id}/ingest",
            guarded(Role::Write, post(ingest_upload).layer(DefaultBodyLimit::max(COMMIT_BODY_LIMIT))),
        )
        .route(
            "/api/v1/tokens",
            guarded(Role::Admin, get(list_tokens).post(create_token)),
        )
        .route("/api/v1/tokens/{id}", guarded(Role::Admin, delete(delete_token)))
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(state.clone(), auth::authenticate));
    Router::new()
        .route("/api/v1/health", get(health))
        .merge(authed)
        .layer(middleware::from_fn_with_state(state.clone(), log_requests))
        .with_state(state)
}

/// Require a role before the handler (and before its body is read).
fn guarded(role: Role, route: MethodRouter<AppState>) -> MethodRouter<AppState> {
    route.route_layer(middleware::from_fn(move |request: Request, next: Next| async move {
        match request.extensions().get::<Principal>() {
            Some(principal) if principal.role >= role => next.run(request).await,
            Some(_) => ApiError::forbidden(role).into_response(),
            None => ApiError::unauthorized("Send your access token as: Authorization: Bearer <token>").into_response(),
        }
    }))
}

async fn not_found() -> ApiError {
    ApiError::not_found("There is nothing at this address.")
}

async fn log_requests(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if !state.options.log_requests {
        return next.run(request).await;
    }
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let ip = auth::client_ip(&request);
    let started = Instant::now();
    let response = next.run(request).await;
    let token = response.extensions().get::<TokenName>().map(|t| t.0.clone()).unwrap_or_else(|| "-".to_string());
    println!(
        "ts={} method={} path={:?} status={} token={:?} ip={} ms={}",
        fmt_utc(crate::timeutil::now_secs()),
        method,
        path,
        response.status().as_u16(),
        token,
        ip,
        started.elapsed().as_millis()
    );
    response
}

// ---------------------------------------------------------------- helpers

fn etag_matches(headers: &HeaderMap, etag: &str) -> bool {
    let Some(value) = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) else { return false };
    let bare = etag.trim_start_matches("W/");
    value.split(',').map(str::trim).any(|candidate| candidate == "*" || candidate.trim_start_matches("W/") == bare)
}

fn not_modified(etag: &str, cache_control: &'static str) -> Response {
    let mut response = StatusCode::NOT_MODIFIED.into_response();
    set_header(&mut response, header::ETAG, etag);
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    response
}

fn set_header(response: &mut Response, name: header::HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        response.headers_mut().insert(name, value);
    }
}

fn json_bytes(status: StatusCode, etag: Option<&str>, body: Bytes) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    if let Some(etag) = etag {
        set_header(&mut response, header::ETAG, etag);
    }
    response
}

fn config_error(error: ConfigError) -> ApiError {
    match &error {
        ConfigError::InvalidName(_) => ApiError::bad_request("invalid_name", error.to_string()),
        ConfigError::DuplicateName(_) => ApiError::conflict("name_taken", error.to_string()),
        ConfigError::NotFound(_) => ApiError::not_found("There is no token with that id."),
        ConfigError::LastAdmin => ApiError::conflict("last_admin", error.to_string()),
        ConfigError::Io(_) | ConfigError::Parse(_) => ApiError::internal(error),
    }
}

fn token_view(token: &TokenRecord) -> Value {
    json!({
        "id": token.id,
        "name": token.name,
        "role": token.role,
        "createdAt": token.created_at,
        "lastUsedAt": token.last_used_at,
    })
}

// ----------------------------------------------------------------- routes

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn info(State(state): State<AppState>, principal: Principal, level: Level) -> Result<Json<Value>, ApiError> {
    let library = state.library.state().await?;
    let name = state.config.snapshot().map(|c| c.name.clone()).unwrap_or_default();
    Ok(Json(json!({
        "name": name,
        "serverVersion": SERVER_VERSION,
        "apiVersion": API_VERSION,
        "role": principal.role,
        // A number for clients that follow the feed, the string they always had for the others.
        "revision": if level.feed() { json!(library.revision) } else { json!(library.legacy_revision()) },
        "dbId": library.db_id,
        "thumbRevision": library.thumb_revision,
    })))
}

/// The whole catalog, for clients of API version 1. It is built when asked for and
/// kept for its revision; nothing a write does builds it.
async fn catalog(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    let catalog = state.library.legacy_catalog().await?;
    if etag_matches(&headers, &catalog.etag) {
        return Ok(not_modified(&catalog.etag, "private, no-cache"));
    }
    Ok(json_bytes(StatusCode::OK, Some(&catalog.etag), catalog.body.clone()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangesParams {
    #[serde(default)]
    since: i64,
    after: Option<String>,
    limit: Option<usize>,
    db_id: Option<String>,
}

fn bad_query(rejection: QueryRejection) -> ApiError {
    ApiError::bad_request("bad_query", format!("The query is not valid: {}", rejection.body_text()))
}

/// What changed in the library after revision `since`, oldest first, in pages.
/// `since=0` is a full, paged download. See `ChangesPage` for the answer.
async fn changes(State(state): State<AppState>, params: Result<Query<ChangesParams>, QueryRejection>) -> Result<Response, ApiError> {
    let Query(params) = params.map_err(bad_query)?;
    if params.since < 0 {
        return Err(ApiError::bad_request("bad_query", "since must be a revision number, 0 or more."));
    }
    if params.after.as_deref().is_some_and(|after| !(after.starts_with("0:") || after.starts_with("1:")) || after.len() < 3) {
        return Err(ApiError::bad_request("bad_cursor", "after must be the cursor of an earlier answer."));
    }
    let limit = params.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    let page = state.library.changes(params.since, params.after, limit, params.db_id).await?;
    let body = serde_json::to_vec(&page).map_err(ApiError::internal)?;
    Ok(json_bytes(StatusCode::OK, None, Bytes::from(body)))
}

/// The page that goes into a write's reply, or none if it could not be read (the
/// client then pulls, which is always correct).
async fn reply_page(state: &AppState, info: crate::library::WriteInfo) -> Option<ChangesPage> {
    state.library.reply_changes(info, REPLY_ROWS).await.ok()
}

// ------------------------------------------------------------------ files

/// What a client may download: the managed media under `library/`. The
/// manifest and everything else in `_library_config`, and files of imports
/// that have not been committed (`library/_import_*`), are never served.
fn readable(rel: &str) -> bool {
    let mut parts = rel.split('/');
    parts.next() == Some("library") && parts.next().is_some_and(|second| !second.starts_with("_import_")) && rel.split('/').count() >= 3
}

/// Paths whose name is a content hash never change, so clients may keep them forever.
fn immutable(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    let hashy = |s: &str| matches!(s.len(), 16 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit());
    match parts.as_slice() {
        ["library", _, key, ..] if hashy(key) => true,
        ["library", "models", "_resources", file] => file.split('.').next().is_some_and(hashy),
        _ => false,
    }
}

fn content_type(rel: &str) -> &'static str {
    let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "gltf" => "model/gltf+json",
        "glb" => "model/gltf-binary",
        "bin" => "application/octet-stream",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "tga" => "image/x-tga",
        "hdr" => "image/vnd.radiance",
        "exr" => "image/x-exr",
        "ktx2" => "image/ktx2",
        "json" => "application/json",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "flac" => "audio/flac",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RangeSpec {
    Full,
    Partial(u64, u64),
    Unsatisfiable,
}

/// Interpret a `Range` header for a file of `len` bytes. Only a single byte
/// range is honoured; anything else is answered with the whole file.
pub fn parse_range(header: Option<&str>, len: u64) -> RangeSpec {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else { return RangeSpec::Full };
    if len == 0 || spec.contains(',') {
        return RangeSpec::Full;
    }
    let Some((first, last)) = spec.trim().split_once('-') else { return RangeSpec::Full };
    if first.is_empty() {
        return match last.trim().parse::<u64>() {
            Ok(0) => RangeSpec::Unsatisfiable,
            Ok(n) => RangeSpec::Partial(len.saturating_sub(n), len - 1),
            Err(_) => RangeSpec::Full,
        };
    }
    let Ok(start) = first.trim().parse::<u64>() else { return RangeSpec::Full };
    if start >= len {
        return RangeSpec::Unsatisfiable;
    }
    let end = if last.trim().is_empty() {
        len - 1
    } else {
        match last.trim().parse::<u64>() {
            Ok(end) if end >= start => end.min(len - 1),
            _ => return RangeSpec::Full,
        }
    };
    RangeSpec::Partial(start, end)
}

fn modified_nanos(meta: &std::fs::Metadata) -> u128 {
    meta.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0)
}

async fn get_file(State(state): State<AppState>, Path(path): Path<String>, headers: HeaderMap) -> Result<Response, ApiError> {
    let rel = files::clean_rel(&path).map_err(path_error)?;
    if !readable(&rel) {
        return Err(ApiError::not_found("There is no such file."));
    }
    let root = state.library.handle.root.clone();
    let lookup = rel.clone();
    let real = tokio::task::spawn_blocking(move || -> Result<_, RelPathError> {
        let real = files::resolve_existing(&root, &lookup)?;
        // A link inside library/ may lead to a place that is off limits (like _library_config).
        let canonical_root = root.canonicalize().map_err(|_| RelPathError::NotFound)?;
        let inner = real.strip_prefix(&canonical_root).map_err(|_| RelPathError::Escapes)?;
        let inner = inner.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
        if !readable(&inner) {
            return Err(RelPathError::Escapes);
        }
        Ok(real)
    })
    .await
    .map_err(ApiError::internal)?
    .map_err(path_error)?;

    let mut file = tokio::fs::File::open(&real).await.map_err(|_| ApiError::not_found("There is no such file."))?;
    let meta = file.metadata().await.map_err(ApiError::internal)?;
    let len = meta.len();
    let etag = format!("\"{:x}-{:x}\"", len, modified_nanos(&meta));
    let cache_control = if immutable(&rel) { "private, max-age=31536000, immutable" } else { "private, no-cache" };
    if etag_matches(&headers, &etag) {
        return Ok(not_modified(&etag, cache_control));
    }
    // If-Range: only honour the range if the file is still the one the client has a piece of.
    let range_header = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let if_range_ok = headers.get(header::IF_RANGE).and_then(|v| v.to_str().ok()).map_or(true, |v| v.trim() == etag);
    let spec = if if_range_ok { parse_range(range_header, len) } else { RangeSpec::Full };

    let (status, start, count) = match spec {
        RangeSpec::Full => (StatusCode::OK, 0, len),
        RangeSpec::Partial(start, end) => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
        RangeSpec::Unsatisfiable => {
            let mut response = ApiError::new(StatusCode::RANGE_NOT_SATISFIABLE, "range_not_satisfiable", "That byte range is outside the file.")
                .into_response();
            set_header(&mut response, header::CONTENT_RANGE, &format!("bytes */{len}"));
            return Ok(response);
        }
    };
    if start > 0 {
        file.seek(std::io::SeekFrom::Start(start)).await.map_err(ApiError::internal)?;
    }
    let stream = tokio_util::io::ReaderStream::with_capacity(file.take(count), 64 * 1024);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    let headers_out = response.headers_mut();
    headers_out.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type(&rel)));
    headers_out.insert(header::CONTENT_LENGTH, HeaderValue::from(count));
    headers_out.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers_out.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    headers_out.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if status == StatusCode::PARTIAL_CONTENT {
        set_header(&mut response, header::CONTENT_RANGE, &format!("bytes {start}-{}/{len}", start + count - 1));
    }
    set_header(&mut response, header::ETAG, &etag);
    Ok(response)
}

// ----------------------------------------------------------------- thumbs

#[derive(Deserialize)]
struct ThumbParams {
    since: Option<i64>,
}

/// Which items have a shared picture. Without `since`: all of them (a listing
/// of every id, once per client). With `since=<thumbRevision>`: only those saved
/// after it, so a client learns changed pictures without listing every one.
async fn list_thumbs(State(state): State<AppState>, params: Result<Query<ThumbParams>, QueryRejection>, headers: HeaderMap) -> Result<Response, ApiError> {
    let Query(params) = params.map_err(bad_query)?;
    // The thumbnail revision is read before the files are looked at, so a picture
    // saved meanwhile is listed again by the next `since` rather than missed.
    let library = state.library.state().await?;
    let handle = state.library.handle.clone();
    let (ids, thumb_revision, legacy) = match params.since {
        Some(since) => {
            let (ids, revision) = state.library.thumbs_since(since.max(0)).await?;
            (ids, revision, false)
        }
        None => {
            // One listing of the picture folder and a lookup of the names found, not a `stat` for each item of the library.
            let dir = handle.thumbs_dir.clone();
            let names: Vec<String> = tokio::task::spawn_blocking(move || {
                std::fs::read_dir(dir)
                    .map(|entries| entries.filter_map(|entry| entry.ok()).filter_map(|entry| entry.file_name().to_str().and_then(|name| name.strip_suffix(".png")).map(str::to_string)).collect())
                    .unwrap_or_default()
            })
            .await
            .map_err(ApiError::internal)?;
            (state.library.ids_by_thumb_name(names).await?, library.thumb_revision, true)
        }
    };
    let found = tokio::task::spawn_blocking(move || {
        ids.into_iter()
            .filter_map(|id| {
                let meta = std::fs::metadata(handle.thumb_path(&id)).ok().filter(|m| m.is_file())?;
                Some(json!({ "id": id, "version": format!("{:x}-{:x}", modified_nanos(&meta), meta.len()) }))
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(ApiError::internal)?;
    let mut body = json!({ "thumbRevision": thumb_revision, "thumbs": found });
    if legacy {
        body["revision"] = json!(library.legacy_revision());
    }
    let body = serde_json::to_vec(&body).map_err(ApiError::internal)?;
    let etag = format!("\"{}\"", hex(&Sha256::digest(&body)[..8]));
    if etag_matches(&headers, &etag) {
        return Ok(not_modified(&etag, "private, no-cache"));
    }
    Ok(json_bytes(StatusCode::OK, Some(&etag), Bytes::from(body)))
}

async fn get_thumb(State(state): State<AppState>, Path(id): Path<String>, headers: HeaderMap) -> Result<Response, ApiError> {
    if state.library.known_ids(vec![id.clone()]).await?.is_empty() {
        return Err(ApiError::not_found("There is no such item."));
    }
    let path = state.library.handle.thumb_path(&id);
    let read = tokio::task::spawn_blocking(move || {
        let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file())?;
        let etag = format!("\"{:x}-{:x}\"", modified_nanos(&meta), meta.len());
        Some((etag, path))
    })
    .await
    .map_err(ApiError::internal)?;
    let Some((etag, path)) = read else { return Err(ApiError::not_found("This item has no thumbnail yet.")) };
    if etag_matches(&headers, &etag) {
        return Ok(not_modified(&etag, "private, no-cache"));
    }
    let bytes = tokio::fs::read(&path).await.map_err(|_| ApiError::not_found("This item has no thumbnail yet."))?;
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    set_header(&mut response, header::ETAG, &etag);
    Ok(response)
}

async fn put_thumb(State(state): State<AppState>, Path(id): Path<String>, body: Body) -> Result<StatusCode, ApiError> {
    let limit = state.options.max_thumb_bytes;
    let bytes = axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| ApiError::too_large(format!("A thumbnail can be at most {} KB.", limit / 1024)))?;
    if bytes.len() < PNG_SIGNATURE.len() || bytes[..8] != PNG_SIGNATURE {
        return Err(ApiError::bad_request("not_png", "A thumbnail must be a PNG image."));
    }
    // The picture is saved and recorded in the thumbnail log, so clients that follow it learn of it.
    if state.library.save_thumb(id, bytes.to_vec()).await?.is_none() {
        return Err(ApiError::not_found("There is no such item."));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- uploads

/// Where an uploaded file may go: `library/_import_<name>/...`, the staging
/// area importers use. Anything else would let a write token drop files into
/// managed storage without going through the manifest.
fn upload_rel(path: &str) -> Result<String, ApiError> {
    let rel = files::portable_rel(path).map_err(path_error)?;
    let parts: Vec<&str> = rel.split('/').collect();
    let ok = parts.len() >= 4
        && parts[0] == "library"
        && parts[1].starts_with("_import_")
        && parts[1].len() > 8
        && parts[1].len() <= 64
        && parts[1].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !rel.ends_with(".tbx-part");
    if ok {
        Ok(rel)
    } else {
        Err(ApiError::bad_request(
            "bad_upload_path",
            "Upload files to a path like library/_import_<name>/<folder>/<file>.",
        ))
    }
}

async fn create_upload(State(state): State<AppState>) -> Result<(StatusCode, Json<Value>), ApiError> {
    let limits = state.uploads.limits;
    let uploads = state.clone();
    let id = tokio::task::spawn_blocking(move || uploads.uploads.create()).await.map_err(ApiError::internal)??;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id,
            "maxFileBytes": limits.max_file_bytes,
            "maxUploadBytes": limits.max_upload_bytes,
            "maxFiles": limits.max_files,
            "idleTimeoutSeconds": limits.idle_timeout.as_secs(),
        })),
    ))
}

async fn delete_upload(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    tokio::task::spawn_blocking(move || state.uploads.remove(&id)).await.map_err(ApiError::internal)??;
    Ok(StatusCode::NO_CONTENT)
}

async fn put_upload_file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let rel = upload_rel(&path)?;
    store_upload_file(&state, &id, rel, request).await
}

/// Where a transfer package file may go: the library-relative path the package
/// names (`library/...`) or a thumbnail (`thumbs/<name>.png`). Files are kept
/// under `package/` inside the upload, apart from staged import files.
fn package_rel(path: &str) -> Result<String, ApiError> {
    let rel = files::portable_rel(path).map_err(path_error)?;
    let parts: Vec<&str> = rel.split('/').collect();
    let ok = (parts[0] == "library" && parts.len() >= 3 && !rel.ends_with(".tbx-part"))
        || (parts[0] == "thumbs" && parts.len() == 2 && parts[1].ends_with(".png"));
    if ok {
        Ok(format!("package/{rel}"))
    } else {
        Err(ApiError::bad_request("bad_upload_path", "Upload package files to a path like library/<...>/<file> or thumbs/<name>.png."))
    }
}

async fn put_package_file(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
    request: Request,
) -> Result<Json<Value>, ApiError> {
    let rel = package_rel(&path)?;
    store_upload_file(&state, &id, rel, request).await
}

/// Stream one request body into the upload at `rel` (relative to its files folder).
async fn store_upload_file(state: &AppState, id: &str, rel: String, request: Request) -> Result<Json<Value>, ApiError> {
    let id = id.to_string();
    if !state.uploads.exists(&id) {
        return Err(ApiError::not_found("There is no such import in progress. It may have expired; start a new one."));
    }
    let limits = state.uploads.limits;
    let declared = request.headers().get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > limits.max_file_bytes) {
        return Err(ApiError::too_large(format!("That file is larger than the {} MB limit for one file.", limits.max_file_bytes / (1024 * 1024))));
    }
    let mut slot = state.uploads.begin_file(&id, &rel)?;
    let base = state.uploads.files_dir(&id);
    let target = files::resolve_for_write(&base, &rel).map_err(|e| match e {
        RelPathError::NotAFile => ApiError::conflict("path_conflict", "A file and a folder would have the same name in this import."),
        other => path_error(other),
    })?;
    let parent = target.parent().map(|p| p.to_path_buf()).ok_or_else(|| ApiError::bad_request("bad_upload_path", "That path is not allowed."))?;
    tokio::fs::create_dir_all(&parent).await.map_err(|_| {
        ApiError::conflict("path_conflict", "A file and a folder would have the same name in this import.")
    })?;
    let file_name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let part = parent.join(format!("{file_name}.{}.tbx-part", random_hex(4)));

    let result: Result<u64, ApiError> = async {
        let mut file = tokio::fs::File::create(&part).await.map_err(ApiError::internal)?;
        let mut stream = request.into_body().into_data_stream();
        let mut total = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ApiError::bad_request("upload_interrupted", "The upload was interrupted before it finished."))?;
            slot.reserve(chunk.len() as u64)?;
            file.write_all(&chunk).await.map_err(ApiError::internal)?;
            total += chunk.len() as u64;
        }
        file.flush().await.map_err(ApiError::internal)?;
        Ok(total)
    }
    .await;
    match result {
        Ok(size) => {
            if tokio::fs::rename(&part, &target).await.is_err() {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(ApiError::conflict("path_conflict", "A file and a folder would have the same name in this import."));
            }
            slot.finish();
            Ok(Json(json!({ "path": rel.strip_prefix("package/").unwrap_or(&rel), "size": size })))
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(error)
        }
    }
}

#[derive(Deserialize)]
struct CommitBody {
    entries: Vec<Value>,
}

async fn commit_upload(State(state): State<AppState>, level: Level, Path(id): Path<String>, ApiJson(body): ApiJson<CommitBody>) -> Result<Response, ApiError> {
    if body.entries.is_empty() {
        return Err(ApiError::bad_request("no_entries", "There is nothing to import: entries is empty."));
    }
    if body.entries.len() > MAX_ENTRIES {
        return Err(ApiError::too_large(format!("One import can hold at most {MAX_ENTRIES} entries.")));
    }
    state.uploads.begin_commit(&id)?;
    // The work runs on its own task: a client that hangs up drops this handler,
    // but the commit still finishes and frees the upload.
    let task = tokio::spawn(async move {
        let result = state.library.commit(state.uploads.files_dir(&id), body.entries).await;
        state.uploads.end_commit(&id);
        let (report, info) = result?;
        let kept = !report.errors.is_empty();
        if !kept {
            let uploads = state.clone();
            let _ = tokio::task::spawn_blocking(move || uploads.uploads.remove(&id)).await;
        }
        let revision = write_revision(&state, level, info).await?;
        let changes = if level.feed() { reply_page(&state, info).await } else { None };
        Ok::<_, ApiError>((report, revision, info, kept, changes))
    });
    let (report, revision, info, kept, changes) = task.await.map_err(ApiError::internal)??;
    let mut payload = json!({
        "revision": revision,
        "imported": report.imported,
        "errors": report.errors,
        "uploadKept": kept,
    });
    if level.feed() {
        payload["base"] = json!(info.base);
        payload["changes"] = json!(changes);
    }
    if report.imported.is_empty() {
        let error = ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "import_failed", "None of the items could be imported. See errors for the reason per item.")
            .with_extra(payload);
        return Ok(error.into_response());
    }
    Ok((StatusCode::OK, Json(payload)).into_response())
}

/// The revision a write's reply names: the library revision as a number for the
/// feed, the string version 1 clients know for the others.
async fn write_revision(state: &AppState, level: Level, info: crate::library::WriteInfo) -> Result<Value, ApiError> {
    Ok(if level.feed() { json!(info.revision) } else { json!(state.library.state().await?.legacy_revision()) })
}

// ----------------------------------------------------------------- assets

#[derive(Deserialize)]
struct DeleteBody {
    ids: Vec<String>,
}

async fn delete_assets(State(state): State<AppState>, level: Level, ApiJson(body): ApiJson<DeleteBody>) -> Result<Response, ApiError> {
    let mut ids = body.ids;
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Err(ApiError::bad_request("no_ids", "Select at least one item to delete."));
    }
    if ids.len() > MAX_DELETE_IDS {
        return Err(ApiError::bad_request("too_many_ids", format!("Delete at most {MAX_DELETE_IDS} items at a time.")));
    }
    // Run to completion even when the client hangs up.
    let task = tokio::spawn(async move {
        let (deleted, info) = state.library.delete(ids).await?;
        let changes = if level.feed() { reply_page(&state, info).await } else { None };
        Ok::<_, ApiError>((deleted, info, changes, state))
    });
    let (deleted, info, changes, state) = task.await.map_err(ApiError::internal)??;
    if level.feed() {
        return Ok((StatusCode::OK, Json(json!({ "revision": info.revision, "base": info.base, "deleted": deleted, "changes": changes }))).into_response());
    }
    // A client of version 1 gets the whole catalog back, built now for it.
    let catalog = state.library.legacy_catalog().await?;
    let mut out = Vec::with_capacity(catalog.body.len() + 256);
    out.extend_from_slice(
        format!(
            "{{\"revision\":{},\"deleted\":{},\"catalog\":",
            serde_json::to_string(&catalog.revision).map_err(ApiError::internal)?,
            serde_json::to_string(&deleted).map_err(ApiError::internal)?
        )
        .as_bytes(),
    );
    out.extend_from_slice(&catalog.body);
    out.push(b'}');
    Ok(json_bytes(StatusCode::OK, None, Bytes::from(out)))
}

#[derive(Deserialize)]
struct MetadataBody {
    edits: Vec<toybox_core::ops::metadata::MetadataEdit>,
}

/// Change the library tags and provenance of items. The answer carries the new
/// revision, the ids that were edited, the top-level items they touched (`touched`)
/// and the rows the edit changed, as `/changes` serves them. Clients of version 1
/// get the touched items themselves instead.
async fn update_metadata(State(state): State<AppState>, level: Level, ApiJson(body): ApiJson<MetadataBody>) -> Result<Json<Value>, ApiError> {
    let edits = body.edits;
    if edits.is_empty() {
        return Err(ApiError::bad_request("no_edits", "There is nothing to change."));
    }
    toybox_core::ops::metadata::validate_edits(&edits).map_err(|e| ApiError::bad_request("invalid_metadata", e.to_string()))?;
    let ids: Vec<String> = edits.iter().map(|edit| edit.id.clone()).collect();
    // Run to completion even when the client hangs up.
    let task = tokio::spawn(async move {
        let (touched, info) = state.library.update_metadata(edits).await?;
        if level.feed() {
            let changes = reply_page(&state, info).await;
            return Ok::<_, ApiError>(json!({ "revision": info.revision, "base": info.base, "touched": touched, "changes": changes }));
        }
        let assets = state.library.served_assets(touched).await?;
        Ok(json!({ "revision": state.library.state().await?.legacy_revision(), "assets": assets }))
    });
    let mut reply = task.await.map_err(ApiError::internal)??;
    reply["updated"] = json!(ids);
    Ok(Json(reply))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportBody {
    ids: Vec<String>,
    #[serde(default)]
    include_clips: bool,
}

/// Describe items as a transfer package (their manifest entries, files with
/// sizes and digests, thumbnails). Files are then fetched with `GET /files` and
/// `GET /thumbs`. Personal metadata is never part of it.
async fn export_assets(State(state): State<AppState>, ApiJson(body): ApiJson<ExportBody>) -> Result<Json<Value>, ApiError> {
    let mut ids = body.ids;
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Err(ApiError::bad_request("no_ids", "Select at least one item to export."));
    }
    if ids.len() > MAX_DELETE_IDS {
        return Err(ApiError::bad_request("too_many_ids", format!("Export at most {MAX_DELETE_IDS} items at a time.")));
    }
    let package = state.library.export_package(ids, body.include_clips).await?;
    Ok(Json(serde_json::to_value(package).map_err(ApiError::internal)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IngestBody {
    package: toybox_core::ops::transfer::TransferPackage,
    #[serde(default)]
    on_conflict: toybox_core::ops::transfer::OnConflict,
}

/// Write a transfer package into the library. The package files must have been
/// uploaded with `PUT /uploads/{id}/package/{path}`. This is the same code a
/// local copy between two libraries runs, so ids, conflicts, packs and
/// categories behave identically. The upload is removed afterwards.
async fn ingest_upload(State(state): State<AppState>, level: Level, Path(id): Path<String>, ApiJson(body): ApiJson<IngestBody>) -> Result<Response, ApiError> {
    if body.package.assets.is_empty() && body.package.failed.is_empty() {
        return Err(ApiError::bad_request("no_entries", "There is nothing to import: the package is empty."));
    }
    if body.package.assets.len() > MAX_ENTRIES {
        return Err(ApiError::too_large(format!("One import can hold at most {MAX_ENTRIES} items.")));
    }
    state.uploads.begin_commit(&id)?;
    // The work runs on its own task: a client that hangs up drops this handler,
    // but the ingest still finishes and frees the upload.
    let task = tokio::spawn(async move {
        let result = state.library.ingest(state.uploads.files_dir(&id).join("package"), body.package, body.on_conflict).await;
        state.uploads.end_commit(&id);
        let (report, info) = result?;
        let uploads = state.clone();
        let _ = tokio::task::spawn_blocking(move || uploads.uploads.remove(&id)).await;
        let revision = write_revision(&state, level, info).await?;
        let changes = if level.feed() { reply_page(&state, info).await } else { None };
        Ok::<_, ApiError>((report, revision, info, changes))
    });
    let (report, revision, info, changes) = task.await.map_err(ApiError::internal)??;
    let mut value = serde_json::to_value(report).map_err(ApiError::internal)?;
    value["revision"] = revision;
    if level.feed() {
        value["base"] = json!(info.base);
        value["changes"] = json!(changes);
    }
    Ok((StatusCode::OK, Json(value)).into_response())
}

// ----------------------------------------------------------------- tokens

async fn list_tokens(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let tokens = state.config.tokens_with_usage().map_err(config_error)?;
    Ok(Json(json!({ "tokens": tokens.iter().map(token_view).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
struct NewToken {
    name: String,
    role: Role,
}

async fn create_token(State(state): State<AppState>, ApiJson(body): ApiJson<NewToken>) -> Result<(StatusCode, Json<Value>), ApiError> {
    let (record, secret) = tokio::task::spawn_blocking(move || state.config.mutate(|config| config.add_token(&body.name, body.role)))
        .await
        .map_err(ApiError::internal)?
        .map_err(config_error)?;
    let mut view = token_view(&record);
    view["token"] = json!(secret);
    Ok((StatusCode::CREATED, Json(view)))
}

async fn delete_token(State(state): State<AppState>, Path(id): Path<String>) -> Result<StatusCode, ApiError> {
    tokio::task::spawn_blocking(move || {
        state.config.mutate(|config| {
            if !config.tokens.iter().any(|t| t.id == id) {
                return Err(ConfigError::NotFound(id.clone()));
            }
            config.remove_token(&id).map(|_| ())
        })
    })
    .await
    .map_err(ApiError::internal)?
    .map_err(config_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parsing() {
        assert_eq!(parse_range(None, 100), RangeSpec::Full);
        assert_eq!(parse_range(Some("bytes=0-9"), 100), RangeSpec::Partial(0, 9));
        assert_eq!(parse_range(Some("bytes=90-"), 100), RangeSpec::Partial(90, 99));
        assert_eq!(parse_range(Some("bytes=-10"), 100), RangeSpec::Partial(90, 99));
        assert_eq!(parse_range(Some("bytes=-500"), 100), RangeSpec::Partial(0, 99));
        assert_eq!(parse_range(Some("bytes=50-5000"), 100), RangeSpec::Partial(50, 99));
        assert_eq!(parse_range(Some("bytes=100-"), 100), RangeSpec::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 100), RangeSpec::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), RangeSpec::Full);
        assert_eq!(parse_range(Some("bytes=9-3"), 100), RangeSpec::Full);
        assert_eq!(parse_range(Some("items=0-3"), 100), RangeSpec::Full);
        assert_eq!(parse_range(Some("bytes=abc"), 100), RangeSpec::Full);
        assert_eq!(parse_range(Some("bytes=0-3"), 0), RangeSpec::Full);
    }

    #[test]
    fn what_may_be_downloaded() {
        assert!(readable("library/audio/0123456789abcdef/x.wav"));
        assert!(readable("library/models/_resources/abc.png"));
        assert!(!readable("_library_config/catalog.json"));
        assert!(!readable("library/_import_ab12/x/y.png"));
        assert!(!readable("library"));
        assert!(!readable("library/x"));
        assert!(!readable("other/library/x/y"));
    }

    #[test]
    fn immutable_paths_are_the_content_addressed_ones() {
        let key = "0123456789abcdef";
        assert!(immutable(&format!("library/audio/{key}/x.wav")));
        assert!(immutable(&format!("library/models/_resources/{}.png", "a".repeat(64))));
        assert!(!immutable("library/audio/legacy_pack/x.wav"));
        assert!(!immutable("library/models/_resources/notahash.png"));
    }

    #[test]
    fn upload_paths_must_stay_in_the_staging_area() {
        assert!(upload_rel("library/_import_ab12/materials/Tiles/albedo.png").is_ok());
        for bad in [
            "library/audio/x/y.wav", "_library_config/catalog.json", "library/_import_/x/y", "library/_import_ab12/x",
            "library/_import_ab12/../../x/y", "../library/_import_ab12/x/y", "library/_import_ab12/x/y.tbx-part", "/library/_import_ab12/x/y",
            "library/_import_a b/x/y",
        ] {
            assert!(upload_rel(bad).is_err(), "{bad}");
        }
    }
}
