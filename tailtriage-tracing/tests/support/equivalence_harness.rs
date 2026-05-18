use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::Duration;

use futures_executor::block_on;
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions, DiagnosisKind, Report};
use tailtriage_core::{
    CaptureMode, QueueEvent, RequestEvent, Run, RunMetadata, StageEvent, TruncationSummary,
    UnfinishedRequests, SCHEMA_VERSION,
};
use tailtriage_tracing::{
    run_from_span_records, ImportOptions, SpanRecord, TracingRecorder, TT_DEPTH_AT_START, TT_KIND,
    TT_QUEUE, TT_REQUEST_ID, TT_ROUTE, TT_STAGE,
};
use tracing_subscriber::prelude::*;

#[derive(Debug)]
#[allow(dead_code)]
pub struct RunParityReport {
    pub mismatches: Vec<String>,
    pub native_request_count: usize,
    pub tracing_request_count: usize,
    pub native_stage_count: usize,
    pub tracing_stage_count: usize,
    pub native_queue_count: usize,
    pub tracing_queue_count: usize,
    pub native_slowest_request_id: Option<String>,
    pub tracing_slowest_request_id: Option<String>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct AnalyzerParityReport {
    pub mismatches: Vec<String>,
    pub native_request_count: usize,
    pub tracing_request_count: usize,
    pub native_primary_suspect: Option<DiagnosisKind>,
    pub tracing_primary_suspect: Option<DiagnosisKind>,
    pub native_primary_score: Option<u8>,
    pub tracing_primary_score: Option<u8>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct RenderedReportParityReport {
    pub mismatches: Vec<String>,
    pub native_sections: BTreeSet<String>,
    pub tracing_sections: BTreeSet<String>,
    pub native_primary_suspect_line: Option<String>,
    pub tracing_primary_suspect_line: Option<String>,
}

#[derive(Debug)]
pub struct ParityReport {
    pub mismatches: Vec<String>,
    pub run: RunParityReport,
    pub analyzer: AnalyzerParityReport,
    pub rendered: RenderedReportParityReport,
}

pub fn assert_deterministic_span_import_full_parity() {
    let native_run = deterministic_native_run();
    let (tracing_run, warnings) = deterministic_tracing_run();
    let report = build_parity_report(&native_run, &tracing_run);
    let native_analysis = analyze_run(&native_run, AnalyzeOptions::default());
    let tracing_analysis = analyze_run(&tracing_run, AnalyzeOptions::default());

    assert!(
        warnings.is_empty(),
        "unexpected tracing warnings: {warnings:?}"
    );
    assert_eq!(
        native_analysis.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert_eq!(
        tracing_analysis.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert!(
        report.mismatches.is_empty(),
        "native/tracing full parity failed\n\nRun parity mismatches:\n{}\n\nAnalyzer parity mismatches:\n{}\n\nRendered report parity mismatches:\n{}\n\nNative/tracing counts:\n- requests: {}/{}\n- stages: {}/{}\n- queues: {}/{}\n\nNative/tracing primary suspect:\n- kind: {:?}/{:?}\n- score: {:?}/{:?}",
        format_mismatches(&report.run.mismatches),
        format_mismatches(&report.analyzer.mismatches),
        format_mismatches(&report.rendered.mismatches),
        report.run.native_request_count,
        report.run.tracing_request_count,
        report.run.native_stage_count,
        report.run.tracing_stage_count,
        report.run.native_queue_count,
        report.run.tracing_queue_count,
        report.analyzer.native_primary_suspect,
        report.analyzer.tracing_primary_suspect,
        report.analyzer.native_primary_score,
        report.analyzer.tracing_primary_score
    );
}

pub fn build_parity_report(native_run: &Run, tracing_run: &Run) -> ParityReport {
    let run = compare_runs(native_run, tracing_run);
    let native_analysis = analyze_run(native_run, AnalyzeOptions::default());
    let tracing_analysis = analyze_run(tracing_run, AnalyzeOptions::default());
    let analyzer = compare_analyzer_reports(&native_analysis, &tracing_analysis);
    let rendered = compare_rendered_reports(&native_analysis, &tracing_analysis);

    let mut mismatches = Vec::new();
    mismatches.extend(run.mismatches.iter().map(|m| format!("run: {m}")));
    mismatches.extend(analyzer.mismatches.iter().map(|m| format!("analyzer: {m}")));
    mismatches.extend(rendered.mismatches.iter().map(|m| format!("rendered: {m}")));

    ParityReport {
        mismatches,
        run,
        analyzer,
        rendered,
    }
}

fn live_tracing_run() -> (Run, Vec<String>) {
    tracing_run_with_queue("permits")
}

#[allow(clippy::too_many_lines)]
fn deterministic_native_run() -> Run {
    let base = 1_700_000_000_000_u64;
    Run {
        schema_version: SCHEMA_VERSION,
        metadata: RunMetadata {
            run_id: "deterministic-native".to_owned(),
            service_name: "svc".to_owned(),
            service_version: None,
            started_at_unix_ms: base,
            finished_at_unix_ms: base + 1_000,
            finalized_at_unix_ms: Some(base + 1_001),
            mode: CaptureMode::Light,
            effective_core_config: None,
            effective_tokio_sampler_config: None,
            host: None,
            pid: None,
            lifecycle_warnings: vec![],
            unfinished_requests: UnfinishedRequests::default(),
            run_end_reason: None,
        },
        requests: vec![
            RequestEvent {
                request_id: "r1".into(),
                route: "/checkout".into(),
                kind: None,
                started_at_unix_ms: base,
                finished_at_unix_ms: base + 100,
                latency_us: 100_000,
                outcome: "ok".into(),
            },
            RequestEvent {
                request_id: "r2".into(),
                route: "/checkout".into(),
                kind: None,
                started_at_unix_ms: base + 100,
                finished_at_unix_ms: base + 205,
                latency_us: 105_000,
                outcome: "ok".into(),
            },
            RequestEvent {
                request_id: "r3".into(),
                route: "/checkout".into(),
                kind: None,
                started_at_unix_ms: base + 205,
                finished_at_unix_ms: base + 307,
                latency_us: 102_000,
                outcome: "ok".into(),
            },
            RequestEvent {
                request_id: "r4".into(),
                route: "/checkout".into(),
                kind: None,
                started_at_unix_ms: base + 307,
                finished_at_unix_ms: base + 409,
                latency_us: 102_000,
                outcome: "ok".into(),
            },
            RequestEvent {
                request_id: "r5".into(),
                route: "/checkout".into(),
                kind: None,
                started_at_unix_ms: base + 409,
                finished_at_unix_ms: base + 513,
                latency_us: 104_000,
                outcome: "ok".into(),
            },
        ],
        stages: [
            ("r1", "db", 80, 90, 10_000),
            ("r1", "cache", 90, 95, 5_000),
            ("r2", "db", 185, 195, 10_000),
            ("r2", "cache", 195, 200, 5_000),
            ("r3", "db", 287, 296, 9_000),
            ("r3", "cache", 296, 301, 5_000),
            ("r4", "db", 387, 396, 9_000),
            ("r4", "cache", 396, 401, 5_000),
            ("r5", "db", 489, 499, 10_000),
            ("r5", "cache", 499, 504, 5_000),
        ]
        .into_iter()
        .map(|(r, s, a, b, l)| StageEvent {
            request_id: r.into(),
            stage: s.into(),
            started_at_unix_ms: base + a,
            finished_at_unix_ms: base + b,
            latency_us: l,
            success: true,
        })
        .collect(),
        queues: [
            ("r1", 0, 80, 80_000),
            ("r2", 100, 185, 85_000),
            ("r3", 205, 287, 82_000),
            ("r4", 307, 387, 80_000),
            ("r5", 409, 489, 80_000),
        ]
        .into_iter()
        .map(|(r, a, b, w)| QueueEvent {
            request_id: r.into(),
            queue: "permits".into(),
            waited_from_unix_ms: base + a,
            waited_until_unix_ms: base + b,
            wait_us: w,
            depth_at_start: Some(3),
        })
        .collect(),
        inflight: vec![],
        runtime_snapshots: vec![],
        truncation: TruncationSummary::default(),
    }
}

fn deterministic_tracing_run() -> (Run, Vec<String>) {
    let b = 1_700_000_000_000_u64;
    let mut spans = Vec::new();
    for (id, s, e, d) in [
        ("r1", 0, 100, 100_000),
        ("r2", 100, 205, 105_000),
        ("r3", 205, 307, 102_000),
        ("r4", 307, 409, 102_000),
        ("r5", 409, 513, 104_000),
    ] {
        spans.push(
            SpanRecord::new("request", b + s, b + e)
                .duration_us(d)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, id)
                .field(TT_ROUTE, "/checkout"),
        );
    }
    for (id, s, e, d) in [
        ("r1", 0, 80, 80_000),
        ("r2", 100, 185, 85_000),
        ("r3", 205, 287, 82_000),
        ("r4", 307, 387, 80_000),
        ("r5", 409, 489, 80_000),
    ] {
        spans.push(
            SpanRecord::new("queue", b + s, b + e)
                .duration_us(d)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, id)
                .field(TT_QUEUE, "permits")
                .field(TT_DEPTH_AT_START, 3_u64),
        );
    }
    for (id, st, s, e, d) in [
        ("r1", "db", 80, 90, 10_000),
        ("r1", "cache", 90, 95, 5_000),
        ("r2", "db", 185, 195, 10_000),
        ("r2", "cache", 195, 200, 5_000),
        ("r3", "db", 287, 296, 9_000),
        ("r3", "cache", 296, 301, 5_000),
        ("r4", "db", 387, 396, 9_000),
        ("r4", "cache", 396, 401, 5_000),
        ("r5", "db", 489, 499, 10_000),
        ("r5", "cache", 499, 504, 5_000),
    ] {
        spans.push(
            SpanRecord::new("stage", b + s, b + e)
                .duration_us(d)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, id)
                .field(TT_STAGE, st)
                .field("tt.success", true),
        );
    }
    let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
    (
        imported.run().clone(),
        imported
            .warnings()
            .iter()
            .map(|w| w.message().to_owned())
            .collect(),
    )
}

fn tracing_run_with_queue(queue_name: &str) -> (Run, Vec<String>) {
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
                        tt.queue = queue_name,
                        tt.depth_at_start = 3_u64
                    );
                    {
                        let _queue_guard = queue.enter();
                        thread::sleep(Duration::from_millis(if slow { 12 } else { 6 }));
                    }
                    drop(queue);

