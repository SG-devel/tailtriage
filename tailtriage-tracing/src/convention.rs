//! Semantic convention keys for tracing-shaped tailtriage intake.

/// Field key for the span kind (`request`, `stage`, or `queue`).
pub const TT_KIND: &str = "tt.kind";
/// Field key for a request identifier.
pub const TT_REQUEST_ID: &str = "tt.request_id";
/// Field key for the normalized route name.
pub const TT_ROUTE: &str = "tt.route";
/// Field key for a stage name.
pub const TT_STAGE: &str = "tt.stage";
/// Field key for a queue name.
pub const TT_QUEUE: &str = "tt.queue";
/// Field key for queue depth observed at queue start.
pub const TT_DEPTH_AT_START: &str = "tt.depth_at_start";
/// Field key for request outcome label.
pub const TT_OUTCOME: &str = "tt.outcome";
/// Field key for boolean stage success.
pub const TT_SUCCESS: &str = "tt.success";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_expected_keys() {
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
