#![cfg(feature = "live")]

use tailtriage_tracing::{RecorderLimits, TailtriageLayer, TracingSession, TracingSessionBuilder};

#[test]
fn public_live_api_imports_only_session_layer_and_limits() {
    fn accepts_builder(_: TracingSessionBuilder) {}
    fn accepts_layer(_: TailtriageLayer) {}

    let builder = TracingSession::builder("svc").limits(RecorderLimits::default());
    accepts_builder(builder.clone());
    let session = builder.build().expect("session builds");
    accepts_layer(session.layer());
    let imported =
        futures_executor::block_on(session.shutdown()).expect("async shutdown returns run");
    assert_eq!(imported.run().metadata.service_name, "svc");
}