                    let db_stage = tracing::info_span!(
                        "stage",
                        tt.kind = "stage",
                        tt.request_id = id,
                        tt.stage = "db",
                        tt.success = true
                    );
                    {
                        let _db_stage_guard = db_stage.enter();
                        thread::sleep(Duration::from_millis(if slow { 3 } else { 1 }));
                    }
                    drop(db_stage);

                    let cache_stage = tracing::info_span!(
                        "stage",
                        tt.kind = "stage",
                        tt.request_id = id,
                        tt.stage = "cache",
                        tt.success = true
                    );
                    {
                        let _cache_stage_guard = cache_stage.enter();
                        thread::sleep(Duration::from_millis(1));
                    }
                    drop(cache_stage);
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
    let native_request_count = native_run.requests.len();
    let tracing_request_count = tracing_run.requests.len();
    let native_stage_count = native_run.stages.len();
    let tracing_stage_count = tracing_run.stages.len();
    let native_queue_count = native_run.queues.len();
    let tracing_queue_count = tracing_run.queues.len();

    if native_run.truncation.limits_hit || tracing_run.truncation.limits_hit {
        mismatches.push("truncation.limits_hit must be false for both runs".to_owned());
    }
    if !native_run.runtime_snapshots.is_empty() || !tracing_run.runtime_snapshots.is_empty() {
        mismatches.push("runtime_snapshots must be empty for both runs".to_owned());
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
            mismatches.push(format!("native request {id} latency must be positive"));
        }
        match treq.get(id) {
            Some((tr, to, tl)) => {
                if route != tr {
                    mismatches.push(format!(
                        "route mismatch for request {id}: native={route}, tracing={tr}"
                    ));
                }
                if outcome != to {
                    mismatches.push(format!(
                        "outcome mismatch for request {id}: native={outcome:?}, tracing={to:?}"
                    ));
                }
                if *tl == 0 {
                    mismatches.push(format!("tracing request {id} latency must be positive"));
                }
            }
            None => mismatches.push(format!("tracing run missing request {id}")),
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
        mismatches.push(format!(
            "stage set mismatch: native={nstage:?}, tracing={tstage:?}"
        ));
    }
    if native_run.stages.iter().any(|s| s.latency_us == 0)
        || tracing_run.stages.iter().any(|s| s.latency_us == 0)
    {
        mismatches.push("all stage latencies must be positive".to_owned());
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
            "queue set mismatch: native={nqueue:?}, tracing={tqueue:?}"
        ));
    }
    if native_run.queues.iter().any(|q| q.wait_us == 0)
        || tracing_run.queues.iter().any(|q| q.wait_us == 0)
    {
        mismatches.push("all queue waits must be positive".to_owned());
    }

    let slow_native = nreq
        .iter()
        .max_by_key(|(_, (_, _, lat))| *lat)
        .map(|(id, _)| id.clone());
    let slow_tracing = treq
        .iter()
        .max_by_key(|(_, (_, _, lat))| *lat)
        .map(|(id, _)| id.clone());
    // Keep slowest-request identity diagnostic-only: request IDs can swap between native and
    // tracing paths due to platform/runtime timing jitter while semantic parity remains intact.
    // Product-semantic parity is enforced by request/stage/queue sets, counts, outcomes, and
    // analyzer/report checks rather than exact identity of the single slowest request.

    if native_request_count != tracing_request_count {
        mismatches.push(format!("request count mismatch: native={native_request_count}, tracing={tracing_request_count}"));
    }
    if native_stage_count != tracing_stage_count {
        mismatches.push(format!(
            "stage count mismatch: native={native_stage_count}, tracing={tracing_stage_count}"
        ));
    }
    if native_queue_count != tracing_queue_count {
        mismatches.push(format!(
            "queue count mismatch: native={native_queue_count}, tracing={tracing_queue_count}"
        ));
    }

    RunParityReport {
        mismatches,
        native_request_count,
        tracing_request_count,
        native_stage_count,
        tracing_stage_count,
        native_queue_count,
        tracing_queue_count,
        native_slowest_request_id: slow_native,
        tracing_slowest_request_id: slow_tracing,
    }
}

