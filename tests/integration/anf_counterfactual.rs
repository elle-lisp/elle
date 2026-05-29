// ANF counter-factual tests.
//
// Three `tests/elle/*.lisp` scripts exhibit the closure-binding-overwrite
// bug (Family C in notes.md): a `letrec`-bound closure called inside a
// `while` body succeeds on the first two iterations, then fails with
// `expected 0 arguments, got 1` on the third. The binding slot has been
// overwritten with an anonymous inner closure of arity 0.
//
// The fix is the A-normal form lift (`src/hir/anf.rs`): every allocating
// expression gets a name, so the lowerer's `binding_to_slot` is the sole
// slot-tracking mechanism — eliminating the shadow `call_region_slot` map
// that the previous code had to maintain in parallel.
//
// These tests pin down causality. Each script is run twice:
//
// 1. **Default** — `--anf` is on. Asserts the script exits 0.
// 2. **`--anf=off`** — the ANF lift is short-circuited to a no-op.
//    Asserts the script exits *non-zero* (the bug returns).
//
// Without (2), an unrelated change between the failing and passing trees
// could appear to "fix" the bug while the real cause hides elsewhere.
// With (2), if anything other than ANF were the cause, the
// `--anf=off` test would still pass — and the regression would be
// caught by `cargo test integration::anf_counterfactual`.
//
// The `--anf=off` flag is a counter-factual hatch. Per the s11 plan, it
// should be removed in a follow-up commit once the causality is reviewed.

use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Run `tests/elle/{name}.lisp` with the given extra args and return
/// `(success, stdout, stderr, exit_code)`.
fn run_script(name: &str, extra_args: &[&str]) -> (bool, String, String, Option<i32>) {
    let bin = elle_binary();
    let script = format!("tests/elle/{}.lisp", name);
    let output = Command::new(bin)
        .args(extra_args)
        .arg(&script)
        .output()
        .unwrap_or_else(|e| panic!("Failed to spawn elle for {} {:?}: {}", script, extra_args, e));
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

/// Assert that a script passes under the default configuration
/// (ANF on). Used to demonstrate the bug is fixed.
fn assert_passes_with_anf(name: &str) {
    let (ok, out, err, code) = run_script(name, &["--jit=off"]);
    assert!(
        ok,
        "tests/elle/{}.lisp must pass with ANF on (default).\n\
         exit: {:?}\nstdout:\n{}\nstderr:\n{}",
        name, code, out, err
    );
}

/// Assert that a script fails with `--anf=off`. Used to demonstrate
/// the bug returns when the ANF transform is short-circuited — the
/// fix really does come from ANF, not from some other branch change.
fn assert_fails_with_anf_off(name: &str) {
    let (ok, out, err, code) = run_script(name, &["--jit=off", "--anf=off"]);
    assert!(
        !ok,
        "tests/elle/{}.lisp must FAIL with --anf=off (counter-factual: bug returns when ANF is disabled).\n\
         If this assertion fires after a future code change, the change either fixed the bug \
         independently of ANF (causality broken — investigate before deleting the flag) or the \
         flag itself is no longer wired correctly.\n\
         exit: {:?}\nstdout:\n{}\nstderr:\n{}",
        name, code, out, err
    );
}

// ── Family C: closure-binding overwrite ───────────────────────────
//
// All three scripts share the same shape: a `while` body calls a
// `letrec`-bound closure; the slot gets aliased on the 3rd iteration.
// ANF makes the aliasing impossible by ensuring every closure
// allocation site has a distinct binding.

#[test]
fn jit_lbox_param_repro_passes_with_anf() {
    assert_passes_with_anf("jit-lbox-param-repro");
}

#[test]
fn jit_lbox_param_repro_fails_with_anf_off() {
    assert_fails_with_anf_off("jit-lbox-param-repro");
}

#[test]
fn jit_lbox_param_noyield_passes_with_anf() {
    assert_passes_with_anf("jit-lbox-param-noyield");
}

#[test]
fn jit_lbox_param_noyield_fails_with_anf_off() {
    assert_fails_with_anf_off("jit-lbox-param-noyield");
}

#[test]
fn letstar_yield_repro_passes_with_anf() {
    assert_passes_with_anf("letstar-yield-repro");
}

#[test]
fn letstar_yield_repro_fails_with_anf_off() {
    assert_fails_with_anf_off("letstar-yield-repro");
}
