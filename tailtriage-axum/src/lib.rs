#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Axum adoption helpers layered on top of `tailtriage-core`.
//!
//! This crate provides a focused middleware + extractor path so handlers can
//! access request instrumentation without repeating request start/finish wiring.

use std::sync::Arc;

use axum::extract::{FromRequestParts, MatchedPath, State};
use axum::http::request::Parts;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::IntoResponse;
use tailtriage_core::{Outcome, OwnedRequestHandle, RequestOptions, Tailtriage};

/// Returns the crate name for smoke-testing workspace wiring.
#[must_use]
pub const fn crate_name() -> &'static str {
    "tailtriage-axum"
}

/// Middleware that starts and finishes one tailtriage request per axum request.
///
/// Use this with `axum::middleware::from_fn_with_state` and pass the same
/// `Arc<Tailtriage>` in middleware state.
pub async fn middleware(
    State(tailtriage): State<Arc<Tailtriage>>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> axum::response::Response {
    let route = request_route_label(&request);
    let started = tailtriage.begin_request_with_owned(route, RequestOptions::new().kind("http"));

    request
        .extensions_mut()
        .insert(TailtriageRequest(started.handle.clone()));

    let response = next.run(request).await;
    let status = response.status();

    started.completion.finish(status_to_outcome(status));
    response
}

/// Handler extractor for the request-scoped instrumentation handle.
#[derive(Debug, Clone)]
pub struct TailtriageRequest(pub OwnedRequestHandle);

impl TailtriageRequest {
    /// Returns the wrapped request handle.
    #[must_use]
    pub fn into_inner(self) -> OwnedRequestHandle {
        self.0
    }
}

impl<S> FromRequestParts<S> for TailtriageRequest
where
    S: Send + Sync,
{
    type Rejection = TailtriageExtractorError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<TailtriageRequest>()
            .cloned()
            .ok_or(TailtriageExtractorError)
    }
}

/// Rejection returned when `TailtriageRequest` is used without middleware.
#[derive(Debug, Clone, Copy)]
pub struct TailtriageExtractorError;

impl IntoResponse for TailtriageExtractorError {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "tailtriage extractor missing. Add tailtriage_axum::middleware.",
        )
            .into_response()
    }
}

fn request_route_label(request: &Request<axum::body::Body>) -> String {
    request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| request.uri().path(), MatchedPath::as_str)
        .to_owned()
}

fn status_to_outcome(status: StatusCode) -> Outcome {
    if status.is_server_error() {
        Outcome::Error
    } else {
        Outcome::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::{crate_name, status_to_outcome};

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(crate_name(), "tailtriage-axum");
    }

    #[test]
    fn maps_server_errors_to_error_outcome() {
        assert_eq!(
            status_to_outcome(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
            tailtriage_core::Outcome::Error
        );
        assert_eq!(
            status_to_outcome(axum::http::StatusCode::BAD_REQUEST),
            tailtriage_core::Outcome::Ok
        );
    }
}
