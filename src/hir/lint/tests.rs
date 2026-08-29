//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::pipeline::CompileCtx;
use crate::primitives::register_primitives;
use crate::vm::VM;

/// Analyze test source with a fresh per-call `CompileCtx` — each compile names
/// its instance's compile state explicitly (docs/impl/region/ctx.md), so the
/// compile context is threaded as a parameter rather than shared. A thin shim
/// over `pipeline::analyze` so the call
/// sites read exactly as the runtime entry point did.
fn analyze(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<crate::pipeline::AnalyzeResult, String> {
    let mut cctx = CompileCtx::new();
    crate::pipeline::analyze(source, symbols, vm, &mut cctx, source_name)
}

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

#[test]
fn test_hir_linter_creation() {
    let linter = HirLinter::new();
    assert_eq!(linter.diagnostics().len(), 0);
    assert!(!linter.has_errors());
    assert!(!linter.has_warnings());
}

#[test]
fn test_hir_linter_arity_check() {
    let (mut symbols, mut vm) = setup();
    // length expects 1 argument — the analyzer catches this as a hard error
    let result = analyze("(length 1 2)", &mut symbols, &mut vm, "<test>");
    match result {
        Err(ref msg) => assert!(
            msg.contains("arity error"),
            "expected arity error, got: {msg}"
        ),
        Ok(_) => panic!("expected arity error for (length 1 2)"),
    }
}

const MUTABLE_NEVER_ASSIGNED: &str = "mutable-binding-never-assigned";
const UNUSED_BINDING: &str = "unused-binding";
const NON_TAIL_SELF_RECURSION: &str = "non-tail-self-recursion";

/// Analyze `source`, run the HIR linter, and return only the diagnostics whose
/// rule matches `rule`.
fn lint_rule(source: &str, rule: &str) -> Vec<crate::lint::diagnostics::Diagnostic> {
    let (mut symbols, mut vm) = setup();
    let analysis = analyze(source, &mut symbols, &mut vm, "<test>").expect("source should analyze");
    let mut linter = HirLinter::new();
    linter.lint(&analysis.hir, &symbols, &analysis.arena);
    linter
        .diagnostics()
        .iter()
        .filter(|d| d.rule == rule)
        .cloned()
        .collect()
}

/// As [`lint_rule`], but through the file front-end, so `source` may hold more
/// than one top-level form (a mutual-recursion pair needs two).
fn lint_file_rule(source: &str, rule: &str) -> Vec<crate::lint::diagnostics::Diagnostic> {
    let (mut symbols, mut vm) = setup();
    let mut cctx = CompileCtx::new();
    let analysis = crate::analyze_file(source, &mut symbols, &mut vm, &mut cctx, "<test>")
        .expect("source should analyze");
    let mut linter = HirLinter::new();
    linter.lint(&analysis.hir, &symbols, &analysis.arena);
    linter
        .diagnostics()
        .iter()
        .filter(|d| d.rule == rule)
        .cloned()
        .collect()
}

/// The binding names a rule flagged, in the order the linter found them.
fn flagged_names(diags: &[crate::lint::diagnostics::Diagnostic]) -> Vec<&str> {
    diags
        .iter()
        .map(|d| {
            let start = d.message.find('\'').expect("message quotes the name") + 1;
            let rest = &d.message[start..];
            &rest[..rest.find('\'').expect("message closes the quote")]
        })
        .collect()
}

// ── W004 unused-binding ──────────────────────────────────────────────

#[test]
fn unused_let_binding_warns() {
    // `x` is bound and never read. This is the typo / dead-refactoring case the
    // rule exists for.
    let diags = lint_file_rule("(defn f [] (let [x 1] 2))\n(f)", UNUSED_BINDING);
    assert_eq!(flagged_names(&diags), ["x"], "got {diags:?}");
    assert_eq!(
        diags[0].severity,
        crate::lint::diagnostics::Severity::Warning
    );
}

#[test]
fn unused_def_binding_warns() {
    // A `def` inside a function body is a letrec binding, and reaches the same
    // checked site as a `let` binding.
    let diags = lint_file_rule("(defn f [] (def x 1) 2)\n(f)", UNUSED_BINDING);
    assert_eq!(flagged_names(&diags), ["x"], "got {diags:?}");
}

#[test]
fn used_binding_no_warning() {
    let diags = lint_file_rule("(defn f [] (let [x 1] x))\n(f)", UNUSED_BINDING);
    assert!(diags.is_empty(), "a read binding must not warn: {diags:?}");
}

#[test]
fn underscore_named_bindings_no_warning() {
    // `_` and `_`-prefixed names are the "I know this is unused" convention, and
    // the compiler's own temporaries (`__destructure_tmp`) share the spelling.
    let diags = lint_file_rule("(defn f [] (let [_ 1 _spare 2] 3))\n(f)", UNUSED_BINDING);
    assert!(diags.is_empty(), "throwaway names must not warn: {diags:?}");
}

#[test]
fn binding_referenced_only_by_its_own_initializer_no_warning() {
    // A self-recursive function references itself from its own body. That is a
    // genuine use — def-use records the self-edge as a capture — so the binding
    // is not unused even when nothing else calls it.
    let diags = lint_file_rule("(defn f [n] (if (= n 0) 0 (f (- n 1))))", UNUSED_BINDING);
    assert!(
        diags.is_empty(),
        "a self-referencing binding is referenced: {diags:?}"
    );
}

#[test]
fn capture_in_nested_closure_counts_as_use() {
    // Counter-factual: were uses collected only from `Var` nodes in the same
    // function, `x` would read as unused here and the rule would flag a binding
    // the closure genuinely needs.
    let diags = lint_file_rule("(defn f [] (let [x 1] (fn [] x)))\n(f)", UNUSED_BINDING);
    assert!(diags.is_empty(), "a captured binding is used: {diags:?}");
}

#[test]
fn destructure_and_primitive_bindings_no_warning() {
    // The destructuring temporary is synthetic, and every primitive is bound
    // into the compilation unit whether the program calls it or not. Neither is
    // the user's dead code.
    let diags = lint_file_rule(
        "(defn f [] (let [(a b) (pair 1 2)] (+ a b)))\n(f)",
        UNUSED_BINDING,
    );
    assert!(
        diags.is_empty(),
        "synthetic and primitive bindings must not warn: {diags:?}"
    );
}

#[test]
fn unused_function_parameter_is_not_reached() {
    // The gap, pinned: `check_binding_site` runs at `let`/`letrec`/`def` sites
    // only, so a lambda parameter never reaches the rule. `x` is unused here and
    // is NOT flagged. Extending the walker to parameters is what would change
    // this assertion.
    let diags = lint_file_rule("(defn f [x] 1)\n(f 1)", UNUSED_BINDING);
    assert!(
        diags.is_empty(),
        "parameters do not reach a checked site: {diags:?}"
    );
}

#[test]
fn mutual_recursion_leaves_both_referenced() {
    let diags = lint_file_rule(
        "(defn a [n] (b n))\n(defn b [n] (a n))\n(a 1)",
        UNUSED_BINDING,
    );
    assert!(
        diags.is_empty(),
        "each function is referenced by the other: {diags:?}"
    );
}

#[test]
fn unused_top_level_definition_warns() {
    let diags = lint_file_rule("(defn used [] 1)\n(defn dead [] 2)\n(used)", UNUSED_BINDING);
    assert_eq!(flagged_names(&diags), ["dead"], "got {diags:?}");
}

#[test]
fn unused_binding_diagnostic_carries_enclosing_function() {
    let diags = lint_file_rule(
        "(defn outer [] (defn inner [] (let [dead 1] 2)) (inner))\n(outer)",
        UNUSED_BINDING,
    );
    assert_eq!(flagged_names(&diags), ["dead"], "got {diags:?}");
    assert_eq!(
        diags[0].function.as_deref(),
        Some("inner"),
        "attribution is to the nearest enclosing named function"
    );
}

// ── W005 non-tail-self-recursion ─────────────────────────────────────

#[test]
fn non_tail_self_call_warns() {
    // The self-call sits under `+`, so each level keeps a frame alive.
    let diags = lint_rule(
        "(defn f [n] (if (= n 0) 0 (+ 1 (f (- n 1)))))",
        NON_TAIL_SELF_RECURSION,
    );
    assert_eq!(diags.len(), 1, "expected one warning, got {diags:?}");
    assert!(
        diags[0].message.contains("f"),
        "message names the function: {}",
        diags[0].message
    );
}

#[test]
fn tail_self_call_no_warning() {
    // The accumulator form: the self-call is the whole `else` branch, so it
    // replaces the frame.
    let diags = lint_rule(
        "(defn f [n acc] (if (= n 0) acc (f (- n 1) (+ acc 1))))",
        NON_TAIL_SELF_RECURSION,
    );
    assert!(
        diags.is_empty(),
        "a tail self-call must not warn: {diags:?}"
    );
}

#[test]
fn self_call_in_both_tail_arms_no_warning() {
    let diags = lint_rule(
        "(defn f [n] (if (= n 0) (f 1) (f 2)))",
        NON_TAIL_SELF_RECURSION,
    );
    assert!(
        diags.is_empty(),
        "both arms of a tail `if` are tail position: {diags:?}"
    );
}

#[test]
fn mutual_recursion_is_out_of_scope() {
    // Two functions that only call each other, both outside tail position.
    // Detecting this needs call-graph reasoning; the rule covers direct
    // self-recursion only and must stay quiet here.
    let diags = lint_file_rule(
        "(defn a [n] (+ 1 (b n)))\n(defn b [n] (+ 1 (a n)))\n(a 1)",
        NON_TAIL_SELF_RECURSION,
    );
    assert!(
        diags.is_empty(),
        "mutual recursion is out of scope: {diags:?}"
    );
}

#[test]
fn letrec_bound_anonymous_fn_self_recursion_warns() {
    // The rule keys on the binding the enclosing lambda was bound to, not on a
    // name in the source, so an anonymous `fn` under `letrec` is covered.
    let diags = lint_rule(
        "(letrec [g (fn [n] (if (= n 0) 0 (+ 1 (g (- n 1)))))] (g 3))",
        NON_TAIL_SELF_RECURSION,
    );
    assert_eq!(diags.len(), 1, "expected one warning, got {diags:?}");
}

#[test]
fn a_sibling_call_is_not_a_self_call() {
    // Counter-factual for keying on the enclosing function rather than on any
    // non-tail call: `g` calls `f` outside tail position and must not warn.
    let diags = lint_file_rule(
        "(defn f [n] n)\n(defn g [n] (+ 1 (f n)))\n(g 1)",
        NON_TAIL_SELF_RECURSION,
    );
    assert!(diags.is_empty(), "a call to another function: {diags:?}");
}

#[test]
fn a_call_from_a_nested_named_closure_is_not_the_outer_self_call() {
    // `h` is bound to a lambda, so the linter's enclosing-function identity is
    // `h` inside it. The `(f 1)` there is not a self-call of `f`.
    let diags = lint_rule(
        "(defn f [n] (let [h (fn [] (+ 1 (f 1)))] (h)))",
        NON_TAIL_SELF_RECURSION,
    );
    assert!(
        diags.is_empty(),
        "the enclosing function inside `h` is `h`: {diags:?}"
    );
}

#[test]
fn mutable_binding_never_assigned_warns() {
    // `count` is declared mutable (var) but only read — never reassigned via
    // `assign`. The binding is a false-mutable and must be flagged.
    let diags = lint_rule("(defn f [] (var count 0) count)", MUTABLE_NEVER_ASSIGNED);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one false-mutable warning, got {diags:?}"
    );
    assert_eq!(
        diags[0].severity,
        crate::lint::diagnostics::Severity::Warning
    );
    assert!(
        diags[0].message.contains("count"),
        "message names the binding: {}",
        diags[0].message
    );
}

