use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use axum::Router;
use tailtriage_axum::TailtriageRequest;
use tailtriage_core::Tailtriage;
use tokio::sync::Semaphore;
use tower::ServiceExt;

#[derive(Clone)]
struct AppState {
    gate: Arc<Semaphore>,
}

async fn ok_handler(
    TailtriageRequest(req): TailtriageRequest,
    State(state): State<AppState>,
) -> StatusCode {
    let _permit = req
        .queue("worker_queue")
        .await_on(state.gate.clone().acquire_owned())
        .await
        .expect("permit should be available");
    let _: Result<(), ()> = req.stage("db_stage").await_on(async { Ok(()) }).await;
    StatusCode::OK
}

async fn failure_handler(TailtriageRequest(_): TailtriageRequest) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}

async fn bad_request_handler(TailtriageRequest(_): TailtriageRequest) -> StatusCode {
    StatusCode::BAD_REQUEST
}

async fn timeout_handler(TailtriageRequest(_): TailtriageRequest) -> StatusCode {
    StatusCode::REQUEST_TIMEOUT
}

#[tokio::test(flavor = "current_thread")]
async fn middleware_injects_request_handle_and_finishes_from_response_status() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let artifact = std::env::temp_dir().join(format!("tailtriage-axum-adapter-{nanos}.json"));

    let tailtriage = Arc::new(
        Tailtriage::builder("axum-adapter-test")
            .output(&artifact)
            .build()
            .expect("build should succeed"),
    );

    let app_state = AppState {
        gate: Arc::new(Semaphore::new(1)),
    };

    let app = Router::new()
        .route("/ok", get(ok_handler))
        .route("/fail", get(failure_handler))
        .layer(from_fn_with_state(
            Arc::clone(&tailtriage),
            tailtriage_axum::middleware,
        ))
        .with_state(app_state);

    let ok = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/ok")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("ok request should succeed");
    assert_eq!(ok.status(), StatusCode::OK);

    let fail = app
        .oneshot(
            Request::builder()
                .uri("/fail")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("fail request should succeed");
    assert_eq!(fail.status(), StatusCode::INTERNAL_SERVER_ERROR);

    tailtriage.shutdown().expect("shutdown should succeed");

    let bytes = std::fs::read(&artifact).expect("artifact should exist");
    let run: tailtriage_core::Run = serde_json::from_slice(&bytes).expect("artifact parses");

    assert_eq!(run.requests.len(), 2);
    assert!(run.requests.iter().any(|req| req.route == "/ok"));
    assert!(run.requests.iter().any(|req| req.route == "/fail"));
    assert!(run.stages.iter().any(|stage| stage.stage == "db_stage"));
    assert!(run.queues.iter().any(|queue| queue.queue == "worker_queue"));

    let failure = run
        .requests
        .iter()
        .find(|req| req.route == "/fail")
        .expect("failure request should exist");
    assert_eq!(failure.outcome, "error");
}

#[tokio::test(flavor = "current_thread")]
async fn middleware_records_default_http_outcomes_in_snapshot() {
    let tailtriage = Arc::new(
        Tailtriage::builder("axum-adapter-outcomes-test")
            .build()
            .expect("build should succeed"),
    );

    let app = Router::new()
        .route("/ok", get(ok_handler))
        .route("/bad", get(bad_request_handler))
        .route("/timeout", get(timeout_handler))
        .route("/fail", get(failure_handler))
        .layer(from_fn_with_state(
            Arc::clone(&tailtriage),
            tailtriage_axum::middleware,
        ))
        .with_state(AppState {
            gate: Arc::new(Semaphore::new(1)),
        });

    for (route, expected_status) in [
        ("/ok", StatusCode::OK),
        ("/bad", StatusCode::BAD_REQUEST),
        ("/timeout", StatusCode::REQUEST_TIMEOUT),
        ("/fail", StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(route)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        assert_eq!(response.status(), expected_status);
    }

    let snapshot = tailtriage.snapshot();

    assert_eq!(snapshot.requests.len(), 4);

    let outcome_for = |route: &str| {
        snapshot
            .requests
            .iter()
            .find(|request| request.route == route)
            .map(|request| request.outcome.as_str())
            .expect("request route should be present")
    };

    assert_eq!(outcome_for("/ok"), "ok");
    assert_eq!(outcome_for("/bad"), "rejected");
    assert_eq!(outcome_for("/timeout"), "timeout");
    assert_eq!(outcome_for("/fail"), "error");
}
