use tailtriage_core::Run;

use super::{
    partial_evidence::{
        EvidenceBasis, ScoredSuspect, PARTIAL_QUEUE_CONFIDENCE_NOTE, PARTIAL_STAGE_CONFIDENCE_NOTE,
    },
    AnalyzeOptions, Confidence, DiagnosisKind, EvidenceQuality, EvidenceQualityLevel,
};

#[allow(dead_code)]
pub(super) fn apply_evidence_aware_confidence_caps(
    suspects: &mut [crate::Suspect],
    run: &Run,
    evidence_quality: &EvidenceQuality,
    options: &AnalyzeOptions,
) {
    let mut scored = suspects
        .iter()
        .cloned()
        .map(|suspect| ScoredSuspect {
            suspect,
            basis: EvidenceBasis::Completed,
        })
        .collect::<Vec<_>>();
    apply_evidence_aware_confidence_caps_scored(&mut scored, run, evidence_quality, options);
    for (target, source) in suspects.iter_mut().zip(scored) {
        *target = source.suspect;
    }
}

pub(super) fn apply_evidence_aware_confidence_caps_scored(
    suspects: &mut [ScoredSuspect],
    run: &Run,
    evidence_quality: &EvidenceQuality,
    options: &AnalyzeOptions,
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
    let ambiguous_cluster = ambiguity_cluster_indices(suspects, options);
    for (i, scored) in suspects.iter_mut().enumerate() {
        let suspect = &mut scored.suspect;
        let mut cap = Confidence::High;
        let mut notes = Vec::new();
        let is_insufficient = suspect.kind == DiagnosisKind::InsufficientEvidence;
        if !is_insufficient && evidence_quality.quality == EvidenceQualityLevel::Weak {
            cap = cap.min(Confidence::Medium);
        }
        if !is_insufficient && run.requests.is_empty() {
            cap = Confidence::Low;
            notes.push("Low completed-request count caps confidence.".to_string());
        } else if run.requests.len() < options.evidence.low_completed_request_threshold
            && !is_insufficient
        {
            cap = cap.min(Confidence::Medium);
            notes.push("Low completed-request count caps confidence.".to_string());
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
            scored.basis,
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
        let has_material_partial_note = notes.iter().any(|note| {
            note == PARTIAL_QUEUE_CONFIDENCE_NOTE || note == PARTIAL_STAGE_CONFIDENCE_NOTE
        });
        stable_dedup(&mut notes);
        if cap_changed_bucket || ambiguity_capped || has_material_partial_note {
            suspect.confidence_notes = notes;
        } else {
            suspect.confidence_notes.clear();
        }
    }
}

fn apply_family_evidence_caps(
    kind: &DiagnosisKind,
    basis: EvidenceBasis,
    run: &Run,
    runtime_snapshots_missing: bool,
    runtime_partial_key_fields: bool,
    cap: &mut Confidence,
    notes: &mut Vec<String>,
) {
    match kind {
        DiagnosisKind::ApplicationQueueSaturation => {
            if basis == EvidenceBasis::ObservedLowerBound {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(PARTIAL_QUEUE_CONFIDENCE_NOTE.to_string());
            }
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
            if basis == EvidenceBasis::ObservedLowerBound {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(PARTIAL_STAGE_CONFIDENCE_NOTE.to_string());
            }
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

fn ambiguity_cluster_indices(suspects: &[ScoredSuspect], options: &AnalyzeOptions) -> Vec<usize> {
    let mut ranked = suspects
        .iter()
        .enumerate()
        .filter(|(_, s)| s.suspect.kind != DiagnosisKind::InsufficientEvidence)
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(_, s)| std::cmp::Reverse(s.suspect.score));
    let Some((_, top)) = ranked.first() else {
        return Vec::new();
    };
    if top.suspect.score < options.confidence.ambiguity_min_score {
        return Vec::new();
    }
    let cluster = ranked
        .iter()
        .take_while(|(_, s)| {
            s.suspect.score >= options.confidence.ambiguity_min_score
                && top.suspect.score.abs_diff(s.suspect.score)
                    <= options.confidence.ambiguity_score_gap
        })
        .map(|(idx, _)| *idx)
        .collect::<Vec<_>>();
    if cluster.len() >= 2 {
        cluster
    } else {
        Vec::new()
    }
}

fn stable_dedup(values: &mut Vec<String>) {
    let mut deduped = Vec::with_capacity(values.len());
    for value in values.drain(..) {
        if !deduped.iter().any(|existing| existing == &value) {
            deduped.push(value);
        }
    }
    *values = deduped;
}
