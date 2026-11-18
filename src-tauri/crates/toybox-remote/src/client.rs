//! A typed, blocking client for `toybox-server`'s HTTP API (see docs/server.md).
//! One method per route. Bearer authentication, timeouts, and every failure
//! mapped to a [`RemoteError`] with a sentence a person can act on. Nothing in
//! here panics on a bad answer, and the token never appears in an error, a
//! `Debug` print or a log.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use toybox_core::infra::library::RemoteRole;
use toybox_core::infra::store::ChangesPage;
use toybox_core::ops::metadata::MetadataEdit;
use toybox_core::ops::transfer::{OnConflict, TransferPackage};

use crate::error::{RemoteError, RemoteResult};
use crate::types::*;

/// The version of the library protocol this client speaks (`apiVersion` of `/info`).
/// 2 follows the revision feed; a server that says less is refused with a clear message.
pub const API_LEVEL: u32 = 2;
/// Sent with every request so the server answers in the shapes of [`API_LEVEL`].
const API_HEADER: &str = "X-Toybox-Api";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// A big library builds its catalog on the first request after a start.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
/// A transfer that makes no progress for this long has stalled.
/// A large ingest or commit legitimately works for a long time before it answers.
const LONG_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const READ_TIMEOUT: Duration = Duration::from_secs(60);
const JSON_LIMIT: u64 = 256 * 1024 * 1024;
const ERROR_LIMIT: u64 = 256 * 1024;

#[derive(Clone)]
pub struct Client {
    base: String,
    token: Arc<str>,
    agent: ureq::Agent,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("base", &self.base).field("token", &"<hidden>").finish()
    }
}

/// A file being downloaded.
pub struct Download {
    /// Size announced by the server.
    pub len: Option<u64>,
    pub reader: Box<dyn Read + Send>,
}

/// The answer to `GET /catalog`.
pub enum CatalogReply {
    Fresh { body: Vec<u8>, etag: Option<String> },
    NotModified,
}

/// The answer to `GET /thumbs`.
pub enum ThumbsReply {
    Fresh { list: ThumbList, etag: Option<String> },
    NotModified,
}

/// Percent-encode a path, keeping `/`.
pub fn encode_path(path: &str) -> String {
    encode(path, true)
}

/// Percent-encode one path segment (an asset id, for example).
pub fn encode_segment(segment: &str) -> String {
    encode(segment, false)
}

