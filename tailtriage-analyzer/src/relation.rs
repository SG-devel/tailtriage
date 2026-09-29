use tailtriage_core::{Run, StageRelation};

use crate::{
    candidate::SupportedCandidate, confidence::maturity_cap, partial_evidence::EvidenceBasis,
    scoring, Confidence, DiagnosisKind, RelatedEvidenceBasis, RelatedEvidenceGroup,
    RelatedEvidenceMeasurement, RelatedEvidenceMember,
};

pub(super) fn resolve_blocking_pool_group(
    candidates: &mut Vec<SupportedCandidate>,
    run: &Run,
    options: &crate::AnalyzeOptions,
) -> Vec<RelatedEvidenceGroup> {
    let Some(blocking_index) = candidates
        .iter()
        .position(|candidate| candidate.suspect.kind == DiagnosisKind::BlockingPoolPressure)
    else {
        return Vec::new();
    };
    let Some(downstream_index) = candidates
        .iter()
        .position(|candidate| candidate.suspect.kind == DiagnosisKind::DownstreamStageDominance)
    else {
        return Vec::new();
    };
    let Some(selected) = candidates[downstream_index].downstream_measurement.as_ref() else {
        return Vec::new();
    };
    let Some(p95_request) = crate::percentile(
        &run.requests
            .iter()
            .map(|request| request.latency_us)
            .collect::<Vec<_>>(),
        95,
        100,
    ) else {
        return Vec::new();
    };
    let related = scoring::blocking_related_stage_candidates(run, p95_request, options);
    // Mixed tagged/untagged evidence under one display name is conservative: only an exact
    // stage-owned representation establishes the selected family's typed relation.
    if !related.iter().any(|measurement| measurement == selected) {
        return Vec::new();
    }

    let representative_index =
        if representative_order(&candidates[blocking_index], &candidates[downstream_index]).is_ge()
        {
            blocking_index
        } else {
            downstream_index
        };
    let representative = candidates[representative_index].suspect.kind.clone();
    let blocking = candidates[blocking_index]
        .blocking_measurement
        .expect("eligible blocking candidate owns its measurement");
    let mut members = vec![RelatedEvidenceMember {
        diagnosis: DiagnosisKind::BlockingPoolPressure,
        stage: None,
        evidence_basis: RelatedEvidenceBasis::Completed,
        relevant_support: blocking.usable_sample_count,
        measurement: RelatedEvidenceMeasurement::BlockingPool {
            usable_snapshots: blocking.usable_sample_count,
            p95_depth: blocking.p95_queue_depth,
            peak_depth: blocking.peak_queue_depth,
            nonzero_share_permille: blocking.nonzero_share_permille,
        },
    }];
    members.extend(
        related
            .into_iter()
            .map(|measurement| RelatedEvidenceMember {
                diagnosis: DiagnosisKind::DownstreamStageDominance,
                stage: Some(measurement.stage),
                evidence_basis: match measurement.basis {
                    EvidenceBasis::Completed => RelatedEvidenceBasis::Completed,
                    EvidenceBasis::ObservedLowerBound => RelatedEvidenceBasis::ObservedLowerBound,
                },
                relevant_support: measurement.request_sample_count,
                measurement: RelatedEvidenceMeasurement::DownstreamStage {
                    tail_contribution_permille: measurement.tail_share_permille,
                    cumulative_contribution_permille: measurement.cumulative_share_permille,
                },
            }),
    );

    let remove_index = if representative_index == blocking_index {
        downstream_index
    } else {
        blocking_index
    };
    candidates.remove(remove_index);
    vec![RelatedEvidenceGroup {
        relation: StageRelation::BlockingPool,
        representative,
        members,
    }]
}

fn representative_order(a: &SupportedCandidate, b: &SupportedCandidate) -> std::cmp::Ordering {
    maturity_rank(maturity_cap(a.relevant_support))
        .cmp(&maturity_rank(maturity_cap(b.relevant_support)))
        .then_with(|| a.relevant_support.cmp(&b.relevant_support))
        .then_with(|| a.suspect.score.cmp(&b.suspect.score))
        // The existing diagnosis-kind tie order puts blocking before downstream.
        .then_with(|| kind_tie_rank(&b.suspect.kind).cmp(&kind_tie_rank(&a.suspect.kind)))
}

const fn maturity_rank(confidence: Confidence) -> u8 {
    match confidence {
        Confidence::Low => 1,
        Confidence::Medium => 2,
        Confidence::High => 3,
    }
}

const fn kind_tie_rank(kind: &DiagnosisKind) -> u8 {
    match kind {
        DiagnosisKind::ApplicationQueuePressure => 0,
        DiagnosisKind::BlockingPoolPressure => 1,
        DiagnosisKind::ExecutorPressure => 2,
        DiagnosisKind::DownstreamStageDominance => 3,
        DiagnosisKind::InsufficientEvidence => 255,
    }
}
