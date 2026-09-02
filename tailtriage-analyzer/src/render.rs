use super::{Confidence, DiagnosisKind, EvidenceQualityLevel, Report, TemporalSegment};

use tailtriage_core::__internal::escape_control_chars;

fn fmt_opt_u64(value: Option<u64>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "n/a".to_string(),
    }
}

fn fmt_percent_permille(value: Option<u64>) -> String {
    match value {
        Some(value) => format!("{}.{:01}%", value / 10, value % 10),
        None => "n/a".to_string(),
    }
}

fn diagnosis_display_name(kind: &DiagnosisKind) -> &'static str {
    match kind {
        DiagnosisKind::ApplicationQueuePressure => "application queue pressure",
        DiagnosisKind::BlockingPoolPressure => "blocking pool pressure",
        DiagnosisKind::ExecutorPressure => "executor pressure",
        DiagnosisKind::DownstreamStageDominance => "downstream stage dominance",
        DiagnosisKind::InsufficientEvidence => "insufficient evidence",
    }
}

fn fmt_confidence(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Low => "low",
        Confidence::Medium => "medium",
        Confidence::High => "high",
    }
}

/// Renders a compact text triage summary from a [`Report`].
///
/// The rendered output is guidance for follow-up checks, not proof of root cause.
#[must_use]
pub fn render_text(report: &Report) -> String {
    let mut lines = vec![
        "tailtriage diagnosis".to_string(),
        format!("Requests analyzed: {}", report.request_count),
        format!(
            "Latency (us): p50 {}, p95 {}, p99 {}",
            fmt_opt_u64(report.p50_latency_us),
            fmt_opt_u64(report.p95_latency_us),
            fmt_opt_u64(report.p99_latency_us),
        ),
        format!(
            "Request time at p95: queue {}, non-queue service {}",
            fmt_percent_permille(report.p95_queue_share_permille),
            fmt_percent_permille(report.p95_service_share_permille),
        ),
    ];

    match &report.inflight_trend {
        Some(trend) => lines.push(render_inflight_trend(trend)),
        None => lines.push("Inflight trend: none".to_string()),
    }

    lines.push(format!(
        "Primary suspect: {} ({} confidence, score {})",
        diagnosis_display_name(&report.primary_suspect.kind),
        fmt_confidence(report.primary_suspect.confidence),
        report.primary_suspect.score,
    ));
    lines.push(format!(
        "Evidence quality: {}{}",
        match report.evidence_quality.quality {
            EvidenceQualityLevel::Strong => "strong",
            EvidenceQualityLevel::Partial => "partial",
            EvidenceQualityLevel::Weak => "weak",
        },
        report
            .evidence_quality
            .limitations
            .first()
            .map_or_else(String::new, |l| format!(" ({})", escape_control_chars(l)))
    ));

    if !report.warnings.is_empty() {
        lines.push("Warnings:".to_string());
        for warning in &report.warnings {
            lines.push(format!("- {}", escape_control_chars(warning)));
        }
    }
    if !report.primary_suspect.evidence.is_empty() {
        lines.push("Evidence:".to_string());
        for evidence in &report.primary_suspect.evidence {
            lines.push(format!("- {}", escape_control_chars(evidence)));
        }
    }
    if !report.primary_suspect.next_checks.is_empty() {
        lines.push("Next checks:".to_string());
        for next_check in &report.primary_suspect.next_checks {
            lines.push(format!("- {}", escape_control_chars(next_check)));
        }
    }
    if !report.secondary_suspects.is_empty() {
        lines.push("Secondary suspects:".to_string());
        for suspect in &report.secondary_suspects {
            lines.push(format!(
                "- {} ({} confidence, score {})",
                diagnosis_display_name(&suspect.kind),
                fmt_confidence(suspect.confidence),
                suspect.score
            ));
        }
    }
    if !report.route_breakdowns.is_empty() {
        lines.push("Route breakdowns:".to_string());
        for route in &report.route_breakdowns {
            lines.push(format!(
                "- {}: requests {}, p95 {}us, suspect {} ({} confidence)",
                escape_control_chars(&route.route),
                route.request_count,
                fmt_opt_u64(route.p95_latency_us),
                diagnosis_display_name(&route.primary_suspect.kind),
                fmt_confidence(route.primary_suspect.confidence)
            ));
        }
    }
    if let Some(config) = &report.analyzer_config {
        lines.push("Analyzer config:".to_string());
        lines.push(format!("- schema_version: {}", config.schema_version));
        for item in &config.non_default_options {
            lines.push(format!(
                "- {}={}",
                escape_control_chars(&item.path),
                escape_control_chars(&item.value)
            ));
        }
    }
    append_temporal_segment_text(&mut lines, &report.temporal_segments);
    lines.join("\n")
}

fn render_inflight_trend(trend: &crate::InflightTrend) -> String {
    let direction = match trend.growth_delta {
        None => "direction unknown".to_string(),
        Some(delta) => format!("net growth {delta:+}"),
    };
    let rate = trend.growth_per_sec_milli.map_or_else(
        || "precise rate unavailable".to_string(),
        |rate| format!("run-relative rate {rate} milli-counts/sec"),
    );
    format!(
        "Inflight latest activity episode: gauge '{}', samples {}, peak {}, p95 {}, {}, {}",
        escape_control_chars(&trend.gauge),
        trend.sample_count,
        trend.peak_count,
        trend.p95_count,
        direction,
        rate,
    )
}

fn append_temporal_segment_text(lines: &mut Vec<String>, segments: &[TemporalSegment]) {
    if segments.is_empty() {
        return;
    }
    lines.push("Temporal segments:".to_string());
    for seg in segments {
        lines.push(format!(
            "- {}: requests {}, p95 {}us, suspect {} ({} confidence)",
            escape_control_chars(&seg.name),
            seg.request_count,
            fmt_opt_u64(seg.p95_latency_us),
            diagnosis_display_name(&seg.primary_suspect.kind),
            fmt_confidence(seg.primary_suspect.confidence)
        ));
    }
}
