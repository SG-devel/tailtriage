//! Semantic convention keys for `tailtriage` tracing intake.

/// Span kind marker field key.
pub const TT_KIND: &str = "tt.kind";
/// Logical request identifier field key.
pub const TT_REQUEST_ID: &str = "tt.request_id";
/// Route or endpoint field key.
pub const TT_ROUTE: &str = "tt.route";
/// Stage name field key.
pub const TT_STAGE: &str = "tt.stage";
/// Queue name field key.
pub const TT_QUEUE: &str = "tt.queue";
/// Queue depth snapshot field key captured at queue wait start.
pub const TT_DEPTH_AT_START: &str = "tt.depth_at_start";
/// Outcome field key for request completion.
pub const TT_OUTCOME: &str = "tt.outcome";
/// Success indicator field key.
pub const TT_SUCCESS: &str = "tt.success";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tt_semantic_keys_match_expected_values() {
        assert_eq!(TT_KIND, "tt.kind");
        assert_eq!(TT_REQUEST_ID, "tt.request_id");
        assert_eq!(TT_ROUTE, "tt.route");
        assert_eq!(TT_STAGE, "tt.stage");
        assert_eq!(TT_QUEUE, "tt.queue");
        assert_eq!(TT_DEPTH_AT_START, "tt.depth_at_start");
        assert_eq!(TT_OUTCOME, "tt.outcome");
        assert_eq!(TT_SUCCESS, "tt.success");
    }
}