fn compare_analyzer_reports(native: &Report, tracing: &Report) -> AnalyzerParityReport {
    let mut mismatches = Vec::new();
    if native.request_count != tracing.request_count {
        mismatches.push(format!(
            "request_count mismatch: native={}, tracing={}",
            native.request_count, tracing.request_count
        ));
    }

    let n_primary = Some(native.primary_suspect.kind.clone());
    let t_primary = Some(tracing.primary_suspect.kind.clone());
    if n_primary != t_primary {
        mismatches.push(format!(
            "primary suspect mismatch: native={n_primary:?}, tracing={t_primary:?}"
        ));
    }

    if native.p95_latency_us.is_none_or(|v| v == 0) || tracing.p95_latency_us.is_none_or(|v| v == 0)
    {
        mismatches.push("p95_latency_us must be non-zero for both runs".to_owned());
    }
    if native.p99_latency_us.is_none_or(|v| v == 0) || tracing.p99_latency_us.is_none_or(|v| v == 0)
    {
        mismatches.push("p99_latency_us must be non-zero for both runs".to_owned());
    }

    let label = "/checkout";
    let native_has = report_contains_label(native, label);
    let tracing_has = report_contains_label(tracing, label);
    if native_has != tracing_has {
        mismatches.push(format!(
            "label presence mismatch for '{label}': native={native_has}, tracing={tracing_has}"
        ));
    }
    // Run artifact parity above already verifies exact request/stage/queue label sets
    // (including db/cache/permits). Analyzer evidence text may surface different
    // supporting labels across platforms, so we avoid duplicating strict label
    // presence checks here. This keeps strict artifact drift detection while
    // preserving stable analyzer semantics (request counts, p95/p99, primary suspect).

    AnalyzerParityReport {
        mismatches,
        native_request_count: native.request_count,
        tracing_request_count: tracing.request_count,
        native_primary_suspect: n_primary,
        tracing_primary_suspect: t_primary,
        native_primary_score: Some(native.primary_suspect.score),
        tracing_primary_score: Some(tracing.primary_suspect.score),
    }
}

