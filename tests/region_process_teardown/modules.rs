// audited: 2026-11-21
//! The residue gates of a run that loads a module, by a loader's direct call or by an import.
//!
//! docs/impl/region/rules.md
//! docs/modules.md

use super::census::{residue_after_teardown, HandOff};
use super::*;
use elle::compiler::stdlib_cache::StdlibCache;

/// Run `src` on a fresh runtime, and require that teardown leaves no region,
/// and no reference the region graph cannot explain.
fn assert_no_residue(src: &str) {
    assert_eq!(
        residue_after_teardown(
            Runtime::with_stdlib_cache(StdlibCache::Off),
            src,
            HandOff::Root
        ),
        0,
        "{src}: regions survived teardown",
    );
    let pinned = pinned_after_teardown(Runtime::with_stdlib_cache(StdlibCache::Off), src);
    assert!(
        pinned.is_empty(),
        "{src}: regions pinned from outside the region graph: {pinned:#?}"
    );
}

/// A program that calls `PROGRAM` twice on a module file whose form is
/// `(fn [] @[1 2])`, and adds the lengths of the two arrays.
fn twice(dir: &tempfile::TempDir, program: &str) -> String {
    let path = dir.path().join("residue-load.lisp");
    std::fs::write(&path, "(fn [] @[1 2])\n").expect("write");
    format!(
        "(let [a (({program} \"{p}\")) b (({program} \"{p}\"))] (+ (length a) (length b)))",
        p = path.display()
    )
}

/// A run that loads one file twice leaves nothing. Each load reads the file,
/// compiles its form through `eval` on a loader fiber, and runs it under the
/// scheduler.
///
/// The control is `teardown_leaves_no_residue`
/// (tests/region_process_teardown/census.rs), whose programs load no file. A
/// unit that something outside the graph keeps pins its code region, and every
/// other gate reads clean.
#[test]
fn a_run_that_loads_a_file_twice_leaves_no_residue() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_no_residue(&twice(&dir, "import/load-file"));
}

/// A run that imports one file twice leaves nothing. The default loader keeps
/// the file's compiled form in a table that `ev/run` binds to a parameter. The
/// table is an Elle value inside the region graph, so the program's own
/// releases reach it and every compiled form it keeps.
///
/// The counter-factual kept the table on the VM and built it in an ordinary
/// region, as the plugin loader builds its struct. No release reaches that
/// region, so the table and the compiled forms it keeps survived teardown. The
/// same table in a process root left nothing, because the teardown sweep
/// releases the root.
#[test]
fn a_run_that_imports_a_file_twice_leaves_no_residue() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_no_residue(&twice(&dir, "import"));
}
