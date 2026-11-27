// audited: 2026-11-26
// `make perf` records a profile perf can unwind through the JIT's frames.
//
// docs/impl/jit-map.md
//
// The recipes are read with `make -n`, so the tests build and record nothing.
// A whole stack needs three things the recipe states: Rust frame pointers in
// the build, the frame-pointer unwinder in the record, and the binary that
// build produced.

use std::process::Command;

use crate::common::make_dry_run_with;

/// The commands `make TARGET FILE=demos/probe.lisp ARGS=one` would run, one
/// per line.
fn recipe(target: &str) -> String {
    make_dry_run_with(target, &["FILE=demos/probe.lisp", "ARGS=one"])
        .unwrap_or_else(|| panic!("make --dry-run {target} failed"))
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
    let build = line_with(&recipe("perf"), "cargo build");
    assert!(build.contains("--profile perf"), "{build}");
    assert!(build.contains("-Cforce-frame-pointers=yes"), "{build}");
}

/// The counter-factual: perf's default unwinder reads no call graph, and the
/// DWARF one stops at the first JIT frame.
#[test]
fn the_record_unwinds_by_frame_pointer_and_runs_the_perf_build() {
    let record = line_with(&recipe("perf"), "perf record");
    assert!(record.contains("--call-graph fp"), "{record}");
    assert!(
        record.contains("./target/perf/elle --jit-verify=off demos/probe.lisp one"),
        "the perf build runs FILE with ARGS after it: {record}"
    );
}

/// A fixed output name would overwrite the last program's profile.
#[test]
fn each_program_records_to_a_file_of_its_own_name() {
    let record = line_with(&recipe("perf"), "perf record");
    assert!(record.contains("-o target/perf-probe.data"), "{record}");
}

/// A build with debug assertions verifies by default. The flag holds the
/// verifier off whatever the build, so the profile never charges its work to
/// the program.
#[test]
fn the_record_runs_without_the_verifier() {
    let record = line_with(&recipe("perf"), "perf record");
    assert!(record.contains("--jit-verify=off"), "{record}");
}

/// The counter-factual: the perf build carries no blob, so a record with no
/// warm-cache image to read charges the program a source boot. On fib that is
/// four fifths of the profile. `--cache=` on either start leaves the record
/// without that image.
#[test]
fn the_record_starts_from_the_image_a_warm_up_stored() {
    let recipe = recipe("perf");
    let lines: Vec<&str> = recipe.lines().collect();
    let at = |needle: &str| {
        lines
            .iter()
            .position(|l| l.trim_start().starts_with(needle))
            .unwrap_or_else(|| panic!("a line starts with `{needle}`:\n{recipe}"))
    };
    let build = at("cargo build");
    let warm = at("./target/perf/elle");
    let record = at("perf record");
    assert!(
        build < warm && warm < record,
        "the perf build starts once between the build and the record:\n{recipe}"
    );
    for start in [lines[warm], lines[record]] {
        assert!(!start.contains("--cache="), "{start}");
    }
}

#[test]
fn hotspot_records_then_opens_the_profile() {
    let recipe = recipe("hotspot");
    let record = recipe.find("perf record").expect("hotspot records first");
    let open = recipe
        .find("hotspot target/perf-probe.data")
        .expect("hotspot opens the profile it recorded");
    assert!(record < open, "the record runs before hotspot:\n{recipe}");
}

/// Without `FILE` the record would start the REPL under perf. The shared
/// dry-run helper drops stderr, which holds the refusal, so this test runs
/// make itself.
#[test]
fn perf_refuses_to_run_without_a_file() {
    let out = Command::new("make")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["--dry-run", "--no-print-directory", "perf"])
        .output()
        .expect("run make");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "make -n perf ran without FILE");
    assert!(stderr.contains("FILE"), "the refusal names FILE: {stderr}");
}