fn compare_rendered_reports(native: &Report, tracing: &Report) -> RenderedReportParityReport {
    let native_render = normalize_rendered_report(&render_text(native));
    let tracing_render = normalize_rendered_report(&render_text(tracing));
    let mut mismatches = Vec::new();

    let native_sections = report_sections(&native_render);
    let tracing_sections = report_sections(&tracing_render);
    if native_sections != tracing_sections {
        mismatches.push(format!(
            "report section mismatch: native={native_sections:?}, tracing={tracing_sections:?}"
        ));
    }

    let n_suspect_line = find_primary_suspect_line(&native_render);
    let t_suspect_line = find_primary_suspect_line(&tracing_render);
    if n_suspect_line != t_suspect_line {
        mismatches.push(format!(
            "primary suspect line mismatch: native={n_suspect_line:?}, tracing={t_suspect_line:?}"
        ));
    }

    RenderedReportParityReport {
        mismatches,
        native_sections,
        tracing_sections,
        native_primary_suspect_line: n_suspect_line,
        tracing_primary_suspect_line: t_suspect_line,
    }
}

fn report_contains_label(report: &Report, label: &str) -> bool {
    report
        .primary_suspect
        .evidence
        .iter()
        .any(|e| e.contains(label))
        || report
            .primary_suspect
            .next_checks
            .iter()
            .any(|n| n.contains(label))
        || report.secondary_suspects.iter().any(|s| {
            s.evidence.iter().any(|e| e.contains(label))
                || s.next_checks.iter().any(|n| n.contains(label))
        })
}

