// `--dump=STAGE[,STAGE,...]` CLI surface tests.
//
// Each stage runs the compiler up to a well-defined point and prints
// the artifact. The test verifies the banner is emitted and that the
// body contains characteristic markers for each stage. This guards
// against the CLI regressing or losing stages.

use std::process::Command;

fn elle() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn dump(stages: &str, source: &str) -> (String, String, std::process::ExitStatus) {
    let output = Command::new(elle())
        .arg(format!("--dump={}", stages))
        .arg("-e")
        .arg(source)
        .output()
        .expect("spawn elle");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status,
    )
}

#[test]
fn ast_prints_parsed_form() {
    let (out, _, status) = dump("ast", "(+ 1 2)");
    assert!(status.success(), "elle --dump=ast exited non-zero");
    assert!(out.contains("── ast"), "missing ast banner:\n{}", out);
    assert!(out.contains("(+ 1 2)"), "missing parsed form:\n{}", out);
}

#[test]
fn hir_prints_entry_and_closures() {
    let (out, _, status) = dump("hir", "(defn sq [x] (* x x)) (sq 3)");
    assert!(status.success());
    assert!(out.contains("── hir"), "missing hir banner:\n{}", out);
    assert!(out.contains("entry"), "missing entry marker:\n{}", out);
    assert!(
        out.contains("closure[0]"),
        "missing closure marker:\n{}",
        out
    );
}

#[test]
fn lir_prints_blocks_and_registers() {
    let (out, _, status) = dump("lir", "(+ 1 2)");
    assert!(status.success());
    assert!(out.contains("── lir"), "missing lir banner:\n{}", out);
    assert!(out.contains("block0:"), "missing block0:\n{}", out);
    // Register assignment arrow appears in the LIR pretty-printer.
    assert!(out.contains("←"), "missing register arrow:\n{}", out);
}

#[test]
fn cfg_prints_block_edges() {
    let (out, _, status) = dump("cfg", "(if true 1 2)");
    assert!(status.success());
    assert!(out.contains("── cfg"), "missing cfg banner:\n{}", out);
    // Successor arrow, one block → [other].
    assert!(out.contains("→"), "missing successor arrow:\n{}", out);
}

#[test]
fn dfa_reports_signal_and_rotation_safety() {
    let (out, _, status) = dump("dfa", "(defn f [x] x)");
    assert!(status.success());
    assert!(out.contains("── dfa"), "missing dfa banner:\n{}", out);
    assert!(
        out.contains("rotation_safe="),
        "missing rotation_safe flag:\n{}",
        out
    );
    assert!(
        out.contains("capture_params_mask="),
        "missing capture mask:\n{}",
        out
    );
}

#[test]
fn jit_reports_eligibility() {
    let (out, _, status) = dump("jit", "(+ 1 2)");
    assert!(status.success());
    assert!(out.contains("── jit"), "missing jit banner:\n{}", out);
    assert!(out.contains("eligible="), "missing eligibility:\n{}", out);
}

#[test]
fn git_stage_emits_per_closure_output() {
    // `git` is the SPIR-V dump stage. It emits a per-closure block;
    // without the `mlir` feature, each block notes that the feature is
    // required. With the feature, it emits real SPIR-V words. Either way
    // the banner must appear and at least one `entry` line must follow.
    let (out, _, status) = dump("git", "(+ 1 2)");
    assert!(status.success());
    assert!(out.contains("── git"), "missing git banner:\n{}", out);
    assert!(
        out.contains("; entry"),
        "expected per-closure 'entry' line in git dump:\n{}",
        out
    );
}

#[test]
fn regions_prints_region_assignments() {
    let (out, _, status) = dump("regions", "(let [x 1] (+ x 2))");
    assert!(status.success());
    assert!(
        out.contains("── regions"),
        "missing regions banner:\n{}",
        out
    );
    assert!(
        out.contains("region assignments"),
        "missing region assignments section:\n{}",
        out
    );
    assert!(
        out.contains("region inference stats"),
        "missing stats:\n{}",
        out
    );
}

#[test]
fn all_stages_run_in_pipeline_order() {
    let (out, _, status) = dump("all", "(defn f [x] x)");
    assert!(status.success());
    // Ordering follows the compilation pipeline: AST → HIR → LIR →
    // CFG → DFA → JIT → git (SPIR-V). `git` is last because it is a
    // codegen stage like JIT, not a pre-analysis stage.
    let order = ["ast", "hir", "lir", "cfg", "dfa", "jit", "git"];
    let mut last = 0;
    for stage in order {
        let banner = format!("── {}", stage);
        let idx = out
            .find(&banner)
            .unwrap_or_else(|| panic!("missing stage '{}':\n{}", stage, out));
        assert!(
            idx >= last,
            "stage '{}' appeared out of pipeline order:\n{}",
            stage,
            out
        );
        last = idx;
    }
}

#[test]
fn unknown_stage_is_rejected() {
    let (_, err, status) = dump("bogus", "(+ 1 2)");
    assert!(!status.success(), "expected non-zero exit for bogus stage");
    assert!(
        err.contains("--dump: unknown stage 'bogus'")
            && err.contains("Valid: ast, hir, fhir, lir, jit, cfg, dfa, defuse, regions, git"),
        "expected helpful error listing valid stages, got:\n{}",
        err
    );
}
