use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::Duration;

use futures_executor::block_on;
use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_core::{MemorySink, Run, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

pub fn assert_native_and_tracing_semantic_parity() {
    let native_run = native_run();
    let (tracing_run, warnings) = tracing_run();

    let report = compare_runs(&native_run, &tracing_run);
    assert!(
        report.mismatches.is_empty(),
        "semantic parity mismatches: {}",
        report.mismatches.join("; ")
    );
    assert!(
        warnings.is_empty(),
        "unexpected tracing warnings: {warnings:?}"
    );
}

struct ArtifactEquivalenceReport {
    mismatches: Vec<String>,
}

fn native_run() -> Run {
    let sink = MemorySink::default();
    let tt = Tailtriage::builder("svc")
        .sink(sink.clone())
        .build()
        .unwrap();
    for (id, slow) in [("r1", false), ("r2", true), ("r3", false)] {
        let started = tt.begin_request_with(
            "/checkout",
            tailtriage_core::RequestOptions::new().request_id(id),
        );
        block_on(
            started
                .handle
                .queue("permits")
                .with_depth_at_start(3)
                .await_on(async {
                    thread::sleep(Duration::from_millis(if slow { 4 } else { 1 }));
                }),
        );
        block_on(started.handle.stage("db").await_on(async {
            thread::sleep(Duration::from_millis(if slow { 6 } else { 2 }));
            Ok::<(), std::io::Error>(())
        }))
        .unwrap();
        block_on(started.handle.stage("cache").await_on(async {
            thread::sleep(Duration::from_millis(1));
            Ok::<(), std::io::Error>(())
        }))
        .unwrap();
        started.completion.finish_ok();
    }
    tt.shutdown().unwrap();
    sink.last_run().unwrap()
}

fn tracing_run() -> (Run, Vec<String>) {
    let recorder = TracingRecorder::builder("svc").build();
    let subscriber = tracing_subscriber::registry().with(recorder.layer());
    tracing::subscriber::with_default(subscriber, || {
        block_on(async {
            for (id, slow) in [("r1", false), ("r2", true), ("r3", false)] {
                let request = tracing::info_span!(
                    "request",
                    tt.kind = "request",
                    tt.request_id = id,
                    tt.route = "/checkout"
                );
                {
                    let _request_guard = request.enter();

                    {
                        let queue = tracing::info_span!(
                            "queue",
                            tt.kind = "queue",
                            tt.request_id = id,
                            tt.queue = "permits",
                            tt.depth_at_start = 3_u64
                        );
                        {
                            let _queue_guard = queue.enter();
                            thread::sleep(Duration::from_millis(if slow { 4 } else { 1 }));
                        }
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
                        {
                            let _stage_guard = stage.enter();
                            thread::sleep(Duration::from_millis(if slow { 6 } else { 2 }));
                        }
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
                        {
                            let _stage_guard = stage.enter();
                            thread::sleep(Duration::from_millis(1));
                        }
                        drop(stage);
                    }
                }
                drop(request);
            }
        });
    });
    let imported = recorder.snapshot_run().unwrap();
    let warnings = imported
        .warnings()
        .iter()
        .map(|w| w.message().to_owned())
        .collect();
    (imported.run().clone(), warnings)
}

#[allow(clippy::too_many_lines)]
fn compare_runs(native_run: &Run, tracing_run: &Run) -> ArtifactEquivalenceReport {
    let mut mismatches = Vec::new();
    if native_run.runtime_snapshots != tracing_run.runtime_snapshots {
        mismatches.push("runtime snapshots mismatch".to_owned());
    }
    if native_run.truncation.limits_hit || tracing_run.truncation.limits_hit {
        mismatches.push("truncation.limits_hit must be false for both runs".to_owned());
    }

    let nreq: BTreeMap<_, _> = native_run
        .requests
        .iter()
        .map(|r| {
            (
                r.request_id.clone(),
                (r.route.clone(), r.outcome.clone(), r.latency_us),
            )
        })
        .collect();
    let treq: BTreeMap<_, _> = tracing_run
        .requests
        .iter()
        .map(|r| {
            (
                r.request_id.clone(),
                (r.route.clone(), r.outcome.clone(), r.latency_us),
            )
        })
        .collect();
    if nreq.keys().collect::<BTreeSet<_>>() != treq.keys().collect::<BTreeSet<_>>() {
        mismatches.push("request id set mismatch".to_owned());
    }
    for (id, (route, outcome, lat)) in &nreq {
        if *lat == 0 {
            mismatches.push(format!("native request {id} latency not positive"));
        }
        match treq.get(id) {
            Some((tr, to, tl)) => {
                if route != tr {
                    mismatches.push(format!("route mismatch for {id}"));
                }
                if outcome != to {
                    mismatches.push(format!("outcome mismatch for {id}"));
                }
                if *tl == 0 {
                    mismatches.push(format!("tracing request {id} latency not positive"));
                }
            }
            None => mismatches.push(format!("missing tracing request {id}")),
        }
    }

    let slow_native = nreq
        .iter()
        .max_by_key(|(_, (_, _, lat))| *lat)
        .map(|(id, _)| id.clone());
    let slow_tracing = treq
        .iter()
        .max_by_key(|(_, (_, _, lat))| *lat)
        .map(|(id, _)| id.clone());
    if slow_native != slow_tracing {
        mismatches.push("slowest request mismatch".to_owned());
    }

    let nstage: BTreeSet<_> = native_run
        .stages
        .iter()
        .map(|s| (s.request_id.clone(), s.stage.clone(), s.success))
        .collect();
    let tstage: BTreeSet<_> = tracing_run
        .stages
        .iter()
        .map(|s| (s.request_id.clone(), s.stage.clone(), s.success))
        .collect();
    if nstage != tstage {
        mismatches.push("stage correlation shape differs".to_owned());
    }
    if native_run.stages.iter().any(|s| s.latency_us == 0)
        || tracing_run.stages.iter().any(|s| s.latency_us == 0)
    {
        mismatches.push("stage latencies must be positive".to_owned());
    }

    let nqueue: BTreeSet<_> = native_run
        .queues
        .iter()
        .map(|q| (q.request_id.clone(), q.queue.clone(), q.depth_at_start))
        .collect();
    let tqueue: BTreeSet<_> = tracing_run
        .queues
        .iter()
        .map(|q| (q.request_id.clone(), q.queue.clone(), q.depth_at_start))
        .collect();
    if nqueue != tqueue {
        mismatches.push("queue correlation shape differs".to_owned());
    }
    if native_run.queues.iter().any(|q| q.wait_us == 0)
        || tracing_run.queues.iter().any(|q| q.wait_us == 0)
    {
        mismatches.push("queue latencies must be positive".to_owned());
    }

    if analyze_run(native_run, AnalyzeOptions::default()).request_count != 3
        || analyze_run(tracing_run, AnalyzeOptions::default()).request_count != 3
    {
        mismatches.push("analyzer request_count must equal 3 for both runs".to_owned());
    }
    ArtifactEquivalenceReport { mismatches }
}
