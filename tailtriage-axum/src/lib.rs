#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

// Axum adoption helpers layered on top of `tailtriage-core`.
//
// This crate provides a focused middleware + extractor path so handlers can
// access request instrumentation without repeating request start/finish wiring.

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

/// Axum middleware for the default tailtriage HTTP request boundary.
///
/// Install this function with [`axum::middleware::from_fn_with_state`], using
/// the same `Arc<Tailtriage>` as its middleware state. Before calling
/// `next.run(request)`, it starts an owned request with `kind = "http"` and
/// inserts [`TailtriageRequest`] into the request extensions for downstream
/// extraction.
///
/// # Measurement boundary
///
/// Capture starts before `next.run(request)`. Once that future returns a
/// [`Response`](axum::response::Response), the returned response status is
/// passed to [`default_status_to_outcome`] and the middleware explicitly
/// finishes the request. Thus this interval measures response production
/// through return of the Axum response. It does not await subsequent polling or
/// consumption of the response body; a streaming body's lifetime, later body
/// errors, socket flush, and client-observed latency are outside this boundary.
///
/// If the middleware future is dropped, aborted, or unwinds after admission but
/// before explicit finish, the owned completion token's `Drop` records one
/// cancelled request while capture remains open. Explicit finish disarms that
/// behavior; a capacity-refused token and a token dropped after core
/// finalization are inert. Cancellation is evidence about this observed
/// middleware lifecycle boundary, not proof that handler, task, or network work
/// stopped.
///
/// # Labels and outcome
///
/// The route label uses Axum [`MatchedPath`] when present. Otherwise it uses
/// `request.uri().path()`: the URI path without the query string, not a
/// normalized route template. Concrete identifiers in this fallback can
/// increase label cardinality. The default status mapping is 408 to
/// [`Outcome::Timeout`], other 4xx statuses to [`Outcome::Rejected`], 5xx
/// statuses to [`Outcome::Error`], and every other status to [`Outcome::Ok`].
///
/// The middleware owns only request-boundary completion. Application code must
/// explicitly record internal queue, stage, and in-flight evidence and remains
/// responsible for calling [`Tailtriage::shutdown`] for the overall collector.
pub async fn middleware(
    State(tailtriage): State<Arc<Tailtriage>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> axum::response::Response {
    run_middleware_with_status_classifier(tailtriage, request, next, default_status_to_outcome)
        .await
}

/// Returns Axum middleware using a caller-owned status-to-outcome policy.
///
/// Install the returned function with
/// [`axum::middleware::from_fn_with_state`] and an `Arc<Tailtriage>` state, as
/// for [`middleware`]. Before `next.run(request)`, it starts an owned request
/// with `kind = "http"` and inserts [`TailtriageRequest`] into the request
/// extensions. The route label prefers [`MatchedPath`] and otherwise uses
/// `request.uri().path()` (the path without its query string). That fallback is
/// not a normalized template and concrete path identifiers can increase label
/// cardinality.
///
/// After `next.run(request)` returns an [`axum::response::Response`], the
/// supplied classifier receives that response's [`StatusCode`], and its
/// returned [`Outcome`] explicitly finishes the request. The caller completely
/// owns this mapping policy. The classifier does not observe panics, request
/// errors, later body errors, or body completion. Response-body polling and
/// consumption happen outside this measurement boundary, so a streaming body
/// can outlive the recorded request.
///
/// If the middleware future is dropped, aborted, or unwinds before explicit
/// completion, an admitted completion token records cancellation on `Drop`
/// while capture is open. Capacity-refused tokens and late drops after core
/// finalization are inert. That cancellation describes the observed middleware
/// lifecycle; it does not prove that underlying handler, task, or network work
/// stopped.
///
/// A custom mapping does not instrument internal queues, stages, or in-flight
/// work. Application code must record those explicitly and remains responsible
/// for [`Tailtriage::shutdown`] of the overall collector.
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
    let started = tailtriage.begin_owned_request_with(route, RequestOptions::new().kind("http"));

    request
        .extensions_mut()
        .insert(TailtriageRequest(started.handle.clone()));

    let response = next.run(request).await;
    let status = response.status();

    started.completion.finish(classify_status(status));
    response
}

/// Request-scoped instrumentation handle installed by the Axum middleware.
///
/// The public tuple field is an [`OwnedRequestHandle`], so handler code may use
/// either `TailtriageRequest(request)` destructuring or [`Self::into_inner`].
/// Cloning this wrapper (or its handle) clones access to request instrumentation
/// for queues, stages, and in-flight evidence; it does not clone request
/// completion ownership. The middleware retains its separate completion token.
///
/// Extraction clones this value from request extensions. It succeeds only when
/// the request reaching the extractor already contains context inserted by
/// [`middleware`] or [`middleware_with_status_classifier`]. Missing context
/// produces [`TailtriageExtractorError`]. This wrapper does not complete the
/// HTTP request or shut down the collector.
#[derive(Debug, Clone)]
pub struct TailtriageRequest(
    /// Public request-scoped instrumentation handle created by the middleware.
    pub OwnedRequestHandle,
);

impl TailtriageRequest {
    /// Consumes the wrapper and returns its owned instrumentation handle.
    ///
    /// This transfers only the wrapper-held handle value. The middleware keeps
    /// the separate token that owns completion of its request boundary.
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

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> {
        std::future::ready(
            parts
                .extensions
                .get::<TailtriageRequest>()
                .cloned()
                .ok_or(TailtriageExtractorError),
        )
    }
}

/// Rejection returned when the expected Tailtriage request context is absent.
///
/// Its [`IntoResponse`] implementation returns HTTP 500 with a message asking
/// for `tailtriage_axum::middleware`. This generally indicates that middleware
/// wiring or ordering did not insert the extension for this handler path.
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

/// Maps an HTTP response status to the default [`Outcome`] for this crate.
///
/// The exact mapping is:
///
/// - 408 Request Timeout -> [`Outcome::Timeout`]
/// - every other 4xx status -> [`Outcome::Rejected`]
/// - every 5xx status -> [`Outcome::Error`]
/// - every other status -> [`Outcome::Ok`]
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
    use super::default_status_to_outcome;
    use axum::http::StatusCode;
    use tailtriage_core::Outcome;

    // TT-TEST: X02 primary
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
