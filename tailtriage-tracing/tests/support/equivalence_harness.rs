use futures_executor::block_on;
use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::Duration;
use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_core::{RequestOptions, Run, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

pub struct ArtifactEquivalenceReport {
    pub native_run: Run,
    pub tracing_run: Run,
}

pub fn build_equivalence_report() -> Result<ArtifactEquivalenceReport, String> {
    let native_run = build_native_run()?;
    let tracing_run = build_tracing_run()?;
    Ok(ArtifactEquivalenceReport {
        native_run,
        tracing_run,
    })
}

#[allow(clippy::too_many_lines)]
pub fn assert_semantic_equivalence(report: &ArtifactEquivalenceReport) {
    let native = &report.native_run;
    let tracing = &report.tracing_run;

    assert_eq!(native.requests.len(), 3, "native request count mismatch");
    assert_eq!(tracing.requests.len(), 3, "tracing request count mismatch");

    let n_req: BTreeMap<_, _> = native
        .requests
        .iter()
        .map(|r| (r.request_id.clone(), r))
        .collect();
    let t_req: BTreeMap<_, _> = tracing
        .requests
        .iter()
        .map(|r| (r.request_id.clone(), r))
        .collect();
    assert_eq!(
        n_req.keys().collect::<BTreeSet<_>>(),
        t_req.keys().collect::<BTreeSet<_>>(),
        "request ids mismatch"
    );
    for (id, nr) in &n_req {
        let tr = t_req[id];
        assert_eq!(nr.route, tr.route, "route mismatch for {id}");
        assert_eq!(nr.outcome, tr.outcome, "outcome mismatch for {id}");
        assert!(
            nr.latency_us > 0 && tr.latency_us > 0,
            "non-positive request latency for {id}"
        );
    }

    for request_id in ["r1", "r2", "r3"] {
        let n_stages: Vec<_> = native
            .stages
            .iter()
            .filter(|s| s.request_id == request_id)
            .collect();
        let t_stages: Vec<_> = tracing
            .stages
            .iter()
            .filter(|s| s.request_id == request_id)
            .collect();
        assert_eq!(
            n_stages.len(),
            2,
            "native stage count mismatch for {request_id}"
        );
        assert_eq!(
            t_stages.len(),
            2,
            "tracing stage count mismatch for {request_id}"
        );
        let n_names: BTreeSet<_> = n_stages.iter().map(|s| s.stage.as_str()).collect();
        let t_names: BTreeSet<_> = t_stages.iter().map(|s| s.stage.as_str()).collect();
        assert_eq!(n_names, t_names, "stage names mismatch for {request_id}");
        for n in n_stages {
            assert!(n.latency_us > 0);
        }
        for t in t_stages {
            assert!(t.latency_us > 0);
        }

        let n_queue: Vec<_> = native
            .queues
            .iter()
            .filter(|q| q.request_id == request_id)
            .collect();
        let t_queue: Vec<_> = tracing
            .queues
            .iter()
            .filter(|q| q.request_id == request_id)
            .collect();
        assert_eq!(
            n_queue.len(),
            1,
            "native queue count mismatch for {request_id}"
        );
        assert_eq!(
            t_queue.len(),
            1,
            "tracing queue count mismatch for {request_id}"
        );
        assert_eq!(
            n_queue[0].queue, t_queue[0].queue,
            "queue name mismatch for {request_id}"
        );
        assert_eq!(
            n_queue[0].depth_at_start, t_queue[0].depth_at_start,
            "queue depth mismatch for {request_id}"
        );
        assert!(
            n_queue[0].wait_us > 0 && t_queue[0].wait_us > 0,
            "non-positive queue wait for {request_id}"
        );
    }

    let native_slowest = native
        .requests
        .iter()
        .max_by_key(|r| r.latency_us)
        .map(|r| r.request_id.as_str());
    let tracing_slowest = tracing
        .requests
        .iter()
        .max_by_key(|r| r.latency_us)
        .map(|r| r.request_id.as_str());
    assert_eq!(
        native_slowest,
        Some("r3"),
        "native slowest request mismatch"
    );
    assert_eq!(
        tracing_slowest,
        Some("r3"),
        "tracing slowest request mismatch"
    );
    assert!(
        native.runtime_snapshots.is_empty() && tracing.runtime_snapshots.is_empty(),
        "runtime snapshot mismatch"
    );
    assert!(
        !native.truncation.limits_hit && !tracing.truncation.limits_hit,
        "unexpected truncation limits hit"
    );

    let n_report = analyze_run(native, AnalyzeOptions::default());
    let t_report = analyze_run(tracing, AnalyzeOptions::default());
    assert_eq!(
        n_report.request_count, 3,
        "native analyzer request count mismatch"
    );
    assert_eq!(
        t_report.request_count, 3,
        "tracing analyzer request count mismatch"
    );
}

fn build_native_run() -> Result<Run, String> {
    let tail = Tailtriage::builder("svc")
        .build()
        .map_err(|e| e.to_string())?;
    for (id, slow) in [("r1", false), ("r2", false), ("r3", true)] {
        let started = tail.begin_request_with(
            "/checkout",
            RequestOptions::new().request_id(id.to_string()),
        );
        let q = started.handle.queue("admission");
        sleep_us(if slow { 1200 } else { 200 });
        drop(q);
        let s1 = started.handle.stage("db");
        sleep_us(if slow { 1600 } else { 300 });
        drop(s1);
        let s2 = started.handle.stage("cache");
        sleep_us(if slow { 900 } else { 180 });
        drop(s2);
        started.completion.finish_ok();
    }
    Ok(tail.snapshot())
}

fn build_tracing_run() -> Result<Run, String> {
    let recorder = TracingRecorder::builder("svc").build();
    let subscriber = tracing_subscriber::registry().with(recorder.layer());
    tracing::subscriber::with_default(subscriber, || {
        block_on(async {
            for (id, slow) in [("r1", false), ("r2", false), ("r3", true)] {
                let request = tracing::info_span!(
                    "request",
                    tt.kind = "request",
                    tt.request_id = id,
                    tt.route = "/checkout",
                    tt.outcome = "ok"
                );
                {
                    let queue = tracing::info_span!(
                        "queue",
                        tt.kind = "queue",
                        tt.request_id = id,
                        tt.queue = "admission",
                        tt.depth_at_start = 3_u64
                    );
                    sleep_us(if slow { 1200 } else { 200 });
                    drop(queue);
                }
                {
                    let stage = tracing::info_span!(
                        "stage",
                        tt.kind = "stage",
                        tt.request_id = id,
                        tt.stage = "db",
                        tt.success = true
                    );
                    sleep_us(if slow { 1600 } else { 300 });
                    drop(stage);
                }
                {
                    let stage = tracing::info_span!(
                        "stage",
                        tt.kind = "stage",
                        tt.request_id = id,
                        tt.stage = "cache",
                        tt.success = true
                    );
                    sleep_us(if slow { 900 } else { 180 });
                    drop(stage);
                }
                drop(request);
            }
        });
    });
    let imported = recorder.snapshot_run().map_err(|e| e.to_string())?;
    if !imported.warnings().is_empty() {
        return Err(format!(
            "unexpected tracing import warnings: {:?}",
            imported.warnings()
        ));
    }
    Ok(imported.run().clone())
}

fn sleep_us(us: u64) {
    thread::sleep(Duration::from_micros(us));
}
