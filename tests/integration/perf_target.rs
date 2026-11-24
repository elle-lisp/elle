// audited: 2026-11-23
// `make perf` records a profile perf can unwind through the JIT's frames.
//
// docs/impl/jit-map.md
//
// The recipe is read with `make -n`, so the test builds and records nothing.
// A whole stack needs three things the recipe states: Rust frame pointers in
// the build, the frame-pointer unwinder in the record, and the binary that
// build produced.

use std::process::Command;

/// The commands `make perf ARGS=probe.lisp` would run, one per line.
fn recipe() -> String {
    let out = Command::new("make")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["--no-print-directory", "-n", "perf", "ARGS=probe.lisp"])
        .output()
        .expect("run make");
    assert!(
        out.status.success(),
        "make -n perf failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The one line of the recipe that holds `needle`.
fn line_with(recipe: &str, needle: &str) -> String {
    let lines: Vec<&str> = recipe.lines().filter(|l| l.contains(needle)).collect();
    assert_eq!(lines.len(), 1, "one line holds `{needle}`:\n{recipe}");
    lines[0].to_string()
}

/// The counter-factual: without the flag, Rust code omits frame pointers, and
/// the unwinder loses the caller of every Rust frame. The profile still
/// records, so only the flame graph shows the loss.
#[test]
fn the_build_keeps_frame_pointers_in_the_perf_profile() {
    let build = line_with(&recipe(), "cargo build");
    assert!(build.contains("--profile perf"), "{build}");
    assert!(build.contains("-Cforce-frame-pointers=yes"), "{build}");
}

/// The counter-factual: perf's default unwinder reads no call graph, and the
/// DWARF one stops at the first JIT frame.
#[test]
fn the_record_unwinds_by_frame_pointer_and_runs_the_perf_build() {
    let record = line_with(&recipe(), "perf record");
    assert!(record.contains("--call-graph fp"), "{record}");
    assert!(record.contains("./target/perf/elle"), "{record}");
    assert!(record.contains("probe.lisp"), "{record}");
}

/// The Makefile exports `ELLE_JIT_VERIFY=on`, so without the flag the
/// profile would charge the verifier's work to the program.
#[test]
fn the_record_runs_without_the_verifier() {
    let record = line_with(&recipe(), "perf record");
    assert!(record.contains("--jit-verify=off"), "{record}");
}
