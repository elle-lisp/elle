// audited: 2026-11-25
//! The assertions a suite-target test makes about the passes `make --dry-run` shows.
//!
//! docs/testing.md
//! docs/test-runner.md
//!
//! Each check reads a `Pass` the way passes.rs reads it, so a target's test
//! and a variant's test hold one pass to one rule.

use super::super::make_expand;
use super::{build_producer_files, lang_files, producer_files, Pass};

/// Assert that `target` runs the build's producers in-process on the rig
/// `rig`, in a pass of their own, and no producer in an isolated pass
/// (docs/test-runner.md).
#[allow(dead_code)]
pub fn assert_producer_pass(target: &str, passes: &[Pass], rig: &str) {
    let producers = producer_files();
    let build = build_producer_files();
    let own: Vec<&Pass> = passes.iter().filter(|p| p.files == build).collect();
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
    assert_language_pass(target, pass, runner, &[]);
}

/// Assert that `pass` runs the language suite as a plain pass does, with
/// `modes` the only words that set the runner's mode.
#[allow(dead_code)]
pub fn assert_language_pass(target: &str, pass: &Pass, runner: &str, modes: &[&str]) {
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
    assert_eq!(
        pass.modes(),
        modes,
        "`make {target}` hands the runner another mode than it names, so the suite \
         runs a runtime nobody asked for:\n  {}",
        pass.command
    );
}