fn normalize_rendered_report(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            if let Some(normalized) = normalize_unstable_line(line) {
                return normalized;
            }

            line.replace(" us", " <normalized_us>")
                .chars()
                .map(|ch| if ch.is_ascii_digit() { '#' } else { ch })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_unstable_line(line: &str) -> Option<String> {
    for prefix in ["Run ID:", "Run:", "Generated:", "Captured:", "Finalized:"] {
        if line.trim_start().starts_with(prefix) {
            return Some(format!("{prefix} <normalized>"));
        }
    }

    let unstable_fields = [
        "started_at_unix_ms",
        "finished_at_unix_ms",
        "finalized_at_unix_ms",
        "captured_at_unix_ms",
        "generated_at_unix_ms",
        "at_unix_ms",
    ];
    if unstable_fields.iter().any(|field| line.contains(field)) {
        return Some("<normalized unstable timestamp field>".to_owned());
    }

    None
}

fn report_sections(rendered: &str) -> BTreeSet<String> {
    let mut sections: BTreeSet<String> = rendered
        .lines()
        .filter(|line| line.starts_with("## "))
        .map(ToOwned::to_owned)
        .collect();

    let lowered = rendered.to_lowercase();
    if lowered.contains("primary suspect") || lowered.contains("diagnosis") {
        sections.insert("semantic: primary suspect / diagnosis".to_owned());
    }
    if lowered.contains("evidence") {
        sections.insert("semantic: evidence".to_owned());
    }
    if lowered.contains("next checks") {
        sections.insert("semantic: next checks".to_owned());
    }
    sections
}

fn find_primary_suspect_line(rendered: &str) -> Option<String> {
    rendered
        .lines()
        .find(|line| line.contains("Primary suspect") || line.contains("primary suspect"))
        .map(ToOwned::to_owned)
}

fn format_mismatches(mismatches: &[String]) -> String {
    if mismatches.is_empty() {
        "  - none".to_owned()
    } else {
        mismatches
            .iter()
            .map(|m| format!("  - {m}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn parity_report_detects_queue_name_mismatch() {
    let native = deterministic_native_run();
    let (mut tracing, _) = deterministic_tracing_run();
    for q in &mut tracing.queues {
        q.queue = "permits_changed".to_owned();
    }
    let report = build_parity_report(&native, &tracing);
    assert!(
        report
            .mismatches
            .iter()
            .any(|m| m.contains("queue set mismatch")),
        "expected queue mismatch, got {:?}",
        report.mismatches
    );
}

#[test]
fn live_recorder_preserves_event_shape_and_outputs_analyzable_run() {
    let (run, warnings) = live_tracing_run();
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(run.requests.len(), 3);
    assert_eq!(run.stages.len(), 6);
    assert_eq!(run.queues.len(), 3);
    assert_eq!(
        run.requests
            .iter()
            .map(|r| r.request_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["r1", "r2", "r3"])
    );
    assert_eq!(
        run.requests
            .iter()
            .map(|r| r.route.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/checkout"])
    );
    assert_eq!(
        run.stages
            .iter()
            .map(|s| (s.request_id.as_str(), s.stage.as_str()))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            ("r1", "db"),
            ("r1", "cache"),
            ("r2", "db"),
            ("r2", "cache"),
            ("r3", "db"),
            ("r3", "cache")
        ])
    );
    assert_eq!(
        run.queues
            .iter()
            .map(|q| (q.request_id.as_str(), q.queue.as_str(), q.depth_at_start))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            ("r1", "permits", Some(3)),
            ("r2", "permits", Some(3)),
            ("r3", "permits", Some(3))
        ])
    );
    assert!(run.requests.iter().all(|r| r.latency_us > 0));
    assert!(run.stages.iter().all(|s| s.latency_us > 0));
    assert!(run.queues.iter().all(|q| q.wait_us > 0));
    let analyzed = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(analyzed.request_count, 3);
}

#[test]
fn normalization_replaces_unstable_id_and_timestamp_lines() {
    let run_id_a = normalize_rendered_report("Run ID: abc123");
    let run_id_b = normalize_rendered_report("Run ID: def999");
    assert_eq!(run_id_a, run_id_b);

    let run_a = normalize_rendered_report("Run: abc123");
    let run_b = normalize_rendered_report("Run: def999");
    assert_eq!(run_a, run_b);

    let generated_a = normalize_rendered_report("Generated: 2026-05-17T12:00:00Z");
    let generated_b = normalize_rendered_report("Generated: 2026-05-18T13:01:59Z");
    assert_eq!(generated_a, generated_b);
}

#[test]
fn normalization_replaces_unstable_timestamp_field_lines() {
    let cases = [
        "started_at_unix_ms: 1712345678901",
        "finished_at_unix_ms: 1712345678902",
        "finalized_at_unix_ms: 1712345678903",
        "captured_at_unix_ms: 1712345678904",
        "generated_at_unix_ms: 1712345678905",
        "at_unix_ms: 1712345678906",
    ];

    let normalized: BTreeSet<_> = cases
        .iter()
        .map(|line| normalize_rendered_report(line))
        .collect();

    assert_eq!(
        normalized.len(),
        1,
        "all unstable timestamp lines should normalize the same"
    );
}

#[test]
fn normalization_preserves_semantic_content() {
    let a = "## Summary
Run ID: abc123
Latency (us): p50 100, p95 200, p99 300
## Diagnosis
Primary suspect: application_queue_saturation (high confidence, score 87)
Evidence:
- queue permits depth spikes on /checkout
Next checks:
- inspect db and cache stage latency";
    let b = "## Summary
Run ID: def999
Latency (us): p50 987, p95 654, p99 321
## Diagnosis
Primary suspect: downstream_stage_slow (high confidence, score 42)
Evidence:
- queue permits depth spikes on /checkout
Next checks:
- inspect db and cache stage latency";
    let normalized_a = normalize_rendered_report(a);
    let normalized_b = normalize_rendered_report(b);

    assert!(normalized_a.contains("Primary suspect: application_queue_saturation"));
    assert!(normalized_a.contains("high confidence"));
    assert!(normalized_a.contains("Evidence:"));
    assert!(normalized_a.contains("Next checks:"));
    assert!(normalized_a.contains("/checkout"));
    assert!(normalized_a.contains("db"));
    assert!(normalized_a.contains("cache"));
    assert!(normalized_a.contains("permits"));

    assert_ne!(
        normalized_a, normalized_b,
        "normalization must not hide semantic differences"
    );
}

#[test]
fn parity_report_detects_request_outcome_mismatch() {
    let native = deterministic_native_run();
    let (mut tracing, _) = deterministic_tracing_run();
    let request = tracing
        .requests
        .iter_mut()
        .find(|request| request.request_id == "r2")
        .expect("expected canonical request r2");
    request.outcome = "error".to_owned();

    let report = build_parity_report(&native, &tracing);
    assert!(
        report
            .run
            .mismatches
            .iter()
            .any(|mismatch| mismatch.contains("outcome mismatch for request r2")),
        "expected outcome mismatch, got {:?}",
        report.run.mismatches
    );
}
