//! Semantic-convention keys for trace-shaped intake into tailtriage.

/// Span kind key.
pub const TT_KIND: &str = "tt.kind";
/// Request identifier key.
pub const TT_REQUEST_ID: &str = "tt.request_id";
/// Route identifier key.
pub const TT_ROUTE: &str = "tt.route";
/// Stage identifier key.
pub const TT_STAGE: &str = "tt.stage";
/// Queue identifier key.
pub const TT_QUEUE: &str = "tt.queue";
/// Queue depth snapshot key.
pub const TT_DEPTH_AT_START: &str = "tt.depth_at_start";
/// Outcome key.
pub const TT_OUTCOME: &str = "tt.outcome";
/// Success flag key.
pub const TT_SUCCESS: &str = "tt.success";
