#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Axum adoption helpers layered on top of `tailtriage-core`.
//!
//! This crate provides a focused middleware + extractor path so handlers can
//! access request instrumentation without repeating request start/finish wiring.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{FromRequestParts, MatchedPath, State};
use axum::http::request::Parts;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::IntoResponse;
use tailtriage_core::{Outcome, OwnedRequestHandle, RequestOptions, Tailtriage};

type MiddlewareFuture = Pin<Box<dyn Future<Output = axum::response::Response> + Send + 'static>>;

/// Returns the crate name for smoke-testing workspace wiring.
#[must_use]
pub const fn crate_name() -> &'static str {
    "tailtriage-axum"
}

/// Middleware that starts and finishes one tailtriage request per axum request.
///
/// Use this with `axum::middleware::from_fn_with_state` and pass the same
/// `Arc<Tailtriage>` in middleware state.
///
/// The middleware records route labels from `MatchedPath` when available and
/// otherwise falls back to the raw URI path. By default it maps 2xx/3xx to
/// [`Outcome::Ok`], 4xx to [`Outcome::Rejected`] (except 408 to
/// [`Outcome::Timeout`]), and 5xx to [`Outcome::Error`].
pub async fn middleware(
    State(tailtriage): State<Arc<Tailtriage>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> axum::response::Response {
    run_middleware_with_status_classifier(tailtriage, request, next, default_status_to_outcome)
        .await
}

/// Returns middleware with an explicit response-status classifier.
///
/// This keeps [`middleware`] ergonomic as the default path while allowing
/// future callers to choose a different status-to-outcome policy.
pub fn middleware_with_status_classifier<C>(
    classify_status: C,
) -> impl Clone
       + Send
       + 'static
       + Fn(State<Arc<Tailtriage>>, Request<axum::body::Body>, Next) -> MiddlewareFuture
where
    C: Fn(StatusCode) -> Outcome + Clone + Send + Sync + 'static,
{
    move |State(tailtriage), request, next| {
        let classify_status = classify_status.clone();
        Box::pin(async move {
            run_middleware_with_status_classifier(tailtriage, request, next, classify_status).await
        })
    }
}

async fn run_middleware_with_status_classifier<C>(
    tailtriage: Arc<Tailtriage>,
    mut request: Request<axum::body::Body>,
    next: Next,
    classify_status: C,
) -> axum::response::Response
where
    C: Fn(StatusCode) -> Outcome,
{
    let route = request_route_label(&request);
    let started = tailtriage.begin_request_with_owned(route, RequestOptions::new().kind("http"));

    request
        .extensions_mut()
        .insert(TailtriageRequest(started.handle.clone()));

    let response = next.run(request).await;
    let status = response.status();

    started.completion.finish(classify_status(status));
    response
}

/// Handler extractor for the request-scoped instrumentation handle.
#[derive(Debug, Clone)]
pub struct TailtriageRequest(
    /// Request-scoped instrumentation handle created by [`middleware`].
    pub OwnedRequestHandle,
);

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

/// Default HTTP response status to [`Outcome`] classifier for this crate.
#[must_use]
pub fn default_status_to_outcome(status: StatusCode) -> Outcome {
    if status == StatusCode::REQUEST_TIMEOUT {
        Outcome::Timeout
    } else if status.is_server_error() {
        Outcome::Error
    } else if status.is_client_error() {
        Outcome::Rejected
    } else {
        Outcome::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::{crate_name, default_status_to_outcome};
    use axum::http::StatusCode;
    use tailtriage_core::Outcome;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(crate_name(), "tailtriage-axum");
    }

    #[test]
    fn default_status_mapping_matches_http_contract() {
        assert_eq!(default_status_to_outcome(StatusCode::OK), Outcome::Ok);
        assert_eq!(
            default_status_to_outcome(StatusCode::NO_CONTENT),
            Outcome::Ok
        );
        assert_eq!(default_status_to_outcome(StatusCode::FOUND), Outcome::Ok);
        assert_eq!(
            default_status_to_outcome(StatusCode::BAD_REQUEST),
            Outcome::Rejected
        );
        assert_eq!(
            default_status_to_outcome(StatusCode::UNAUTHORIZED),
            Outcome::Rejected
        );
        assert_eq!(
            default_status_to_outcome(StatusCode::FORBIDDEN),
            Outcome::Rejected
        );
        assert_eq!(
            default_status_to_outcome(StatusCode::NOT_FOUND),
            Outcome::Rejected
        );
        assert_eq!(
            default_status_to_outcome(StatusCode::CONFLICT),
            Outcome::Rejected
        );
        assert_eq!(
            default_status_to_outcome(StatusCode::UNPROCESSABLE_ENTITY),
            Outcome::Rejected
        );
        assert_eq!(
            default_status_to_outcome(StatusCode::TOO_MANY_REQUESTS),
            Outcome::Rejected
        );
        assert_eq!(
            default_status_to_outcome(StatusCode::REQUEST_TIMEOUT),
            Outcome::Timeout
        );
        assert_eq!(
            default_status_to_outcome(StatusCode::INTERNAL_SERVER_ERROR),
            Outcome::Error
        );
        assert_eq!(
            default_status_to_outcome(StatusCode::SERVICE_UNAVAILABLE),
            Outcome::Error
        );
    }
}
