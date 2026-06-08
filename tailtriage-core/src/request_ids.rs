use std::collections::HashSet;

use crate::Run;

pub(crate) fn duplicate_completed_request_ids(run: &Run) -> Vec<String> {
    let mut seen = HashSet::<&str>::new();
    let mut duplicates = HashSet::<&str>::new();
    for request in &run.requests {
        if !seen.insert(request.request_id.as_str()) {
            duplicates.insert(request.request_id.as_str());
        }
    }
    let mut duplicates = duplicates
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    duplicates.sort();
    duplicates
}

pub(crate) fn add_duplicate_completed_request_id_warning(run: &mut Run) {
    let duplicates = duplicate_completed_request_ids(run);
    if duplicates.is_empty() {
        return;
    }
    let warning = format!(
        "duplicate completed request_id value(s) detected in retained request events: {}. request_id should be unique per completed logical request/work item in one Run; request-scoped attribution may be ambiguous.",
        duplicates.join(", ")
    );
    if !run
        .metadata
        .lifecycle_warnings
        .iter()
        .any(|existing| existing == &warning)
    {
        run.metadata.lifecycle_warnings.push(warning);
    }
}
