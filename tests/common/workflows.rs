// audited: 2026-10-21
//! The CI workflow files, the jobs in them, and the steps in a job.
//!
//! docs/analysis/ci.md
//!
//! `workflows.rs` asks what the pull-request gate waits for, `ci_steps.rs`
//! what each step runs, `run_artifacts.rs` what each suite job leaves behind,
//! `toolchains.rs` what each job's toolchain carries, and `suite_plugins.rs`
//! whether a suite job builds the suite plugins first. Each needs the same
//! reading — a workflow file split into jobs — and a second copy of it is a
//! second answer to "what is a job".

use super::repo_root;

/// Every workflow file, as (repo-relative path, text).
#[allow(dead_code)]
pub fn workflow_files() -> Vec<(String, String)> {
    let dir = repo_root().join(".github/workflows");
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|entry| {
            let path = entry.expect("a workflow directory entry").path();
            let name = path.file_name()?.to_str()?.to_string();
            if !name.ends_with(".yml") && !name.ends_with(".yaml") {
                return None;
            }
            let text = std::fs::read_to_string(&path).expect("read a workflow file");
            Some((format!(".github/workflows/{name}"), text))
        })
        .collect();
    out.sort();
    assert!(
        !out.is_empty(),
        "no workflow files read from {}",
        dir.display()
    );
    out
}

/// `  name:` at exactly two spaces of indent — a key in the `jobs` mapping.
/// A comment at that indent has no bare identifier before the colon, so the
/// character check rejects it.
fn job_header(line: &str) -> Option<String> {
    let rest = line.strip_prefix("  ")?;
    if rest.starts_with(' ') {
        return None;
    }
    let name = rest.strip_suffix(':')?;
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    if name.is_empty() || !name.chars().all(ident) {
        return None;
    }
    Some(name.to_string())
}

/// Every job in one workflow, as (name, body). Comment lines are dropped from
/// the body: the trap is that several jobs discuss commands they do not run —
/// the WASM job's comment names `make smoke-wasm` while running `check-wasm` —
/// so a body scan that kept comments would read those as steps.
#[allow(dead_code)]
pub fn workflow_jobs(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, String)> = None;
    let mut in_jobs = false;

    for line in text.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        // Any other top-level key closes the `jobs` mapping.
        if !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
        if let Some(name) = job_header(line) {
            out.extend(current.take());
            current = Some((name, String::new()));
            continue;
        }
        if let Some((_, body)) = current.as_mut() {
            if !line.trim_start().starts_with('#') {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    out.extend(current);
    out
}

/// The steps of one job, split at the `- ` that opens each.
#[allow(dead_code)]
pub fn job_steps(body: &str) -> Vec<String> {
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

/// The lines of a job's body that run something: every line but a step's
/// `name:`, which spells the command it labels.
#[allow(dead_code)]
pub fn step_lines(body: &str) -> Vec<&str> {
    body.lines()
        .filter(|line| {
            let line = line.trim_start().trim_start_matches("- ");
            !line.starts_with("name:")
        })
        .collect()
}

/// Does this job run `make TARGET`? The name has to end where the match does:
/// `make smoke-lang` is not a run of `make smoke`.
#[allow(dead_code)]
pub fn runs_target(body: &str, target: &str) -> bool {
    body.split(&format!("make {target}")).skip(1).any(|rest| {
        !rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// Every `make TARGET VAR=VALUE…` a run line of `body` starts, as the target
/// and its variables.
#[allow(dead_code)]
pub fn make_steps(body: &str) -> Vec<(String, Vec<String>)> {
    body.lines()
        .filter_map(|line| line.trim_start().strip_prefix("run: make "))
        .filter_map(|rest| {
            let mut words = rest.split_whitespace();
            let target = words.next()?.to_string();
            let vars = words
                .filter(|w| w.contains('='))
                .map(str::to_string)
                .collect();
            Some((target, vars))
        })
        .collect()
}
