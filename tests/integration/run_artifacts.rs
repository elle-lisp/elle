// audited: 2026-09-22
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
// The trap is which jobs these are. A corpus pass that runs one process per
// file (`smoke-vm`, `smoke-jit`, `smoke-nouring`) records nothing: only the
// targets that reach `elle test` do, and their recipes say so by calling
// `RUN_CORPUS`. So the list is read out of the Makefile rather than written
// here, and a test below pins the assumption that makes reading it sound.

use crate::common::{workflow_files, workflow_jobs};
use std::collections::{BTreeMap, BTreeSet};

fn makefile() -> String {
    std::fs::read_to_string(crate::common::repo_root().join("Makefile")).expect("read the Makefile")
}

/// Every rule in the makefile, as name → (prerequisites, recipe).
///
/// A rule line starts at column zero and names its target before the colon. A
/// target-specific variable assignment (`smoke-boot-image: FLAGS += …`) is a
/// second rule line for the same target, so the two entries merge.
fn rules(makefile: &str) -> BTreeMap<String, (String, String)> {
    let mut out: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in makefile.lines() {
        if line.starts_with('\t') {
            if let Some(name) = &current {
                out.entry(name.clone()).or_default().1.push_str(line);
            }
            continue;
        }
        if line.trim().is_empty() || line.starts_with('#') || line.starts_with(' ') {
            continue;
        }
        current = None;
        let Some((head, deps)) = line.split_once(':') else {
            continue;
        };
        // `NAME := value` and `NAME = value` are assignments, not rules.
        if head.contains('=') || head.contains(' ') || head.is_empty() {
            continue;
        }
        let deps = deps.trim_start_matches('=').split('#').next().unwrap_or("");
        let entry = out.entry(head.to_string()).or_default();
        entry.0.push(' ');
        entry.0.push_str(deps);
        current = Some(head.to_string());
    }
    out
}

/// Every make target that records runs: the ones whose recipe drives the
/// runner, and every target that reaches one through its prerequisites.
fn recording_targets(makefile: &str) -> BTreeSet<String> {
    let rules = rules(makefile);
    let mut found: BTreeSet<String> = rules
        .iter()
        .filter(|(_, (_, recipe))| recipe.contains("$(RUN_CORPUS)"))
        .map(|(name, _)| name.clone())
        .collect();
    assert!(
        !found.is_empty(),
        "no makefile recipe calls RUN_CORPUS; the parse is broken, not the Makefile"
    );

    // A target that depends on a recording one records too, however deep the
    // chain: `smoke` reaches the runner only through `smoke-elle`.
    loop {
        let grown: BTreeSet<String> = rules
            .iter()
            .filter(|(_, (deps, _))| deps.split_whitespace().any(|d| found.contains(d)))
            .map(|(name, _)| name.clone())
            .collect();
        let before = found.len();
        found.extend(grown);
        if found.len() == before {
            return found;
        }
    }
}

/// Does this job run `make TARGET`? The name has to end where the match does:
/// `make smoke-vm` is not a run of `make smoke`.
fn runs_target(body: &str, target: &str) -> bool {
    body.split(&format!("make {target}")).skip(1).any(|rest| {
        !rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// The steps of one job, split at the `- ` that opens each.
fn steps(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in body.lines() {
        if line.starts_with("      - ") {
            out.push(String::new());
        }
        if let Some(step) = out.last_mut() {
            step.push_str(line);
            step.push('\n');
        }
    }
    out
}

/// The step that uploads an artifact, if the job has one.
fn upload_step(body: &str) -> Option<String> {
    steps(body)
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
    let targets = recording_targets(&makefile());
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
        jobs.iter().map(|(w, j, _)| format!("{w}:{j}")).collect::<Vec<_>>()
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
    assert!(found > 0, "no recording job uploads anything; the sweep proved nothing");
}

/// What makes the list above readable out of the Makefile: a recipe reaches
/// the runner through `RUN_CORPUS` and nowhere else. A target invoking
/// `$(ELLE) test` directly would record runs that no job is asked to upload,
/// and every check here would pass over it.
#[test]
fn only_run_corpus_drives_the_runner() {
    let makefile = makefile();
    assert!(
        makefile.contains("$(ELLE) test"),
        "no recipe invokes the runner; this test is reading for the wrong string"
    );

    let direct: Vec<String> = rules(&makefile)
        .into_iter()
        .filter(|(_, (_, recipe))| recipe.contains("$(ELLE) test"))
        .map(|(name, _)| name)
        .collect();
    assert!(
        direct.is_empty(),
        "these targets run the runner outside RUN_CORPUS, so the jobs that run \
         them record runs nothing uploads: {direct:?}"
    );
}
