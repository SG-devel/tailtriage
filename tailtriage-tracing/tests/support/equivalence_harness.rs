use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::Duration;

use futures_executor::block_on;
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions, Report};
use tailtriage_core::{MemorySink, Run, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

pub fn assert_native_and_tracing_full_parity() {
    let native_run = native_run();
    let (tracing_run, warnings) = tracing_run();

    let report = build_parity_report(&native_run, &tracing_run);
    assert!(
        warnings.is_empty(),
        "unexpected tracing warnings: {warnings:?}"
    );
    assert_parity_report(&report);
}

pub fn build_parity_report(native_run: &Run, tracing_run: &Run) -> ParityReport {
    let native_analysis = analyze_run(native_run, AnalyzeOptions::default());
    let tracing_analysis = analyze_run(tracing_run, AnalyzeOptions::default());

    let run_report = compare_runs(native_run, tracing_run);
    let analyzer_report = compare_analyzer_reports(&native_analysis, &tracing_analysis);
    let rendered_report = compare_rendered_reports(&native_analysis, &tracing_analysis);

    ParityReport {
        run_report,
        analyzer_report,
        rendered_report,
        mismatches: Vec::new(),
    }
    .with_aggregate_mismatches()
}

pub fn assert_parity_report(report: &ParityReport) {
    assert!(
        report.mismatches.is_empty(),
        "native-vs-tracing parity mismatch\n{}",
        report.render_failure_summary()
    );
}

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
    pub native_primary_suspect_kind: Option<String>,
    pub tracing_primary_suspect_kind: Option<String>,
    pub native_primary_score: Option<u8>,
    pub tracing_primary_score: Option<u8>,
    pub native_p95_latency_us: Option<u64>,
    pub tracing_p95_latency_us: Option<u64>,
    pub native_p99_latency_us: Option<u64>,
    pub tracing_p99_latency_us: Option<u64>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct RenderedReportParityReport {
    pub mismatches: Vec<String>,
    pub native_section_count: usize,
    pub tracing_section_count: usize,
    pub native_primary_suspect_kind_text: Option<String>,
    pub tracing_primary_suspect_kind_text: Option<String>,
}

#[derive(Debug)]
pub struct ParityReport {
    pub run_report: RunParityReport,
    pub analyzer_report: AnalyzerParityReport,
    pub rendered_report: RenderedReportParityReport,
    pub mismatches: Vec<String>,
}

impl ParityReport {
    fn with_aggregate_mismatches(mut self) -> Self {
        self.mismatches.extend(
            self.run_report
                .mismatches
                .iter()
                .map(|m| format!("run: {m}")),
        );
        self.mismatches.extend(
            self.analyzer_report
                .mismatches
                .iter()
                .map(|m| format!("analyzer: {m}")),
        );
        self.mismatches.extend(
            self.rendered_report
                .mismatches
                .iter()
                .map(|m| format!("rendered-report: {m}")),
        );
        self
    }

    fn render_failure_summary(&self) -> String {
        format!(
            "Run mismatches:\n- {}\nAnalyzer mismatches:\n- {}\nRendered report mismatches:\n- {}",
            self.run_report.mismatches.join("\n- "),
            self.analyzer_report.mismatches.join("\n- "),
            self.rendered_report.mismatches.join("\n- ")
        )
    }
}

fn native_run() -> Run {
    let sink = MemorySink::default();
    let tt = Tailtriage::builder("svc")
        .sink(sink.clone())
        .build()
        .unwrap();
    emit_scenario_with_tailtriage(&tt, "permits");
    tt.shutdown().unwrap();
    sink.last_run().unwrap()
}

fn tracing_run() -> (Run, Vec<String>) {
    tracing_run_with_queue_name("permits")
}

pub fn tracing_run_with_queue_name(queue_name: &str) -> (Run, Vec<String>) {
    let recorder = TracingRecorder::builder("svc").build();
    let subscriber = tracing_subscriber::registry().with(recorder.layer());
    tracing::subscriber::with_default(subscriber, || {
        block_on(async {
            emit_scenario_with_tracing(queue_name);
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

fn emit_scenario_with_tailtriage(tt: &Tailtriage, queue_name: &str) {
    for (id, slow) in [("r1", false), ("r2", true), ("r3", false)] {
        let started = tt.begin_request_with(
            "/checkout",
            tailtriage_core::RequestOptions::new().request_id(id),
        );
        block_on(
            started
                .handle
                .queue(queue_name)
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
}

fn emit_scenario_with_tracing(queue_name: &str) {
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
                    tt.queue = queue_name,
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
}

fn compare_runs(native_run: &Run, tracing_run: &Run) -> RunParityReport {
    let mut mismatches = Vec::new();
    if !native_run.runtime_snapshots.is_empty() || !tracing_run.runtime_snapshots.is_empty() {
        mismatches.push("runtime snapshots should be empty in parity scenario".to_owned());
    }
    if native_run.truncation.limits_hit || tracing_run.truncation.limits_hit {
        mismatches.push("truncation.limits_hit must be false for both runs".to_owned());
    }
    let nreq: BTreeMap<_, _> = native_run
        .requests
        .iter()
        .map(|r| (r.request_id.clone(), (&r.route, &r.outcome, r.latency_us)))
        .collect();
    let treq: BTreeMap<_, _> = tracing_run
        .requests
        .iter()
        .map(|r| (r.request_id.clone(), (&r.route, &r.outcome, r.latency_us)))
        .collect();
    if nreq.keys().collect::<BTreeSet<_>>() != treq.keys().collect::<BTreeSet<_>>() {
        mismatches.push("request id set mismatch".to_owned());
    }
    for (id, (nr, no, nl)) in &nreq {
        match treq.get(id) {
            Some((tr, to, tl)) => {
                if nr != tr {
                    mismatches.push(format!(
                        "route mismatch for {id}: native={nr}, tracing={tr}"
                    ));
                }
                if no != to {
                    mismatches.push(format!("outcome mismatch for {id}"));
                }
                if *nl == 0 || *tl == 0 {
                    mismatches.push(format!("request latency must be positive for {id}"));
                }
            }
            None => mismatches.push(format!("missing tracing request {id}")),
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
            "queue set mismatch: native={nqueue:?}, tracing={tqueue:?}"
        ));
    }
    if native_run.stages.iter().any(|s| s.latency_us == 0)
        || tracing_run.stages.iter().any(|s| s.latency_us == 0)
    {
        mismatches.push("stage durations must be positive".to_owned());
    }
    if native_run.queues.iter().any(|q| q.wait_us == 0)
        || tracing_run.queues.iter().any(|q| q.wait_us == 0)
    {
        mismatches.push("queue durations must be positive".to_owned());
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
        mismatches.push(format!(
            "slowest request mismatch: native={slow_native:?}, tracing={slow_tracing:?}"
        ));
    }

    RunParityReport {
        mismatches,
        native_request_count: native_run.requests.len(),
        tracing_request_count: tracing_run.requests.len(),
        native_stage_count: native_run.stages.len(),
        tracing_stage_count: tracing_run.stages.len(),
        native_queue_count: native_run.queues.len(),
        tracing_queue_count: tracing_run.queues.len(),
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
    if native.primary_suspect.kind != tracing.primary_suspect.kind {
        mismatches.push(format!(
            "primary suspect kind mismatch: native={}, tracing={}",
            native.primary_suspect.kind.as_str(),
            tracing.primary_suspect.kind.as_str()
        ));
    }
    if native.primary_suspect.score != tracing.primary_suspect.score {
        mismatches.push(format!(
            "primary suspect score differs: native={}, tracing={}",
            native.primary_suspect.score, tracing.primary_suspect.score
        ));
    }
    for label in ["/checkout", "db", "cache", "permits"] {
        let nn = report_contains_label(native, label);
        let tn = report_contains_label(tracing, label);
        if nn != tn {
            mismatches.push(format!(
                "label presence mismatch for '{label}': native={nn}, tracing={tn}"
            ));
        }
    }
    if native.p95_latency_us.unwrap_or(0) == 0 || tracing.p95_latency_us.unwrap_or(0) == 0 {
        mismatches.push("p95 latency must be present and non-zero".to_owned());
    }
    if native.p99_latency_us.unwrap_or(0) == 0 || tracing.p99_latency_us.unwrap_or(0) == 0 {
        mismatches.push("p99 latency must be present and non-zero".to_owned());
    }
    AnalyzerParityReport {
        mismatches,
        native_request_count: native.request_count,
        tracing_request_count: tracing.request_count,
        native_primary_suspect_kind: Some(native.primary_suspect.kind.as_str().to_owned()),
        tracing_primary_suspect_kind: Some(tracing.primary_suspect.kind.as_str().to_owned()),
        native_primary_score: Some(native.primary_suspect.score),
        tracing_primary_score: Some(tracing.primary_suspect.score),
        native_p95_latency_us: native.p95_latency_us,
        tracing_p95_latency_us: tracing.p95_latency_us,
        native_p99_latency_us: native.p99_latency_us,
        tracing_p99_latency_us: tracing.p99_latency_us,
    }
}
fn report_contains_label(report: &Report, label: &str) -> bool {
    let text = render_text(report);
    let json = serde_json::to_string(report).unwrap_or_default();
    text.contains(label) || json.contains(label)
}

fn compare_rendered_reports(native: &Report, tracing: &Report) -> RenderedReportParityReport {
    let mut mismatches = Vec::new();
    let native_text = normalize_rendered_report(&render_text(native));
    let tracing_text = normalize_rendered_report(&render_text(tracing));
    for section in ["Primary suspect:", "Evidence:", "Next checks:"] {
        if !(native_text.contains(section) && tracing_text.contains(section)) {
            mismatches.push(format!(
                "missing key section '{section}' in one or both reports"
            ));
        }
    }
    if extract_primary_suspect_kind_text(&native_text)
        != extract_primary_suspect_kind_text(&tracing_text)
    {
        mismatches.push(format!(
            "primary suspect text mismatch: native={:?}, tracing={:?}",
            extract_primary_suspect_kind_text(&native_text),
            extract_primary_suspect_kind_text(&tracing_text)
        ));
    }
    RenderedReportParityReport {
        mismatches,
        native_section_count: native_text.lines().filter(|l| l.ends_with(':')).count(),
        tracing_section_count: tracing_text.lines().filter(|l| l.ends_with(':')).count(),
        native_primary_suspect_kind_text: extract_primary_suspect_kind_text(&native_text),
        tracing_primary_suspect_kind_text: extract_primary_suspect_kind_text(&tracing_text),
    }
}

pub fn normalize_rendered_report(input: &str) -> String {
    input
        .lines()
        .map(normalize_line)
        .collect::<Vec<_>>()
        .join("\n")
}
fn normalize_line(line: &str) -> String {
    let mut s = line.to_owned();
    for key in ["Run:", "Generated:"] {
        if s.contains(key) {
            return format!("{key} <normalized>");
        }
    }
    if s.contains("µs") || s.contains("ms") {
        s = strip_digits(&s);
    }
    s
}
fn strip_digits(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect()
}
fn extract_primary_suspect_kind_text(text: &str) -> Option<String> {
    text.lines()
        .find(|l| l.to_ascii_lowercase().contains("primary suspect"))
        .map(strip_digits)
}