fn encode(text: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

impl Client {
    /// `base` is a normalised server address (see [`crate::url`]).
    pub fn new(base: &str, token: &str) -> Client {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // The token is never sent anywhere but the address the user gave.
            .max_redirects(0)
            .timeout_resolve(Some(CONNECT_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(RESPONSE_TIMEOUT))
            .timeout_per_read(Some(READ_TIMEOUT))
            .user_agent(format!("Toybox/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Client { base: base.trim_end_matches('/').to_string(), token: Arc::from(token), agent }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// A client for the same server with another token.
    pub fn with_token(&self, token: &str) -> Client {
        Client { base: self.base.clone(), token: Arc::from(token), agent: self.agent.clone() }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    // ---- plumbing ---------------------------------------------------------------

    fn transport(&self, error: ureq::Error) -> RemoteError {
        let server = self.base.clone();
        let text = error.to_string();
        let lower = text.to_ascii_lowercase();
        let secure = |detail: String| RemoteError::Tls { server: server.clone(), detail };
        match error {
            ureq::Error::Timeout(_) => RemoteError::Unreachable { server, detail: "it did not answer in time".into() },
            ureq::Error::HostNotFound => RemoteError::Unreachable { server, detail: "no server with that name was found".into() },
            ureq::Error::ConnectionFailed => RemoteError::Unreachable { server, detail: "the connection was refused".into() },
            ureq::Error::BodyStalled => RemoteError::Unreachable { server, detail: "the transfer stalled".into() },
            ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::Pem(_) => secure(text),
            ureq::Error::BadUri(_) | ureq::Error::Http(_) | ureq::Error::InvalidProxyUrl => RemoteError::Protocol(format!("the address is not valid: {text}")),
            ureq::Error::BodyExceedsLimit(_) => RemoteError::Protocol("the answer is unexpectedly large".into()),
            _ if ["certificate", "tls", "handshake", "unknownissuer", "notvalidforname", "peer sent"].iter().any(|word| lower.contains(word)) => secure(text),
            _ => RemoteError::Unreachable { server, detail: text },
        }
    }

    /// Turn a response into an error unless it is a success (or `allow` says it is expected).
    fn check(&self, response: ureq::http::Response<ureq::Body>, allow: &[u16]) -> RemoteResult<ureq::http::Response<ureq::Body>> {
        let status = response.status().as_u16();
        if (200..300).contains(&status) || allow.contains(&status) {
            return Ok(response);
        }
        let retry_after = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
        let mut body = response.into_body();
        let bytes = body.with_config().limit(ERROR_LIMIT).read_to_vec().unwrap_or_default();
        Err(status_error(status, retry_after, &bytes, &self.base))
    }

    fn call(&self, result: Result<ureq::http::Response<ureq::Body>, ureq::Error>, allow: &[u16]) -> RemoteResult<ureq::http::Response<ureq::Body>> {
        match result {
            Ok(response) => self.check(response, allow),
            Err(error) => Err(self.transport(error)),
        }
    }

    fn get(&self, path: &str, extra: &[(&str, &str)]) -> RemoteResult<ureq::http::Response<ureq::Body>> {
        let mut request = self.agent.get(self.url(path)).header("Authorization", self.auth()).header("Accept", "application/json").header(API_HEADER, "2");
        for (name, value) in extra {
            request = request.header(*name, *value);
        }
        self.call(request.call(), &[304])
    }

    fn body_bytes(&self, response: ureq::http::Response<ureq::Body>) -> RemoteResult<Vec<u8>> {
        let mut body = response.into_body();
        body.with_config().limit(JSON_LIMIT).read_to_vec().map_err(|e| self.transport(e))
    }

    fn json<T: DeserializeOwned>(&self, response: ureq::http::Response<ureq::Body>) -> RemoteResult<T> {
        let bytes = self.body_bytes(response)?;
        serde_json::from_slice(&bytes).map_err(|e| RemoteError::Protocol(format!("unexpected JSON: {e}")))
    }

    fn post_json(&self, path: &str, body: &impl Serialize, allow: &[u16]) -> RemoteResult<ureq::http::Response<ureq::Body>> {
        self.post_json_within(path, body, allow, RESPONSE_TIMEOUT)
    }

    fn post_json_within(&self, path: &str, body: &impl Serialize, allow: &[u16], response_timeout: Duration) -> RemoteResult<ureq::http::Response<ureq::Body>> {
        let bytes = serde_json::to_vec(body).map_err(|e| RemoteError::Local(format!("could not encode the request: {e}")))?;
        let result = self
            .agent
            .post(self.url(path))
            .config()
            .timeout_recv_response(Some(response_timeout))
            // A read that waits for the answer is covered by the per-read limit too.
            .timeout_per_read(Some(response_timeout.max(READ_TIMEOUT)))
            .build()
            .header("Authorization", self.auth())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header(API_HEADER, "2")
            .send(bytes);
        self.call(result, allow)
    }

    fn empty_post(&self, path: &str) -> RemoteResult<ureq::http::Response<ureq::Body>> {
        let result = self.agent.post(self.url(path)).header("Authorization", self.auth()).header("Accept", "application/json").header(API_HEADER, "2").send_empty();
        self.call(result, &[])
    }

    fn delete(&self, path: &str) -> RemoteResult<()> {
        let result = self.agent.delete(self.url(path)).header("Authorization", self.auth()).header(API_HEADER, "2").call();
        self.call(result, &[]).map(|_| ())
    }

    fn etag(response: &ureq::http::Response<ureq::Body>) -> Option<String> {
        response.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string)
    }

    // ---- routes -----------------------------------------------------------------

    /// `GET /info`: the server's name and the role of this token.
    pub fn info(&self) -> RemoteResult<Info> {
        self.json(self.get("/info", &[])?)
    }

    /// `GET /catalog`; `etag` from an earlier answer gets `NotModified` when nothing changed.
    pub fn catalog(&self, etag: Option<&str>) -> RemoteResult<CatalogReply> {
        let extra: Vec<(&str, &str)> = etag.map(|e| vec![("If-None-Match", e)]).unwrap_or_default();
        let response = self.get("/catalog", &extra)?;
        if response.status().as_u16() == 304 {
            return Ok(CatalogReply::NotModified);
        }
        let etag = Self::etag(&response);
        Ok(CatalogReply::Fresh { body: self.body_bytes(response)?, etag })
    }

    /// `GET /changes`: the library's changes after revision `since` (and the cursor
    /// `after` inside it), at most `limit` rows. `db_id` is the identity of the
    /// server database the caller's cursor belongs to (empty if it has none yet).
    pub fn changes(&self, since: i64, after: Option<&str>, limit: usize, db_id: &str) -> RemoteResult<ChangesPage> {
        let mut path = format!("/changes?since={since}&limit={limit}");
        if let Some(after) = after {
            path.push_str(&format!("&after={}", encode_segment(after)));
        }
        if !db_id.is_empty() {
            path.push_str(&format!("&dbId={}", encode_segment(db_id)));
        }
        self.json(self.get(&path, &[])?)
    }

    /// `GET /thumbs`, or with `since` only the pictures saved after that thumbnail revision.
    pub fn thumbs_since(&self, since: Option<i64>) -> RemoteResult<ThumbList> {
        let path = match since {
            Some(since) => format!("/thumbs?since={since}"),
            None => "/thumbs".to_string(),
        };
        self.json(self.get(&path, &[])?)
    }

    /// `GET /files/{path}`, streamed.
    pub fn file(&self, rel: &str) -> RemoteResult<Download> {
        let response = self.agent.get(self.url(&format!("/files/{}", encode_path(rel)))).header("Authorization", self.auth()).header(API_HEADER, "2").call();
        let response = self.call(response, &[])?;
        let len = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
        Ok(Download { len, reader: Box::new(response.into_body().into_reader()) })
    }

    /// `GET /thumbs`: which items have a shared picture.
    pub fn thumbs(&self, etag: Option<&str>) -> RemoteResult<ThumbsReply> {
        let extra: Vec<(&str, &str)> = etag.map(|e| vec![("If-None-Match", e)]).unwrap_or_default();
        let response = self.get("/thumbs", &extra)?;
        if response.status().as_u16() == 304 {
            return Ok(ThumbsReply::NotModified);
        }
        let etag = Self::etag(&response);
        Ok(ThumbsReply::Fresh { list: self.json(response)?, etag })
    }

    /// `GET /thumbs/{id}`; `None` when the server has no picture for it.
    pub fn thumb(&self, id: &str) -> RemoteResult<Option<Vec<u8>>> {
        match self.get(&format!("/thumbs/{}", encode_segment(id)), &[]) {
            Ok(response) => Ok(Some(self.body_bytes(response)?)),
            Err(RemoteError::NotFound(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// `PUT /thumbs/{id}` (write role).
    pub fn put_thumb(&self, id: &str, png: &[u8]) -> RemoteResult<()> {
        let result = self
            .agent
            .put(self.url(&format!("/thumbs/{}", encode_segment(id))))
            .header("Authorization", self.auth())
            .header("Content-Type", "image/png")
            .header(API_HEADER, "2")
            .send(png);
        self.call(result, &[]).map(|_| ())
    }

    /// `POST /uploads` (write role).
    pub fn create_upload(&self) -> RemoteResult<UploadInfo> {
        self.json(self.empty_post("/uploads")?)
    }

    /// `DELETE /uploads/{id}`.
    pub fn cancel_upload(&self, upload: &str) -> RemoteResult<()> {
        self.delete(&format!("/uploads/{upload}"))
    }

    fn put_stream(&self, path: &str, reader: &mut dyn Read, len: u64) -> RemoteResult<()> {
        let body = ureq::SendBody::from_reader(reader);
        let result = self
            .agent
            .put(self.url(path))
            .header("Authorization", self.auth())
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", len.to_string())
            .header(API_HEADER, "2")
            .send(body);
        self.call(result, &[]).map(|_| ())
    }

    /// `PUT /uploads/{id}/files/{path}`: stage one import file (`library/_import_<name>/...`).
    pub fn put_upload_file(&self, upload: &str, rel: &str, reader: &mut dyn Read, len: u64) -> RemoteResult<()> {
        self.put_stream(&format!("/uploads/{upload}/files/{}", encode_path(rel)), reader, len)
    }

    /// `PUT /uploads/{id}/package/{path}`: one file of a transfer package.
    pub fn put_package_file(&self, upload: &str, path: &str, reader: &mut dyn Read, len: u64) -> RemoteResult<()> {
        self.put_stream(&format!("/uploads/{upload}/package/{}", encode_path(path)), reader, len)
    }

    /// `POST /uploads/{id}/commit`. A commit where nothing could be imported
    /// still answers with the per-entry reasons (the server sends them with a 422).
    pub fn commit(&self, upload: &str, entries: &[Value]) -> RemoteResult<CommitReply> {
        let response = self.post_json_within(&format!("/uploads/{upload}/commit"), &json!({ "entries": entries }), &[422], LONG_RESPONSE_TIMEOUT)?;
        self.json(response)
    }

    /// `POST /uploads/{id}/ingest`: write an uploaded transfer package.
    pub fn ingest(&self, upload: &str, package: &TransferPackage, on_conflict: OnConflict) -> RemoteResult<IngestReply> {
        let response = self.post_json_within(&format!("/uploads/{upload}/ingest"), &json!({ "package": package, "onConflict": on_conflict }), &[], LONG_RESPONSE_TIMEOUT)?;
        self.json(response)
    }

    /// `POST /assets/export` (read role): describe items as a transfer package.
    pub fn export_package(&self, ids: &[String], include_clips: bool) -> RemoteResult<TransferPackage> {
        self.json(self.post_json("/assets/export", &json!({ "ids": ids, "includeClips": include_clips }), &[])?)
    }

    /// `POST /assets/delete` (write role). The answer carries the rows it changed.
    pub fn delete_assets(&self, ids: &[String]) -> RemoteResult<DeleteReply> {
        self.json(self.post_json("/assets/delete", &json!({ "ids": ids }), &[])?)
    }

    /// `POST /assets/metadata` (write role): change library tags and provenance.
    pub fn update_metadata(&self, edits: &[MetadataEdit]) -> RemoteResult<MetadataReply> {
        self.json(self.post_json("/assets/metadata", &json!({ "edits": edits }), &[])?)
    }

    /// `GET /tokens` (admin).
    pub fn tokens(&self) -> RemoteResult<Vec<TokenView>> {
        #[derive(Deserialize)]
        struct List {
            tokens: Vec<TokenView>,
        }
        Ok(self.json::<List>(self.get("/tokens", &[])?)?.tokens)
    }

    /// `POST /tokens` (admin). The secret is in this answer only.
    pub fn create_token(&self, name: &str, role: RemoteRole) -> RemoteResult<NewToken> {
        self.json(self.post_json("/tokens", &json!({ "name": name, "role": role }), &[])?)
    }

    /// `DELETE /tokens/{id}` (admin).
    pub fn revoke_token(&self, id: &str) -> RemoteResult<()> {
        self.delete(&format!("/tokens/{}", encode_segment(id)))
    }
}

/// Map an error status and its body to a [`RemoteError`].
pub fn status_error(status: u16, retry_after: Option<u64>, body: &[u8], server: &str) -> RemoteError {
    let parsed: Option<(String, String)> = serde_json::from_slice::<Value>(body).ok().and_then(|v| {
        let error = v.get("error")?;
        Some((error.get("code")?.as_str()?.to_string(), error.get("message")?.as_str()?.to_string()))
    });
    let (code, message) = parsed.clone().unwrap_or_else(|| (String::new(), format!("The server answered with an error (HTTP {status}).")));
    match status {
        401 => RemoteError::BadToken,
        403 => RemoteError::Forbidden(message),
        404 if parsed.is_none() => RemoteError::Protocol(format!("{server} did not answer like a Toybox server (HTTP 404)")),
        404 => RemoteError::NotFound(message),
        409 => RemoteError::Conflict { code, message },
        429 if code == "locked_out" => RemoteError::LockedOut { retry_after },
        300..=399 => RemoteError::Protocol(format!("{server} redirected somewhere else (HTTP {status}); enter the address it redirects to")),
        500..=599 => RemoteError::Server { status, message },
        _ => RemoteError::Rejected { status, code, message },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_ids_are_percent_encoded() {
        assert_eq!(encode_path("library/audio/ab cd/été.wav"), "library/audio/ab%20cd/%C3%A9t%C3%A9.wav");
        assert_eq!(encode_segment("uid://abc"), "uid%3A%2F%2Fabc");
    }

    #[test]
    fn debug_output_hides_the_token() {
        let client = Client::new("https://a.example.com", "tbx_secretsecretsecret");
        assert!(!format!("{client:?}").contains("secretsecret"));
    }

    #[test]
    fn server_errors_become_plain_errors() {
        let body = br#"{"error":{"code":"forbidden","message":"This token cannot do that. It needs the write role."}}"#;
        assert_eq!(status_error(403, None, body, "s"), RemoteError::Forbidden("This token cannot do that. It needs the write role.".into()));
        assert_eq!(status_error(401, None, br#"{"error":{"code":"unauthorized","message":"x"}}"#, "s"), RemoteError::BadToken);
        let locked = br#"{"error":{"code":"locked_out","message":"x"}}"#;
        assert_eq!(status_error(429, Some(900), locked, "s"), RemoteError::LockedOut { retry_after: Some(900) });
        assert_eq!(status_error(409, None, br#"{"error":{"code":"last_admin","message":"m"}}"#, "s").code(), "last_admin");
        // Not JSON at all (a proxy's page): never a panic, always a sentence.
        assert!(matches!(status_error(502, None, b"<html>Bad gateway</html>", "s"), RemoteError::Server { .. }));
        assert!(matches!(status_error(404, None, b"<html>", "s"), RemoteError::Protocol(_)));
        assert!(matches!(status_error(400, None, b"", "s"), RemoteError::Rejected { .. }));
    }
}
