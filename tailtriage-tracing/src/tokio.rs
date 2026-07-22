use tailtriage_core::Run;

use crate::{ImportWarning, ImportedRun};

pub(crate) fn with_manual_sampler_warning(
    mut merged: ImportedRun,
    sampler_disabled: bool,
) -> ImportedRun {
    if !sampler_disabled {
        return merged;
    }
    let warning = if merged.run().runtime_snapshots.is_empty() {
        "tailtriage-tracing session ran with background runtime sampling disabled and no manual runtime snapshots were recorded"
    } else {
        "tailtriage-tracing session ran with background runtime sampling disabled; runtime snapshots were manually recorded"
    };
    let (mut run, mut warnings, retained_sources) = merged.into_internal_parts();
    if !run.metadata.lifecycle_warnings.iter().any(|w| w == warning) {
        run.metadata.lifecycle_warnings.push(warning.to_string());
    }
    if !warnings.iter().any(|w| w.message() == warning) {
        warnings.push(ImportWarning::new(warning.to_string()));
    }
    merged = ImportedRun::with_retained_sources(run, warnings, retained_sources);
    merged
}

const MERGED_RUNTIME_SNAPSHOT_RUN_OFFSET_WARNING: &str = "TracingSession merged runtime snapshots from a separate runtime collector; runtime snapshot at_run_us offsets were cleared so temporal runtime attribution uses Unix-ms windows.";

pub(crate) fn merge_runtime_data(imported: ImportedRun, runtime_run: &Run) -> ImportedRun {
    let (mut tracing_run, mut warnings, retained_sources) = imported.into_internal_parts();
    let runtime_snapshot_offsets_cleared = runtime_run
        .runtime_snapshots
        .iter()
        .any(|snapshot| snapshot.at_run_us.is_some());
    let mut runtime_snapshots = runtime_run.runtime_snapshots.clone();
    for snapshot in &mut runtime_snapshots {
        snapshot.at_run_us = None;
    }
    tracing_run.runtime_snapshots = runtime_snapshots;
    if !tracing_run.runtime_snapshots.is_empty() {
        let runtime_min = tracing_run
            .runtime_snapshots
            .iter()
            .map(|snapshot| snapshot.at_unix_ms)
            .min()
            .expect("non-empty runtime snapshots have a minimum timestamp");
        let runtime_max = tracing_run
            .runtime_snapshots
            .iter()
            .map(|snapshot| snapshot.at_unix_ms)
            .max()
            .expect("non-empty runtime snapshots have a maximum timestamp");

        tracing_run.metadata.started_at_unix_ms =
            tracing_run.metadata.started_at_unix_ms.min(runtime_min);
        let finalized = tracing_run
            .metadata
            .finalized_at_unix_ms
            .unwrap_or(tracing_run.metadata.started_at_unix_ms)
            .max(runtime_max);
        tracing_run.metadata.finalized_at_unix_ms = Some(finalized);
    }
    tracing_run.metadata.effective_tokio_sampler_config =
        runtime_run.metadata.effective_tokio_sampler_config;
    tracing_run.truncation.dropped_runtime_snapshots =
        runtime_run.truncation.dropped_runtime_snapshots;
    tracing_run.truncation.limits_hit =
        tracing_run.truncation.limits_hit || runtime_run.truncation.limits_hit;
    for warning in &runtime_run.metadata.lifecycle_warnings {
        if !tracing_run.metadata.lifecycle_warnings.contains(warning) {
            tracing_run
                .metadata
                .lifecycle_warnings
                .push(warning.clone());
        }
        if !warnings
            .iter()
            .any(|import_warning| import_warning.message() == warning)
        {
            warnings.push(ImportWarning::new(warning.clone()));
        }
    }
    if runtime_snapshot_offsets_cleared {
        let warning = MERGED_RUNTIME_SNAPSHOT_RUN_OFFSET_WARNING;
        if !tracing_run
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|existing| existing == warning)
        {
            tracing_run
                .metadata
                .lifecycle_warnings
                .push(warning.to_string());
        }
        if !warnings
            .iter()
            .any(|import_warning| import_warning.message() == warning)
        {
            warnings.push(ImportWarning::new(warning.to_string()));
        }
    }
    ImportedRun::with_retained_sources(tracing_run, warnings, retained_sources)
}
