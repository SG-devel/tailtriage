use tailtriage_core::Run;

use super::{
    candidate::SupportedCandidate,
    partial_evidence::{
        EvidenceBasis, PARTIAL_QUEUE_CONFIDENCE_NOTE, PARTIAL_STAGE_CONFIDENCE_NOTE,
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
        .map(|suspect| SupportedCandidate {
            suspect,
            basis: EvidenceBasis::Completed,
            executor_limitation: None,
            relevant_support: 20,
        })
        .collect::<Vec<_>>();
    apply_evidence_aware_confidence_caps_scored(&mut scored, run, evidence_quality, options);
    for (target, source) in suspects.iter_mut().zip(scored) {
        *target = source.suspect;
    }
}

pub(super) const fn maturity_cap(relevant_support: usize) -> Confidence {
    match relevant_support {
        0..=7 => Confidence::Low,
        8..=19 => Confidence::Medium,
        _ => Confidence::High,
    }
}

pub(super) fn apply_evidence_aware_confidence_caps_scored(
    suspects: &mut [SupportedCandidate],
    run: &Run,
    evidence_quality: &EvidenceQuality,
    options: &AnalyzeOptions,
) {
    let runtime_snapshots_missing = run.runtime_snapshots.is_empty();
    let runtime_partial_key_fields = !runtime_snapshots_missing
        && (run
            .runtime_snapshots
            .iter()
            .all(|s| s.blocking_queue_depth.is_none())
            || run
                .runtime_snapshots
                .iter()
                .all(|s| s.local_queue_depth.is_none())
            || run
                .runtime_snapshots
                .iter()
                .all(|s| s.global_queue_depth.is_none()));

    // Maturity and candidate-local limitations are materialized before ambiguity is computed.
    for scored in suspects.iter_mut() {
        let suspect = &mut scored.suspect;
        let is_insufficient = suspect.kind == DiagnosisKind::InsufficientEvidence;
        if is_insufficient {
            continue;
        }
        let original = suspect.confidence;
        let mut cap = maturity_cap(scored.relevant_support);
        let mut notes = Vec::new();
        if original > cap {
            notes.push(format!(
                "Family-relevant support is {} observation(s); provisional maturity caps confidence at {}.",
                scored.relevant_support,
                match cap { Confidence::Low => "low", Confidence::Medium => "medium", Confidence::High => "high" }
            ));
        }
        if evidence_quality.quality == EvidenceQualityLevel::Weak {
            cap = cap.min(Confidence::Medium);
        }
        if run.requests.is_empty() {
            cap = Confidence::Low;
            notes.push("Low completed-request count caps confidence.".to_string());
        } else if run.requests.len() < options.evidence.low_completed_request_threshold {
            cap = cap.min(Confidence::Medium);
            notes.push("Low completed-request count caps confidence.".to_string());
        }
        if run.truncation.dropped_requests > 0 {
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
        if let Some(limitation) = scored.executor_limitation {
            cap = cap.min(Confidence::Medium);
            notes.push(match limitation {
                crate::scoring::ExecutorConfidenceLimitation::MissingLocalDepth => "Missing local queue depth makes normalized runnable depth a lower bound; executor confidence cannot exceed medium.".to_string(),
                crate::scoring::ExecutorConfidenceLimitation::AmbiguousWorkers(status) => format!("Ambiguous worker-count evidence ({status:?}) requires legacy executor scoring; confidence cannot exceed medium."),
            });
        }
        suspect.confidence = original.min(cap);
        stable_dedup(&mut notes);
        if suspect.confidence != original
            || notes
                .iter()
                .any(|n| n == PARTIAL_QUEUE_CONFIDENCE_NOTE || n == PARTIAL_STAGE_CONFIDENCE_NOTE)
            || scored.executor_limitation.is_some()
        {
            suspect.confidence_notes = notes;
        }
    }

    let ambiguous_cluster = current_ambiguity_cluster_indices(suspects, options);
    for (index, scored) in suspects.iter_mut().enumerate() {
        if ambiguous_cluster.contains(&index)
            && scored.suspect.kind != DiagnosisKind::InsufficientEvidence
        {
            scored.suspect.confidence = scored.suspect.confidence.min(Confidence::Medium);
            scored.suspect.confidence_notes.push(
                "Top suspects are close in score; confidence is capped by ambiguity.".to_string(),
            );
            stable_dedup(&mut scored.suspect.confidence_notes);
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
        DiagnosisKind::ApplicationQueuePressure => {
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
        DiagnosisKind::DownstreamStageDominance => {
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
        DiagnosisKind::BlockingPoolPressure | DiagnosisKind::ExecutorPressure => {
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

/// Returns current raw-score ambiguity-cluster membership used by confidence capping.
pub(super) fn current_ambiguity_cluster_indices(
    suspects: &[SupportedCandidate],
    options: &AnalyzeOptions,
) -> Vec<usize> {
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
