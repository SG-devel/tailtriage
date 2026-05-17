use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::Duration;

use futures_executor::block_on;
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions, Report};
use tailtriage_core::{MemorySink, Run, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

#[allow(dead_code)]
pub struct RunParityReport {
    pub mismatches: Vec<String>,
    pub request_count_native: usize,
    pub request_count_tracing: usize,
    pub stage_count_native: usize,
    pub stage_count_tracing: usize,
    pub queue_count_native: usize,
    pub queue_count_tracing: usize,
    pub slowest_request_id_native: Option<String>,
    pub slowest_request_id_tracing: Option<String>,
}

#[allow(dead_code)]
pub struct AnalyzerParityReport {
    pub mismatches: Vec<String>,
    pub request_count_native: usize,
    pub request_count_tracing: usize,
    pub primary_suspect_native: String,
    pub primary_suspect_tracing: String,
    pub primary_score_native: u8,
    pub primary_score_tracing: u8,
}

#[allow(dead_code)]
pub struct RenderedReportParityReport {
    pub mismatches: Vec<String>,
    pub normalized_text_native: String,
    pub normalized_text_tracing: String,
}

#[allow(dead_code)]
pub struct ParityReport {
    pub mismatches: Vec<String>,
    pub run_report: RunParityReport,
    pub analyzer_report: AnalyzerParityReport,
    pub rendered_report: RenderedReportParityReport,
}

pub fn assert_native_and_tracing_full_parity() {
    let native = native_run();
    let (tracing_run, warnings) = tracing_run();
    let report = build_full_parity_report(&native, &tracing_run);

    assert!(
        warnings.is_empty(),
        "unexpected tracing warnings: {warnings:?}"
    );
    assert!(
        report.mismatches.is_empty(),
        "full parity mismatches:\n{}",
        report.mismatches.join("\n")
    );
}

#[test]
fn parity_report_detects_queue_mismatch() {
    let native = native_run();
    let mut tracing_run = native.clone();
    let queue = tracing_run
        .queues
        .iter_mut()
        .find(|q| q.request_id == "r2")
        .expect("r2 queue should exist");
    queue.queue = "permits_changed".to_owned();

    let report = build_full_parity_report(&native, &tracing_run);
    assert!(
        report
            .run_report
            .mismatches
            .iter()
            .any(|m| m.contains("queue set mismatch")),
        "expected queue mismatch, got: {:?}",
        report.run_report.mismatches
    );
}

#[test]
fn report_normalization_keeps_semantic_differences_visible() {
    let native = native_run();
    let mut tracing_run = native.clone();
    tracing_run.requests.clear();
    tracing_run.stages.clear();
    tracing_run.queues.clear();

    let report = build_full_parity_report(&native, &tracing_run);
    assert!(
        report
            .rendered_report
            .mismatches
            .iter()
            .any(|m| m.contains("semantic token mismatch") || m.contains("primary suspect text")),
        "normalization should not hide semantic differences: {:?}",
        report.rendered_report.mismatches
    );
}

fn build_full_parity_report(native_run: &Run, tracing_run: &Run) -> ParityReport {
    let run_report = compare_runs(native_run, tracing_run);
    let native_analysis = analyze_run(native_run, AnalyzeOptions::default());
    let tracing_analysis = analyze_run(tracing_run, AnalyzeOptions::default());
    let analyzer_report = compare_analyzer_reports(&native_analysis, &tracing_analysis);
    let rendered_report = compare_rendered_reports(&native_analysis, &tracing_analysis);

    let mut mismatches = Vec::new();
    mismatches.extend(run_report.mismatches.iter().map(|m| format!("[run] {m}")));
    mismatches.extend(
        analyzer_report
            .mismatches
            .iter()
            .map(|m| format!("[analyzer] {m}")),
    );
    mismatches.extend(
        rendered_report
            .mismatches
            .iter()
            .map(|m| format!("[rendered] {m}")),
    );

    ParityReport {
        mismatches,
        run_report,
        analyzer_report,
        rendered_report,
    }
}

fn native_run() -> Run {
    /* unchanged scenario */
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
    /* unchanged scenario */
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
fn compare_runs(native_run: &Run, tracing_run: &Run) -> RunParityReport {
    let mut mismatches = Vec::new();
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
        if let Some((tr, to, tl)) = treq.get(id) {
            if route != tr {
                mismatches.push(format!(
                    "route mismatch for {id}: native={route} tracing={tr}"
                ));
            }
            if outcome != to {
                mismatches.push(format!("outcome mismatch for {id}"));
            }
            if *tl == 0 {
                mismatches.push(format!("tracing request {id} latency not positive"));
            }
        }
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
        mismatches.push("stage set mismatch".to_owned());
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
        mismatches.push(format!(
            "queue set mismatch: native={nqueue:?} tracing={tqueue:?}"
        ));
    }

    if native_run.runtime_snapshots != tracing_run.runtime_snapshots
        || !native_run.runtime_snapshots.is_empty()
    {
        mismatches.push("runtime snapshots must match and be empty".to_owned());
    }
    if native_run.truncation.limits_hit || tracing_run.truncation.limits_hit {
        mismatches.push("truncation.limits_hit must be false".to_owned());
    }
    if native_run.requests.len() != tracing_run.requests.len() {
        mismatches.push("request count mismatch".to_owned());
    }
    if native_run.stages.len() != tracing_run.stages.len() {
        mismatches.push("stage count mismatch".to_owned());
    }
    if native_run.queues.len() != tracing_run.queues.len() {
        mismatches.push("queue count mismatch".to_owned());
    }
    if native_run.stages.iter().any(|s| s.latency_us == 0)
        || tracing_run.stages.iter().any(|s| s.latency_us == 0)
    {
        mismatches.push("all stage durations must be positive".to_owned());
    }
    if native_run.queues.iter().any(|q| q.wait_us == 0)
        || tracing_run.queues.iter().any(|q| q.wait_us == 0)
    {
        mismatches.push("all queue durations must be positive".to_owned());
    }

    let slow_n = nreq
        .iter()
        .max_by_key(|(_, (_, _, lat))| *lat)
        .map(|(id, _)| id.clone());
    let slow_t = treq
        .iter()
        .max_by_key(|(_, (_, _, lat))| *lat)
        .map(|(id, _)| id.clone());
    if slow_n != slow_t {
        mismatches.push(format!(
            "slowest request mismatch: native={slow_n:?} tracing={slow_t:?}"
        ));
    }

    RunParityReport {
        mismatches,
        request_count_native: native_run.requests.len(),
        request_count_tracing: tracing_run.requests.len(),
        stage_count_native: native_run.stages.len(),
        stage_count_tracing: tracing_run.stages.len(),
        queue_count_native: native_run.queues.len(),
        queue_count_tracing: tracing_run.queues.len(),
        slowest_request_id_native: slow_n,
        slowest_request_id_tracing: slow_t,
    }
}

fn compare_analyzer_reports(native: &Report, tracing: &Report) -> AnalyzerParityReport {
    let mut mismatches = Vec::new();
    if native.request_count != tracing.request_count {
        mismatches.push("request_count mismatch".to_owned());
    }
    if native.primary_suspect.kind != tracing.primary_suspect.kind {
        mismatches.push(format!(
            "primary suspect mismatch: native={} tracing={}",
            native.primary_suspect.kind.as_str(),
            tracing.primary_suspect.kind.as_str()
        ));
    }
    if native.p95_latency_us.unwrap_or(0) == 0 || tracing.p95_latency_us.unwrap_or(0) == 0 {
        mismatches.push("p95 latency must be present and non-zero".to_owned());
    }
    if native.p99_latency_us.unwrap_or(0) == 0 || tracing.p99_latency_us.unwrap_or(0) == 0 {
        mismatches.push("p99 latency must be present and non-zero".to_owned());
    }

    let labels = |report: &Report| {
        let mut labels = BTreeSet::new();
        for text in &report.primary_suspect.evidence {
            if text.contains("/checkout") {
                labels.insert("route:/checkout".to_owned());
            }
            if text.contains("db") {
                labels.insert("stage:db".to_owned());
            }
            if text.contains("cache") {
                labels.insert("stage:cache".to_owned());
            }
            if text.contains("permits") || text.contains("queue") {
                labels.insert("queue:permits_or_queue_signal".to_owned());
            }
        }
        labels
    };
    let nlabels = labels(native);
    let tlabels = labels(tracing);
    if nlabels != tlabels {
        mismatches.push(format!(
            "evidence labels differ: native={nlabels:?} tracing={tlabels:?}"
        ));
    }

    AnalyzerParityReport {
        mismatches,
        request_count_native: native.request_count,
        request_count_tracing: tracing.request_count,
        primary_suspect_native: native.primary_suspect.kind.as_str().to_owned(),
        primary_suspect_tracing: tracing.primary_suspect.kind.as_str().to_owned(),
        primary_score_native: native.primary_suspect.score,
        primary_score_tracing: tracing.primary_suspect.score,
    }
}

fn normalize_rendered_text(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.contains("run ")
                || line.contains("captured")
                || line.contains("latency")
                || line.contains("p95")
                || line.contains("p99")
                || line.contains("us")
            {
                "<normalized-unstable>".to_owned()
            } else {
                line.trim().to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn compare_rendered_reports(native: &Report, tracing: &Report) -> RenderedReportParityReport {
    let n = normalize_rendered_text(&render_text(native));
    let t = normalize_rendered_text(&render_text(tracing));
    let mut mismatches = Vec::new();
    if n.trim().is_empty() || t.trim().is_empty() {
        mismatches.push("rendered text must be non-empty".to_owned());
    }
    if !n.to_lowercase().contains("next checks") || !t.to_lowercase().contains("next checks") {
        mismatches.push("missing key section: next checks".to_owned());
    }
    let semantic_tokens = |text: &str| {
        let lower = text.to_lowercase();
        let mut tokens = BTreeSet::new();
        for token in ["/checkout", "db", "cache", "permits", "next checks"] {
            if lower.contains(token) {
                tokens.insert(token.to_owned());
            }
        }
        tokens
    };
    let ntokens = semantic_tokens(&n);
    let ttokens = semantic_tokens(&t);
    if ntokens != ttokens {
        mismatches.push(format!(
            "semantic token mismatch after normalization: native={ntokens:?} tracing={ttokens:?}"
        ));
    }
    let np = native.primary_suspect.kind.as_str();
    let tp = tracing.primary_suspect.kind.as_str();
    if np != tp {
        mismatches.push(format!(
            "primary suspect text differs: native={np} tracing={tp}"
        ));
    }
    RenderedReportParityReport {
        mismatches,
        normalized_text_native: n,
        normalized_text_tracing: t,
    }
}
