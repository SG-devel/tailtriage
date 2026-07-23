use tailtriage_core::Run;

use crate::Suspect;

pub(super) const PARTIAL_WARNING: &str =
    "Partial queue/stage observations are lower bounds; completed-duration percentiles exclude them.";
pub(super) const PARTIAL_QUEUE_CONFIDENCE_NOTE: &str =
    "Partial queue evidence materially contributes to this suspect; confidence cannot exceed medium because partial durations are lower bounds.";
pub(super) const PARTIAL_STAGE_CONFIDENCE_NOTE: &str =
    "Partial stage evidence materially contributes to this suspect; confidence cannot exceed medium because partial durations are lower bounds.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct CompletionCounts {
    pub(super) completed: usize,
    pub(super) partial: usize,
}

impl CompletionCounts {
    pub(super) fn total(self) -> usize {
        self.completed + self.partial
    }
    pub(super) fn has_partial(self) -> bool {
        self.partial > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct PartialEvidenceProfile {
    pub(super) queues: CompletionCounts,
    pub(super) stages: CompletionCounts,
}

impl PartialEvidenceProfile {
    pub(super) fn from_run(run: &Run) -> Self {
        let queues = CompletionCounts {
            completed: run.queues.iter().filter(|q| q.completed).count(),
            partial: run.queues.iter().filter(|q| !q.completed).count(),
        };
        let stages = CompletionCounts {
            completed: run.stages.iter().filter(|s| s.completed).count(),
            partial: run.stages.iter().filter(|s| !s.completed).count(),
        };
        Self { queues, stages }
    }
    pub(super) fn has_partial(self) -> bool {
        self.queues.has_partial() || self.stages.has_partial()
    }
    pub(super) fn limitation(self) -> Option<String> {
        self.has_partial().then(|| format!(
            "Partial evidence captured: queues {} completed/{} partial; stages {} completed/{} partial. Partial durations are observed lower bounds.",
            self.queues.completed, self.queues.partial, self.stages.completed, self.stages.partial
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EvidenceBasis {
    Completed,
    ObservedLowerBound,
}

#[derive(Debug, Clone)]
pub(super) struct ScoredSuspect {
    pub(super) suspect: Suspect,
    pub(super) basis: EvidenceBasis,
}
