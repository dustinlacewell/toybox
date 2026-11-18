//! Bearer-token authentication, roles, failed-attempt delay and lockout.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::config::Role;
use crate::error::ApiError;
use crate::state::AppState;

const HOW_TO_SIGN_IN: &str = "Send your access token as: Authorization: Bearer <token>";

/// Who is calling: put into the request by [`authenticate`].
#[derive(Debug, Clone)]
pub struct Principal {
    pub token_id: String,
    pub name: String,
    pub role: Role,
}

impl Principal {
    /// Refuse unless the token has at least `needed`.
    pub fn require(&self, needed: Role) -> Result<(), ApiError> {
        if self.role >= needed {
            Ok(())
        } else {
            Err(ApiError::forbidden(needed))
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Principal {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        parts.extensions.get::<Principal>().cloned().ok_or_else(|| ApiError::unauthorized(HOW_TO_SIGN_IN))
    }
}

/// The token name, copied onto the response so the request log can print it.
#[derive(Debug, Clone)]
pub struct TokenName(pub String);

/// The caller's address. Behind the reverse proxy on the same machine the peer
/// is loopback and the real client is the last entry of `X-Forwarded-For`; from
/// anyone else that header is ignored because they could write anything in it.
pub fn client_ip(request: &Request) -> IpAddr {
    let peer = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let Some(peer) = peer else { return IpAddr::V4(Ipv4Addr::UNSPECIFIED) };
    if peer.is_loopback() {
        let forwarded = request
            .headers()
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .filter_map(|part| part.trim().parse::<IpAddr>().ok())
            .next_back();
        if let Some(ip) = forwarded {
            return ip;
        }
    }
    peer
}

fn bearer(request: &Request) -> Option<&str> {
    let value = request.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim()).filter(|t| !t.is_empty())
}

/// Middleware for every route except `/api/v1/health`.
pub async fn authenticate(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let ip = client_ip(&request);
    if let Err(wait) = state.limiter.check(ip) {
        let mut error = ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "locked_out",
            "Too many wrong tokens from this address. Try again later.",
        );
        error.retry_after = Some(wait.as_secs().max(1));
        return error.into_response();
    }
    let Some(secret) = bearer(&request) else {
        return ApiError::unauthorized(HOW_TO_SIGN_IN).into_response();
    };
    let config = match state.config.snapshot() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("toybox-server: {error}");
            return ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "settings_unreadable",
                "The server settings file could not be read, so nobody can sign in until it is fixed.",
            )
            .into_response();
        }
    };
    let principal = match config.authenticate(secret) {
        Some(token) => Principal { token_id: token.id.clone(), name: token.name.clone(), role: token.role },
        None => {
            state.limiter.failure(ip);
            tokio::time::sleep(state.options.fail_delay).await;
            return ApiError::unauthorized("That token is not valid. It may have been revoked.").into_response();
        }
    };
    state.limiter.success(ip);
    state.config.touch(&principal.token_id);
    let name = TokenName(principal.name.clone());
    request.extensions_mut().insert(principal);
    let mut response = next.run(request).await;
    response.extensions_mut().insert(name);
    response
}
