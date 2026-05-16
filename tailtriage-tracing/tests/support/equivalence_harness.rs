use futures_executor::block_on;
use std::{
    collections::{BTreeMap, BTreeSet},
    thread,
    time::Duration,
};
use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_core::{RequestOptions, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

pub struct ArtifactEquivalenceReport {
    pub details: Vec<String>,
}

pub fn run_equivalence() -> ArtifactEquivalenceReport {
    let native = native_run();
    let tracing = tracing_run();
    let mut details = Vec::new();
    let nreq: BTreeSet<_> = native
        .requests
        .iter()
        .map(|r| r.request_id.clone())
        .collect();
    let treq: BTreeSet<_> = tracing
        .requests
        .iter()
        .map(|r| r.request_id.clone())
        .collect();
    if nreq != treq {
        details.push(format!(
            "request id mismatch: native={nreq:?}, tracing={treq:?}"
        ));
    }
    if native.requests.len() != 3 || tracing.requests.len() != 3 {
        details.push(format!(
            "count mismatch: native={}, tracing={}",
            native.requests.len(),
            tracing.requests.len()
        ));
    }
    let map = |run: &tailtriage_core::Run| {
        let mut m = BTreeMap::new();
        for r in &run.requests {
            m.insert(
                r.request_id.clone(),
                (r.route.clone(), r.outcome.clone(), r.latency_us),
            );
        }
        m
    };
    let nm = map(&native);
    let tm = map(&tracing);
    if nm.keys().collect::<Vec<_>>() != tm.keys().collect::<Vec<_>>() {
        details.push("correlation shape mismatch".into());
    }
    for id in nm.keys() {
        if nm[id].0 != tm[id].0 || nm[id].1 != tm[id].1 {
            details.push(format!("route/outcome mismatch for {id}"));
        }
    }
    if !native.runtime_snapshots.is_empty() || !tracing.runtime_snapshots.is_empty() {
        details.push("runtime_snapshots not empty".into());
    }
    if native.truncation.limits_hit || tracing.truncation.limits_hit {
        details.push("limits_hit should be false".into());
    }
    for run in [&native, &tracing] {
        if run.requests.iter().any(|r| r.latency_us == 0)
            || run.stages.iter().any(|s| s.latency_us == 0)
            || run.queues.iter().any(|q| q.wait_us == 0)
        {
            details.push("non-positive latency signal".into());
        }
    }
    let slow_n = native
        .requests
        .iter()
        .max_by_key(|r| r.latency_us)
        .map(|r| r.request_id.clone());
    let slow_t = tracing
        .requests
        .iter()
        .max_by_key(|r| r.latency_us)
        .map(|r| r.request_id.clone());
    if slow_n != Some("r3".into()) || slow_t != Some("r3".into()) {
        details.push(format!(
            "slowest mismatch: native={slow_n:?}, tracing={slow_t:?}"
        ));
    }
    if analyze_run(&native, AnalyzeOptions::default()).request_count != 3
        || analyze_run(&tracing, AnalyzeOptions::default()).request_count != 3
    {
        details.push("analyzer request_count mismatch".into());
    }
    ArtifactEquivalenceReport { details }
}

fn native_run() -> tailtriage_core::Run {
    let t = Tailtriage::builder("svc").build().unwrap();
    for (id, ms, depth) in [("r1", 2, 1), ("r2", 2, 2), ("r3", 8, 3)] {
        let started = t.begin_request_with("/checkout", RequestOptions::new().request_id(id));
        let req = started.handle;
        block_on(async {
            req.queue("permits")
                .with_depth_at_start(depth)
                .await_on(async {
                    thread::sleep(Duration::from_millis(1));
                })
                .await;
            req.stage("db")
                .await_on(async {
                    thread::sleep(Duration::from_millis(ms));
                    Ok::<(), ()>(())
                })
                .await
                .unwrap();
            req.stage("cache")
                .await_on(async {
                    thread::sleep(Duration::from_millis(1));
                    Ok::<(), ()>(())
                })
                .await
                .unwrap();
        });
        started.completion.finish_ok();
    }
    t.snapshot()
}

fn tracing_run() -> tailtriage_core::Run {
    let recorder = TracingRecorder::builder("svc").build();
    let subscriber = tracing_subscriber::registry().with(recorder.layer());
    tracing::subscriber::with_default(subscriber, || {
        for (id, ms, depth) in [("r1", 2, 1_u64), ("r2", 2, 2_u64), ("r3", 8, 3_u64)] {
            block_on(async {
                let req = tracing::info_span!(
                    "request",
                    tt.kind = "request",
                    tt.request_id = id,
                    tt.route = "/checkout",
                    tt.outcome = "ok"
                );
                {
                    let q = tracing::info_span!(
                        "queue",
                        tt.kind = "queue",
                        tt.request_id = id,
                        tt.queue = "permits",
                        tt.depth_at_start = depth
                    );
                    thread::sleep(Duration::from_millis(1));
                    drop(q);
                }
                {
                    let s = tracing::info_span!(
                        "stage",
                        tt.kind = "stage",
                        tt.request_id = id,
                        tt.stage = "db",
                        tt.success = true
                    );
                    thread::sleep(Duration::from_millis(ms));
                    drop(s);
                }
                {
                    let s = tracing::info_span!(
                        "stage",
                        tt.kind = "stage",
                        tt.request_id = id,
                        tt.stage = "cache",
                        tt.success = true
                    );
                    thread::sleep(Duration::from_millis(1));
                    drop(s);
                }
                drop(req);
            });
        }
    });
    let imported = recorder.snapshot_run().unwrap();
    assert!(imported.warnings().is_empty());
    imported.run().clone()
}
