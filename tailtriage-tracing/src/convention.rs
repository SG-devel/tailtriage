//! Semantic convention keys for tracing intake fields.

/// Span kind field key (`request`, `stage`, or `queue`).
pub const TT_KIND: &str = "tt.kind";
/// Request identifier field key.
pub const TT_REQUEST_ID: &str = "tt.request_id";
/// Route or operation name field key.
pub const TT_ROUTE: &str = "tt.route";
/// Stage name field key.
pub const TT_STAGE: &str = "tt.stage";
/// Queue name field key.
pub const TT_QUEUE: &str = "tt.queue";
/// Queue depth-at-start field key.
pub const TT_DEPTH_AT_START: &str = "tt.depth_at_start";
/// Outcome text field key.
pub const TT_OUTCOME: &str = "tt.outcome";
/// Success boolean field key.
pub const TT_SUCCESS: &str = "tt.success";
