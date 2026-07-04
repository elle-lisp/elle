//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::runtime::Runtime;

// Mirror the runtime environment the runner compiles in (main.rs
// run_test_subcommand): a full `Runtime` — primitives registered, stdlib
// loaded into its own `CompileCtx`, and the VM/symbol-table thread contexts
// installed (the jit dump reads the VM context; the analyzer's `syntax->datum`
// reads the symbol context). `parts()` hands out the disjoint borrows
// `render_all` threads explicitly.
fn setup() -> Runtime {
    Runtime::new()
}

#[test]
fn render_all_yields_the_pipeline_artifacts() {
    let mut rt = setup();
    let (_vm, symbols, cctx) = rt.parts();
    let dumps = render_all("(defn sq [x] (* x x)) (sq 3)", "<test>", symbols, cctx);
    // The headline value: a queryable LIR body for the form.
    let lir = dumps.get("lir").expect("lir artifact present");
    assert!(lir.contains("block0:"), "lir missing block label:\n{}", lir);
    assert!(lir.contains('←'), "lir missing register arrow:\n{}", lir);
    // AST round-trips the source form.
    assert!(
        dumps.get("ast").expect("ast present").contains("sq"),
        "ast missing the defn name"
    );
    // Front-end + lowered stages all populated for a compiling module.
    for kind in [
        "ast", "fhir", "defuse", "regions", "hir", "lir", "cfg", "dfa", "jit",
    ] {
        assert!(dumps.contains_key(kind), "missing dump kind {}", kind);
    }
    // DFA / JIT carry their characteristic markers (same as the CLI).
    assert!(dumps["dfa"].contains("capture_params_mask="));
    assert!(dumps["jit"].contains("eligible="));
}

#[test]
fn render_all_on_uncompilable_source_omits_lowered_stages() {
    let mut rt = setup();
    let (_vm, symbols, cctx) = rt.parts();
    // Unbalanced/garbage that fails the reader → no AST, no lowered stages,
    // but the call itself does not panic (stages are independently fallible).
    let dumps = render_all("(this is (unclosed", "<test>", symbols, cctx);
    assert!(
        !dumps.contains_key("lir"),
        "uncompilable source yielded lir"
    );
}