#[test]
fn assigned_mutable_binding_no_warning() {
    // `count` is genuinely reassigned — a real mutable binding, not flagged.
    let diags = lint_rule(
        "(defn f [] (var count 0) (assign count 1) count)",
        MUTABLE_NEVER_ASSIGNED,
    );
    assert!(
        diags.is_empty(),
        "assigned binding must not warn: {diags:?}"
    );
}

#[test]
fn immutable_binding_no_warning() {
    // `x` is immutable (let, no `@`) — nothing to recommend.
    let diags = lint_rule("(defn f [] (let [x 1] x))", MUTABLE_NEVER_ASSIGNED);
    assert!(
        diags.is_empty(),
        "immutable binding must not warn: {diags:?}"
    );
}

#[test]
fn loop_binding_no_warning() {
    // A loop variable is rebound via `recur`, not `assign`. It is not a mutable
    // binding in the assign sense and must not be flagged.
    let diags = lint_rule(
        "(defn f [] (loop [i 0] (if (< i 3) (recur (+ i 1)) i)))",
        MUTABLE_NEVER_ASSIGNED,
    );
    assert!(diags.is_empty(), "loop binding must not warn: {diags:?}");
}

#[test]
fn destructure_temporary_no_warning() {
    // The compiler's destructuring temporary (`__destructure_tmp`) and the
    // immutable leaf bindings must not be flagged.
    let diags = lint_rule(
        "(defn f [] (let [(a b) (pair 1 2)] (+ a b)))",
        MUTABLE_NEVER_ASSIGNED,
    );
    assert!(
        diags.is_empty(),
        "destructure temp must not warn: {diags:?}"
    );
}

