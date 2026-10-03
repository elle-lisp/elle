// audited: 2026-09-29
// `make crosscheck` compiles for every target a CI job cross-compiles, so a
// break in a `cfg` arm shows locally first.
//
// docs/analysis/ci.md

use crate::common::{makefile, repo_root};
use std::collections::BTreeSet;

fn workflow_text() -> String {
    let path = repo_root().join(".github/workflows/pr.yml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every triple named by a `--target` flag in `text`, with a `$(VAR)` resolved
/// against the makefile's own assignments.
///
/// The workflow writes its triples out and the makefile hides them behind
/// variables, so a scan that read the literal text would compare a triple with
/// `$(ANDROID_TARGET)` and find nothing in common.
fn cross_targets(text: &str, makefile: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for part in text.split("--target ").skip(1) {
        let word = part.split_whitespace().next().unwrap_or("");
        let resolved = match word.strip_prefix("$(").and_then(|w| w.strip_suffix(')')) {
            Some(name) => assignment(makefile, name),
            None => Some(word.to_string()),
        };
        out.extend(resolved.filter(|t| t.contains('-')));
    }
    out
}

/// The value of a `NAME := value` or `NAME = value` line in a makefile.
fn assignment(makefile: &str, name: &str) -> Option<String> {
    makefile.lines().find_map(|line| {
        let rest = line.strip_prefix(name)?.trim_start();
        let rest = rest.strip_prefix(":=").or_else(|| rest.strip_prefix('='))?;
        Some(rest.trim().to_string())
    })
}

/// The recipe of one makefile target, up to the first line outside it.
fn recipe(makefile: &str, target: &str) -> String {
    makefile
        .lines()
        .skip_while(|line| !line.starts_with(&format!("{target}:")))
        .skip(1)
        .take_while(|line| line.starts_with('\t') || line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// A job that cross-compiles for a platform no runner executes is the only
// reader of the `cfg` arms that platform takes. Nothing local compiles them, so
// the arm's first reader is a runner and the report lands after the push.
// `make crosscheck` is the local gate, and it is only a gate over the targets
// it names — the argument is in docs/analysis/ci.md.
//
// The counter-factual: the Android job ran for months against a `crosscheck`
// that compiled the macOS arms alone. A `not(target_os = "linux")` arm reaching
// for a libc call bionic does not have compiled on every box a developer had,
// and CI was the first thing to say so.
#[test]
fn make_crosscheck_covers_every_target_a_job_cross_compiles() {
    let make = makefile();
    let in_ci = cross_targets(&workflow_text(), &make);
    let local = cross_targets(&recipe(&make, "crosscheck"), &make);

    assert!(
        in_ci.len() > 1,
        "found {in_ci:?} cross-compiled in .github/workflows/pr.yml; the parse is \
         broken, not the workflow"
    );

    let uncovered: Vec<_> = in_ci.difference(&local).collect();
    assert!(
        uncovered.is_empty(),
        "CI cross-compiles these targets and `make crosscheck` does not, so a \
         break in their `cfg` arms reaches a runner before it reaches anyone: \
         {uncovered:?}. Local coverage is {local:?}."
    );
}
