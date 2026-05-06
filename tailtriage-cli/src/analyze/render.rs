use super::{Confidence, EvidenceQualityLevel, Report, TemporalSegment};

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
        Some(trend) => {
            lines.push(format!(
                "Inflight trend: gauge '{}', samples {}, peak {}, p95 {}, net growth {:+}",
                trend.gauge,
                trend.sample_count,
                trend.peak_count,
                trend.p95_count,
                trend.growth_delta,
            ));
        }
        None => lines.push("Inflight trend: none".to_string()),
    }

    lines.push(format!(
        "Primary suspect: {} ({} confidence, score {})",
        report.primary_suspect.kind.as_str(),
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
            .map_or_else(String::new, |l| format!(" ({l})"))
    ));

    if !report.warnings.is_empty() {
        lines.push("Warnings:".to_string());
        for warning in &report.warnings {
            lines.push(format!("- {warning}"));
        }
    }
    if !report.primary_suspect.evidence.is_empty() {
        lines.push("Evidence:".to_string());
        for evidence in &report.primary_suspect.evidence {
            lines.push(format!("- {evidence}"));
        }
    }
    if !report.primary_suspect.next_checks.is_empty() {
        lines.push("Next checks:".to_string());
        for next_check in &report.primary_suspect.next_checks {
            lines.push(format!("- {next_check}"));
        }
    }
    if !report.secondary_suspects.is_empty() {
        lines.push("Secondary suspects:".to_string());
        for suspect in &report.secondary_suspects {
            lines.push(format!(
                "- {} ({} confidence, score {})",
                suspect.kind.as_str(),
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
                route.route,
                route.request_count,
                fmt_opt_u64(route.p95_latency_us),
                route.primary_suspect.kind.as_str(),
                fmt_confidence(route.primary_suspect.confidence)
            ));
        }
    }
    append_temporal_segment_text(&mut lines, &report.temporal_segments);
    lines.join("\n")
}

fn append_temporal_segment_text(lines: &mut Vec<String>, segments: &[TemporalSegment]) {
    if segments.is_empty() {
        return;
    }
    lines.push("Temporal segments:".to_string());
    for seg in segments {
        lines.push(format!(
            "- {}: requests {}, p95 {}us, suspect {} ({} confidence)",
            seg.name,
            seg.request_count,
            fmt_opt_u64(seg.p95_latency_us),
            seg.primary_suspect.kind.as_str(),
            fmt_confidence(seg.primary_suspect.confidence)
        ));
    }
}