#[test]
fn immutable_binding_of_mutable_value_no_warning() {
    // The conflation stated positively: `buf` binds a mutable VALUE (a mutable
    // string), but the BINDING is immutable — the binding never changes, so it
    // is not a false-mutable.
    let diags = lint_rule("(defn f [] (let [buf @\"\"] buf))", MUTABLE_NEVER_ASSIGNED);
    assert!(
        diags.is_empty(),
        "immutable binding of a mutable value must not warn: {diags:?}"
    );
}

#[test]
fn false_mutable_diagnostic_carries_enclosing_function() {
    // The advisory is attributed to the nearest enclosing named function so a
    // per-function consumer (the portrait system) can filter by it. A flag in a
    // nested function is attributed to the inner function, not the outer.
    let diags = lint_rule(
        "(defn outer [] (defn inner [] (var n 0) n) (inner))",
        MUTABLE_NEVER_ASSIGNED,
    );
    assert_eq!(diags.len(), 1, "exactly one false-mutable (n): {diags:?}");
    assert_eq!(
        diags[0].function.as_deref(),
        Some("inner"),
        "n is attributed to its enclosing function `inner`, not `outer`"
    );
}

#[test]
fn test_hir_linter_nested_expressions() {
    let (mut symbols, mut vm) = setup();
    let result = analyze(
        "(let [camelCase 1] (if true camelCase 0))",
        &mut symbols,
        &mut vm,
        "<test>",
    );
    assert!(result.is_ok());
    let analysis = result.unwrap();

    let mut linter = HirLinter::new();
    linter.lint(&analysis.hir, &symbols, &analysis.arena);

    // Let bindings don't trigger naming convention checks (only define does)
    assert!(!linter.has_warnings());
}
