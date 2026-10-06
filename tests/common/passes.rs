// audited: 2026-10-05
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

    /// Whether the pass reads each file's charge on the runner's heap.
    pub fn charge(&self) -> bool {
        self.command.split_whitespace().any(|w| w == "--charge")
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

/// Every file a ledger under `tests/ledger` names in its `(producer "…")`
/// header: the producers, which run in a pass of their own. The runner's own
/// producer, `elle test`, is no file.
#[allow(dead_code)]
pub fn producer_files() -> BTreeSet<String> {
    let dir = super::repo_root().join("tests/ledger");
    let mut out = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("tests/ledger exists") {
        let text = std::fs::read_to_string(entry.expect("a directory entry").path())
            .expect("read a ledger");
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("(producer \"") {
                let producer = rest.trim_end_matches("\")");
                if producer.ends_with(".lisp") {
                    out.insert(producer.to_string());
                }
            }
        }
    }
    assert!(!out.is_empty(), "tests/ledger names no producer");
    out
}

/// The file whose claim needs a process of its own: it asserts that a program
/// cannot change the JIT policy, which the in-process runner sets.
#[allow(dead_code)]
pub const CHARGE_SKIP: &str = "tests/impl/config.lisp";

/// Every file whose charge on the runner's heap the charge pass reads: the
/// language suite and the implementation files with no sidecar, less the
/// producers and `CHARGE_SKIP` (docs/test-gauges.md).
#[allow(dead_code)]
pub fn charge_files() -> BTreeSet<String> {
    let root = super::repo_root();
    let producers = producer_files();
    lang_files()
        .into_iter()
        .chain(
            impl_files()
                .into_iter()
                .filter(|f| !root.join(f.replace(".lisp", ".toml")).exists()),
        )
        .filter(|f| !producers.contains(f) && f != CHARGE_SKIP)
        .collect()
}

/// The implementation suite less the producers: what an isolated pass runs.
#[allow(dead_code)]
pub fn isolated_impl_files() -> BTreeSet<String> {
    impl_files()
        .difference(&producer_files())
        .cloned()
        .collect()
}

/// Assert that `target` runs every producer in-process on the rig `rig`, in a
/// pass of their own, and none of them in an isolated pass (docs/test-runner.md).
#[allow(dead_code)]
pub fn assert_producer_pass(target: &str, passes: &[Pass], rig: &str) {
    let producers = producer_files();
    let own: Vec<&Pass> = passes.iter().filter(|p| p.files == producers).collect();
    assert_eq!(
        own.len(),
        1,
        "`make {target}` runs the producers in one pass of their own"
    );
    assert_eq!(
        own[0].isolate(),
        None,
        "`make {target}` isolates the producers, so no reading is read under both JIT policies"
    );
    assert_rig_runs(own[0], rig, &format!("`make {target}`'s producer pass"));
    for pass in passes.iter().filter(|p| p.isolate().is_some()) {
        let twice: Vec<&String> = pass.files.intersection(&producers).collect();
        assert!(
            twice.is_empty(),
            "`make {target}` runs producers in an isolated pass too: {twice:?}"
        );
    }
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
