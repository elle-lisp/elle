// audited: 2026-10-15
//! The Makefile's rules, and the targets among them that record `elle test` runs.
//!
//! docs/analysis/ci.md
//!
//! `run_artifacts.rs` asks which CI jobs record runs, `suites.rs` and
//! `corpus_targets.rs` which targets are suite targets, `ci_steps.rs` whether
//! a CI step runs a target of more than one pass, and `doctest.rs` which
//! packages `make doctest` builds. All of them read the same rules, and a
//! second copy of that reading is a second answer to "what does a target run".

use std::collections::{BTreeMap, BTreeSet};

use super::{make_expand, makefile};

/// Every rule in the makefile, as name → (prerequisites, recipe).
///
/// A rule line starts at column zero and names its target before the colon. A
/// target-specific variable assignment (`smoke-wasm: JOBS = …`) is a second
/// rule line for the same target, so the two entries merge. A rule line that
/// names several targets gives each of them the line.
#[allow(dead_code)]
pub fn makefile_rules() -> BTreeMap<String, (String, String)> {
    let text = makefile();
    let mut out: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in text.lines() {
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
        if head.contains('=') || head.is_empty() {
            continue;
        }
        let deps = deps.trim_start_matches('=').split('#').next().unwrap_or("");
        let names: Vec<&str> = head.split_whitespace().collect();
        for name in &names {
            let entry = out.entry(name.to_string()).or_default();
            entry.0.push(' ');
            entry.0.push_str(deps);
        }
        // A recipe follows a rule of one target alone in this Makefile.
        if names.len() == 1 {
            current = Some(names[0].to_string());
        }
    }
    out
}

/// The targets a recipe hands to a sub-make of this Makefile, with each
/// `$(VAR)` word expanded by `make`. A `$(MAKE) -C DIR` line runs another
/// Makefile, so its words name no target here.
fn handed_targets(recipe: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in recipe.split("$(MAKE)").skip(1) {
        let words: Vec<&str> = part
            .split('\t')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        if words.contains(&"-C") {
            continue;
        }
        for word in words {
            if word.starts_with('-') || word.contains('=') {
                continue;
            }
            match word.strip_prefix("$(").and_then(|w| w.strip_suffix(')')) {
                Some(var) => out.extend(make_expand(var).split_whitespace().map(str::to_string)),
                None => out.push(word.to_string()),
            }
        }
    }
    out
}

/// Every target whose own recipe drives the runner: the recipe calls
/// `RUN_SUITE`, which is the one way a recipe reaches `elle test`
/// (`run_artifacts.rs` pins that).
#[allow(dead_code)]
pub fn suite_targets() -> BTreeSet<String> {
    let found: BTreeSet<String> = makefile_rules()
        .into_iter()
        .filter(|(_, (_, recipe))| recipe.contains("$(call RUN_SUITE"))
        .map(|(name, _)| name)
        .collect();
    assert!(
        !found.is_empty(),
        "no makefile recipe calls RUN_SUITE; the parse is broken, not the Makefile"
    );
    found
}

/// Every make target that records runs: the suite targets, and every target
/// that reaches one through its prerequisites or through a sub-make its recipe
/// starts, however deep the chain.
#[allow(dead_code)]
pub fn recording_targets() -> BTreeSet<String> {
    let rules = makefile_rules();
    let mut found = suite_targets();
    let reaches: BTreeMap<&String, Vec<String>> = rules
        .iter()
        .map(|(name, (deps, recipe))| {
            let mut next: Vec<String> = deps.split_whitespace().map(str::to_string).collect();
            next.extend(handed_targets(recipe));
            (name, next)
        })
        .collect();
    loop {
        let grown: BTreeSet<String> = reaches
            .iter()
            .filter(|(_, next)| next.iter().any(|t| found.contains(t)))
            .map(|(name, _)| (*name).clone())
            .collect();
        let before = found.len();
        found.extend(grown);
        if found.len() == before {
            return found;
        }
    }
}
