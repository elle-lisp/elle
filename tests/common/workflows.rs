// audited: 2026-10-04
//! The CI workflow files, the jobs in them, and the steps in a job.
//!
//! docs/analysis/ci.md
//!
//! `workflows.rs` asks what the pull-request gate waits for,
//! `run_artifacts.rs` asks what each suite job leaves behind, and
//! `toolchains.rs` asks what each job's toolchain carries. All three need the
//! same reading — a workflow file split into jobs — and a second copy of it is a
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
