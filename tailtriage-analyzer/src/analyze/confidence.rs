use tailtriage_core::Run;

use super::{
    Confidence, DiagnosisKind, EvidenceQuality, EvidenceQualityLevel, Suspect,
    AMBIGUITY_MIN_SCORE_THRESHOLD, AMBIGUITY_SCORE_GAP_THRESHOLD, LOW_COMPLETED_REQUEST_THRESHOLD,
};

pub(super) fn apply_evidence_aware_confidence_caps(
    suspects: &mut [Suspect],
    run: &Run,
    evidence_quality: &EvidenceQuality,
) {
    let runtime_snapshots_missing = run.runtime_snapshots.is_empty();
    let runtime_partial_key_fields = !runtime_snapshots_missing
        && (run
            .runtime_snapshots
            .iter()
            .all(|snapshot| snapshot.blocking_queue_depth.is_none())
            || run
                .runtime_snapshots
                .iter()
                .all(|snapshot| snapshot.local_queue_depth.is_none())
            || run
                .runtime_snapshots
                .iter()
                .all(|snapshot| snapshot.global_queue_depth.is_none()));
    let ambiguous_cluster = ambiguity_cluster_indices(suspects);
    for (i, suspect) in suspects.iter_mut().enumerate() {
        let mut cap = Confidence::High;
        let mut notes = Vec::new();
        let is_primary = i == 0;
        let is_insufficient = suspect.kind == DiagnosisKind::InsufficientEvidence;
        if !is_insufficient && evidence_quality.quality == EvidenceQualityLevel::Weak {
            cap = cap.min(Confidence::Medium);
        }
        if !is_insufficient && run.requests.is_empty() {
            cap = Confidence::Low;
            notes.push("Low completed-request count caps confidence.".to_string());
        } else if run.requests.len() < LOW_COMPLETED_REQUEST_THRESHOLD {
            if !is_insufficient {
                cap = cap.min(Confidence::Medium);
            }
            if is_primary {
                notes.push("Low completed-request count caps confidence.".to_string());
            }
        }
        if run.truncation.dropped_requests > 0 && !is_insufficient {
            cap = cap.min(Confidence::Medium);
            notes.push(
                "Capture truncation caps confidence because dropped evidence may affect ranking."
                    .to_string(),
            );
        }
        apply_family_evidence_caps(
            &suspect.kind,
            run,
            runtime_snapshots_missing,
            runtime_partial_key_fields,
            &mut cap,
            &mut notes,
        );
        let ambiguity_capped = ambiguous_cluster.contains(&i) && !is_insufficient;
        if ambiguity_capped {
            cap = cap.min(Confidence::Medium);
            notes.push(
                "Top suspects are close in score; confidence is capped by ambiguity.".to_string(),
            );
        }
        let original = suspect.confidence;
        suspect.confidence = original.min(cap);
        let cap_changed_bucket = suspect.confidence != original;
        if cap_changed_bucket || ambiguity_capped {
            notes.sort();
            notes.dedup();
            suspect.confidence_notes = notes;
        } else {
            suspect.confidence_notes.clear();
        }
    }
}

fn apply_family_evidence_caps(
    kind: &DiagnosisKind,
    run: &Run,
    runtime_snapshots_missing: bool,
    runtime_partial_key_fields: bool,
    cap: &mut Confidence,
    notes: &mut Vec<String>,
) {
    match kind {
        DiagnosisKind::ApplicationQueueSaturation => {
            if run.truncation.dropped_queues > 0 {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Capture truncation caps confidence because dropped evidence may affect ranking."
                        .to_string(),
                );
            }
            if run.queues.is_empty() {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Missing queue instrumentation limits queue-saturation confidence.".to_string(),
                );
            }
        }
        DiagnosisKind::DownstreamStageDominates => {
            if run.truncation.dropped_stages > 0 {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Capture truncation caps confidence because dropped evidence may affect ranking."
                        .to_string(),
                );
            }
            if run.stages.is_empty() {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Missing stage instrumentation limits downstream-stage confidence.".to_string(),
                );
            }
        }
        DiagnosisKind::BlockingPoolPressure | DiagnosisKind::ExecutorPressureSuspected => {
            if run.truncation.dropped_runtime_snapshots > 0 {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Capture truncation caps confidence because dropped evidence may affect ranking."
                        .to_string(),
                );
            }
            if runtime_snapshots_missing {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Missing runtime snapshots limit executor/blocking confidence.".to_string(),
                );
            } else if runtime_partial_key_fields {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Runtime snapshots are partial; missing runtime queue-depth fields limit executor/blocking confidence.".to_string(),
                );
            }
        }
        DiagnosisKind::InsufficientEvidence => {}
    }
}

fn ambiguity_cluster_indices(suspects: &[Suspect]) -> Vec<usize> {
    let mut ranked = suspects
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind != DiagnosisKind::InsufficientEvidence)
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(_, s)| std::cmp::Reverse(s.score));
    let Some((_, top)) = ranked.first() else {
        return Vec::new();
    };
    if top.score < AMBIGUITY_MIN_SCORE_THRESHOLD {
        return Vec::new();
    }
    let cluster = ranked
        .iter()
        .take_while(|(_, s)| {
            s.score >= AMBIGUITY_MIN_SCORE_THRESHOLD
                && top.score.abs_diff(s.score) <= AMBIGUITY_SCORE_GAP_THRESHOLD
        })
        .map(|(idx, _)| *idx)
        .collect::<Vec<_>>();
    if cluster.len() >= 2 {
        cluster
    } else {
        Vec::new()
    }
}
