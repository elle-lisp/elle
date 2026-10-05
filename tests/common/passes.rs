// audited: 2026-10-04
//! The `elle test` passes a suite target runs, read off `make --dry-run`.
//!
//! docs/testing.md
//! rig/overview.md
//!
//! `suites.rs` asks what the default build's targets run, and `variants.rs`
//! what each other build's run. Both read a recipe the same way — one pass per
//! `elle test` batch line — and a second copy of that reading is a second answer
//! to "what is a pass".

use std::collections::BTreeSet;

use super::{make_dry_run_with, make_expand, suite};

/// One `elle test` batch pass: the files it deals out, and the command each
/// batch runs.
pub struct Pass {
    pub files: BTreeSet<String>,
    pub command: String,
}

#[allow(dead_code)]
impl Pass {
    /// The child flags the pass gives each file, what `--isolate` quotes; `None`
    /// for a pass that runs its files inside the runner.
    pub fn isolate(&self) -> Option<&str> {
        let (_, rest) = self.command.split_once("--isolate '")?;
        Some(rest.split_once('\'').map(|(flags, _)| flags).unwrap_or(""))
    }

    /// The program each child runs under, when the pass names one.
    pub fn host(&self) -> Option<&str> {
        let (_, rest) = self.command.split_once("--host ")?;
        rest.split_whitespace().next()
    }

    /// The binary each batch runs `test` on: the runner, and the build every
    /// file runs on, in-process or, under `--isolate` with no `--host`, as
    /// each child.
    pub fn runner(&self) -> &str {
        let (head, _) = self
            .command
            .split_once(" test ")
            .unwrap_or_else(|| panic!("a pass that runs no `test`: {}", self.command));
        head.split_whitespace().last().unwrap_or("")
    }
}

/// Every suite pass `make TARGET VARS…` will run, in order.
#[allow(dead_code)]
pub fn passes(target: &str, vars: &[&str]) -> Vec<Pass> {
    let recipe = make_dry_run_with(target, vars)
        .unwrap_or_else(|| panic!("`make --dry-run {target}` did not run"));
    let passes: Vec<Pass> = recipe
        .lines()
        .filter(|line| line.contains(" test ") && line.contains("xargs"))
        .map(|line| Pass {
            files: line
                .split_whitespace()
                .filter(|w| w.starts_with("tests/") && w.ends_with(".lisp"))
                .map(str::to_string)
                .collect(),
            command: line.to_string(),
        })
        .collect();
    assert!(
        !passes.is_empty(),
        "`make {target}` runs no suite pass:\n{recipe}"
    );
    passes
}

/// Every file of the language suite.
#[allow(dead_code)]
pub fn lang_files() -> BTreeSet<String> {
    suite("tests/lang").into_iter().collect()
}

/// Every file of the implementation suite.
#[allow(dead_code)]
pub fn impl_files() -> BTreeSet<String> {
    suite("tests/impl").into_iter().collect()
}

/// Assert that `pass` runs as `RIG test`, so each child is the rig the
/// variable `rig` names and the run has that rig's build: no `--host` names
/// another program (docs/ratchet.md).
#[allow(dead_code)]
pub fn assert_rig_runs(pass: &Pass, rig: &str, what: &str) {
    assert_eq!(
        pass.runner(),
        make_expand(rig),
        "{what} runs under a runner that is not the rig {rig}:\n  {}",
        pass.command
    );
    assert_eq!(
        pass.host(),
        None,
        "{what} names a --host, so the run has no build:\n  {}",
        pass.command
    );
}

/// Assert that `pass` runs the language suite through the runner of the build
/// itself, the binary `runner` names: every file, in-process, with no flag.
#[allow(dead_code)]
pub fn assert_plain_language_pass(target: &str, pass: &Pass, runner: &str) {
    assert_eq!(
        pass.files,
        lang_files(),
        "`make {target}` does not run exactly the language suite"
    );
    assert_eq!(
        pass.runner(),
        make_expand(runner),
        "`make {target}` runs the language suite in another build than {runner}"
    );
    assert_eq!(
        pass.isolate(),
        None,
        "`make {target}` runs each language file as its own child, where the \
         runner cannot run it on each tier and no differential runs"
    );
    assert_eq!(
        pass.host(),
        None,
        "`make {target}` runs the language suite on another program than the build"
    );
    assert!(
        !pass.command.contains("--trace") && !pass.command.contains("--profile"),
        "`make {target}` hands the runner a mode, so the suite runs a runtime no \
         user runs:\n  {}",
        pass.command
    );
}
