use crate::{
    CaptureLimits, InFlightSnapshot, QueueEvent, RequestEvent, Run, RuntimeSnapshot, StageEvent,
};

pub(crate) fn push_request_bounded(
    run: &mut Run,
    limits: CaptureLimits,
    event: RequestEvent,
) -> bool {
    if run.requests.len() < limits.max_requests {
        run.requests.push(event);
        return false;
    }

    run.truncation.limits_hit = true;
    run.truncation.dropped_requests = run.truncation.dropped_requests.saturating_add(1);
    true
}

pub(crate) fn push_stage_bounded(run: &mut Run, limits: CaptureLimits, event: StageEvent) -> bool {
    if run.stages.len() < limits.max_stages {
        run.stages.push(event);
        return false;
    }

    run.truncation.limits_hit = true;
    run.truncation.dropped_stages = run.truncation.dropped_stages.saturating_add(1);
    true
}

pub(crate) fn push_queue_bounded(run: &mut Run, limits: CaptureLimits, event: QueueEvent) -> bool {
    if run.queues.len() < limits.max_queues {
        run.queues.push(event);
        return false;
    }

    run.truncation.limits_hit = true;
    run.truncation.dropped_queues = run.truncation.dropped_queues.saturating_add(1);
    true
}

pub(crate) fn push_inflight_snapshot_bounded(
    run: &mut Run,
    limits: CaptureLimits,
    snapshot: InFlightSnapshot,
) -> bool {
    if run.inflight.len() < limits.max_inflight_snapshots {
        run.inflight.push(snapshot);
        return false;
    }

    run.truncation.limits_hit = true;
    run.truncation.dropped_inflight_snapshots =
        run.truncation.dropped_inflight_snapshots.saturating_add(1);
    true
}

pub(crate) fn push_runtime_snapshot_bounded(
    run: &mut Run,
    limits: CaptureLimits,
    snapshot: RuntimeSnapshot,
) -> bool {
    if run.runtime_snapshots.len() < limits.max_runtime_snapshots {
        run.runtime_snapshots.push(snapshot);
        return false;
    }

    run.truncation.limits_hit = true;
    run.truncation.dropped_runtime_snapshots =
        run.truncation.dropped_runtime_snapshots.saturating_add(1);
    true
}
