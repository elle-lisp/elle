// audited: 2026-10-15
// A CI job that records runs publishes the store it recorded them in, so a red
// job is read with a query rather than out of its log.
//
// docs/analysis/ci.md
// docs/test-store.md
//
// The counter-factual: this is the state main was in. `elle test` wrote a
// session DB and a CAS on the runner, the job ended, and the machine went away
// with both — leaving the scrollback the runner exists to replace. Nothing went
// red, because a store nobody uploads is invisible to every other check.
//
// The trap is which jobs these are. A target records runs when its recipe
// reaches `elle test`, and a recipe says so by calling `RUN_SUITE`. So the list
// is read out of the Makefile rather than written here, and a test below pins
// the assumption that makes reading it sound.

use crate::common::{
    job_steps, makefile, makefile_rules, recording_targets, runs_target, workflow_files,
    workflow_jobs,
};
use std::collections::BTreeMap;

/// The step that uploads an artifact, if the job has one.
fn upload_step(body: &str) -> Option<String> {
    job_steps(body)
        .into_iter()
        .find(|step| step.contains("actions/upload-artifact"))
}

/// The value of one `key:` in a block, as written.
fn field(block: &str, key: &str) -> Option<String> {
    let (_, rest) = block.split_once(&format!("{key}:"))?;
    Some(rest.lines().next().unwrap_or("").trim().to_string())
}

/// The artifact a step uploads, which is the `name:` under its `with:`. The
/// step's own `name:` comes first in the text and is a label for the log, so
/// reading the first one would compare two labels and prove nothing.
fn artifact_name(upload: &str) -> Option<String> {
    let (_, with) = upload.split_once("with:")?;
    field(with, "name")
}

/// Every (workflow, job, body) that runs a target recording runs.
fn recording_jobs() -> Vec<(String, String, String)> {
    let targets = recording_targets();
    let mut out = Vec::new();
    for (path, text) in workflow_files() {
        for (name, body) in workflow_jobs(&text) {
            if targets.iter().any(|t| runs_target(&body, t)) {
                out.push((path.clone(), name, body));
            }
        }
    }
    out
}

/// The merge queue runs `make smoke` as steps, one per pass, and its store is
/// the one a reader wants when a queue run goes red. The counter-factual: a
/// reading that took `make smoke` as the one recording target misses every
/// pass the queue runs as a step of its own, and the queue's upload goes
/// unchecked with every test here still green.
#[test]
fn the_merge_queue_smoke_job_records_runs() {
    let found = recording_jobs()
        .into_iter()
        .any(|(path, name, _)| path.ends_with("/merge-queue.yml") && name == "tests");
    assert!(
        found,
        "the merge queue's `tests` job runs the passes of `make smoke` and reads \
         as recording no run"
    );
}

/// A store left on the runner is a store nobody can query. The upload has to
/// happen on the failing run above all, so `if: always()` is half the claim and
/// the artifact naming the store's own directory is the other half.
#[test]
fn every_job_that_records_a_run_uploads_its_store() {
    let jobs = recording_jobs();
    assert!(
        jobs.len() > 2,
        "found {} jobs recording runs; the parse is broken, not the workflows: {:?}",
        jobs.len(),
        jobs.iter()
            .map(|(w, j, _)| format!("{w}:{j}"))
            .collect::<Vec<_>>()
    );

    for (path, name, body) in jobs {
        let where_ = format!("{path} job `{name}`");
        let upload = upload_step(&body)
            .unwrap_or_else(|| panic!("{where_} records runs and uploads nothing"));

        assert_eq!(
            field(&upload, "if").as_deref(),
            Some("always()"),
            "{where_} uploads its store only on some outcomes; a failed run is \
             the one a reader needs"
        );
        assert!(
            field(&upload, "retention-days").is_some(),
            "{where_} uploads its store without a retention window"
        );

        // The state directory the job names is the directory it has to upload.
        // A job that uploads some other path ships an artifact that imports as
        // nothing, and reports success doing it.
        let state = field(&body, "ELLE_STATE")
            .unwrap_or_else(|| panic!("{where_} records runs and names no ELLE_STATE"));
        let dir = state
            .rsplit_once("}}/")
            .map(|(_, tail)| tail.to_string())
            .unwrap_or(state);
        assert!(
            !dir.is_empty() && upload.contains(&dir),
            "{where_} points ELLE_STATE at `{dir}` and uploads something else:\n{upload}"
        );
    }
}

/// Two artifacts of one name are one artifact, so a second job's store either
/// collides or replaces the first. The names also have to say which job left
/// which store, since that is all a reader has to choose between them.
#[test]
fn no_two_jobs_upload_under_one_artifact_name() {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut found = 0;

    for (path, name, body) in recording_jobs() {
        let Some(upload) = upload_step(&body) else {
            continue;
        };
        let artifact = artifact_name(&upload)
            .unwrap_or_else(|| panic!("{path} job `{name}` uploads an unnamed artifact"));
        found += 1;
        if let Some(other) = seen.insert(format!("{path}:{artifact}"), name.clone()) {
            panic!("{path} jobs `{other}` and `{name}` both upload `{artifact}`");
        }
    }
    assert!(
        found > 0,
        "no recording job uploads anything; the sweep proved nothing"
    );
}

/// What makes the list above readable out of the Makefile: a recipe reaches
/// the runner through `RUN_SUITE` and nowhere else. A target invoking
/// `$(ELLE) test` directly would record runs that no job is asked to upload,
/// and every check here would pass over it.
///
/// `RUN_SUITE` runs `$(SUITE_ELLE) test`, the binary of the build a target
/// runs its suites on. A direct call names that variable or one of the
/// binaries it can name, so the scan reads for all of them.
#[test]
fn only_run_suite_drives_the_runner() {
    let makefile = makefile();
    assert!(
        makefile.contains("$(SUITE_ELLE) test"),
        "no recipe invokes the runner; this test is reading for the wrong string"
    );

    let runners = [
        "$(SUITE_ELLE) test",
        "$(ELLE) test",
        "$(ELLE_WASM) test",
        "$(ELLE_MLIR) test",
        "$(ELLE_RIG) test",
        "$(ELLE_RIG_MLIR) test",
        "$(ELLE_RIG_WASM) test",
    ];
    let direct: Vec<String> = makefile_rules()
        .into_iter()
        .filter(|(_, (_, recipe))| runners.iter().any(|runner| recipe.contains(runner)))
        .map(|(name, _)| name)
        .collect();
    assert!(
        direct.is_empty(),
        "these targets run the runner outside RUN_SUITE, so the jobs that run \
         them record runs nothing uploads: {direct:?}"
    );
}
