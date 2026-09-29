use crate::{
    partial_evidence::EvidenceBasis, scoring::ExecutorConfidenceLimitation, DiagnosisKind, Suspect,
};

/// A magnitude result together with the support context needed by later phases.
#[derive(Debug, Clone)]
pub(super) struct SupportedCandidate {
    pub(super) suspect: Suspect,
    pub(super) basis: EvidenceBasis,
    pub(super) executor_limitation: Option<ExecutorConfidenceLimitation>,
    pub(super) relevant_support: usize,
}

/// Owns the single selected representation for each diagnosis family.
///
/// The family slots make it impossible for completed and lower-bound forms of
/// one family to independently enter cross-family relation and ranking work.
#[derive(Debug, Default)]
pub(super) struct FamilyCandidates {
    queue: Option<SupportedCandidate>,
    blocking: Option<SupportedCandidate>,
    executor: Option<SupportedCandidate>,
    downstream: Option<SupportedCandidate>,
}

impl FamilyCandidates {
    pub(super) fn set_queue(&mut self, candidate: Option<SupportedCandidate>) {
        self.queue = candidate;
    }

    pub(super) fn set_blocking(&mut self, candidate: Option<SupportedCandidate>) {
        self.blocking = candidate;
    }

    pub(super) fn set_executor(&mut self, candidate: Option<SupportedCandidate>) {
        self.executor = candidate;
    }

    pub(super) fn set_downstream(&mut self, candidate: Option<SupportedCandidate>) {
        self.downstream = candidate;
    }

    pub(super) fn into_cross_family_candidates(self) -> Vec<SupportedCandidate> {
        [self.queue, self.blocking, self.executor, self.downstream]
            .into_iter()
            .flatten()
            .collect()
    }
}

pub(super) fn completed_candidate(suspect: Suspect, relevant_support: usize) -> SupportedCandidate {
    SupportedCandidate {
        suspect,
        basis: EvidenceBasis::Completed,
        executor_limitation: None,
        relevant_support,
    }
}

pub(super) fn fallback_candidate(suspect: Suspect) -> SupportedCandidate {
    debug_assert_eq!(suspect.kind, DiagnosisKind::InsufficientEvidence);
    completed_candidate(suspect, 0)
}
