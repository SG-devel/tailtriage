use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use axum::Router;
use tailtriage_core::Tailtriage;
use tailtriage_tokio::axum::TailtriageRequest;
use tokio::sync::{oneshot, Semaphore};

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
            tailtriage_tokio::axum::middleware,
        ))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind should succeed");
    let addr: SocketAddr = listener.local_addr().expect("addr should succeed");

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client should build");

    let ok = client
        .get(format!("http://{addr}/ok"))
        .send()
        .await
        .expect("ok request should succeed");
    assert_eq!(ok.status(), StatusCode::OK);

    let fail = client
        .get(format!("http://{addr}/fail"))
        .send()
        .await
        .expect("fail request should succeed");
    assert_eq!(fail.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let _ = shutdown_tx.send(());
    server
        .await
        .expect("server task should join")
        .expect("server should stop cleanly");

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
