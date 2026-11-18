//! API errors: `{ "error": { "code", "message" } }` with plain-language messages.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};

use crate::config::Role;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    /// Extra top-level fields merged next to `error` (for example the per-entry
    /// errors of a failed import).
    pub extra: Option<Value>,
    pub retry_after: Option<u64>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        ApiError { status, code, message: message.into(), extra: None, retry_after: None }
    }
    pub fn with_extra(mut self, extra: Value) -> Self {
        self.extra = Some(extra);
        self
    }
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }
    pub fn too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", message)
    }
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }
    pub fn forbidden(needed: Role) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            format!("This token cannot do that. It needs the {} role.", needed.as_str()),
        )
    }
    /// An unexpected failure. The detail goes to the server log, not the client.
    pub fn internal(detail: impl std::fmt::Display) -> Self {
        eprintln!("toybox-server: internal error: {detail}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "Something went wrong on the server. Check the server log.")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({ "error": { "code": self.code, "message": self.message } });
        if let (Some(Value::Object(extra)), Some(map)) = (self.extra, body.as_object_mut()) {
            map.extend(extra);
        }
        let mut response = (self.status, axum::Json(body)).into_response();
        if self.status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer realm=\"toybox\""));
        }
        if let Some(seconds) = self.retry_after {
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}

/// JSON body extractor whose failures use the API error shape.
pub struct ApiJson<T>(pub T);

impl<T, S> axum::extract::FromRequest<S> for ApiJson<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;
    async fn from_request(request: axum::extract::Request, state: &S) -> Result<Self, ApiError> {
        match axum::Json::<T>::from_request(request, state).await {
            Ok(axum::Json(value)) => Ok(ApiJson(value)),
            Err(rejection) => Err(rejection.into()),
        }
    }
}

impl From<axum::extract::rejection::JsonRejection> for ApiError {
    fn from(rejection: axum::extract::rejection::JsonRejection) -> Self {
        use axum::extract::rejection::JsonRejection as R;
        match rejection {
            R::BytesRejection(_) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                ApiError::too_large("The request body is too large.")
            }
            R::MissingJsonContentType(_) => {
                ApiError::bad_request("bad_request", "Send the body as JSON with Content-Type: application/json.")
            }
            other => ApiError::bad_request("bad_request", format!("The request body is not valid: {}", other.body_text())),
        }
    }
}

impl From<axum::extract::rejection::PathRejection> for ApiError {
    fn from(rejection: axum::extract::rejection::PathRejection) -> Self {
        ApiError::bad_request("bad_request", format!("The address is not valid: {}", rejection.body_text()))
    }
}
