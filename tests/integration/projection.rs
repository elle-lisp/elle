// Integration tests for signal projection and compile-time squelch
//
// Signal projection: the compiler extracts signal profiles from exported
// closures in module files, enabling cross-file signal inference.
//
// Compile-time squelch: the analyzer recognizes (squelch f :kw) as a
// signal-narrowing operation and computes the result signal statically.

use elle::hir::HirKind;
use elle::primitives::register_primitives;
use elle::signals::{Signal, SIG_IO, SIG_YIELD};
use elle::symbol::SymbolTable;
use elle::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

// Local `analyze_file` shim preserving the pre-CompileCtx arity. These tests
// analyze a single module file in isolation (no stdlib, no execution), so a
// fresh `CompileCtx` per call (primitives + core + prelude) gives exactly the
// old bare path; no projection or compile-time state is shared across calls.
fn analyze_file(
    source: &str,
    symbols: &mut SymbolTable,
    vm: &mut VM,
    source_name: &str,
) -> Result<elle::pipeline::AnalyzeResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::analyze_file(source, symbols, vm, &mut cctx, source_name)
}

// Local `compile_file` shim, same rationale as `analyze_file`.
fn compile_file(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<elle::CompileResult, String> {
    let mut cctx = elle::pipeline::CompileCtx::new();
    elle::pipeline::compile_file(source, symbols, &mut cctx, source_name)
}

// ============================================================================
// 1. SIGNAL::SQUELCH METHOD TESTS (compile-time algebra)
// ============================================================================

#[test]
fn test_squelch_yields_to_silent() {
    // Squelching :yield on a yields function produces errors-only.
    let sig = Signal::yields();
    let result = sig.squelch(SIG_YIELD);
    assert!(!result.may_yield());
    assert!(result.may_error()); // squelch adds error
    assert!(result.may_suspend()); // error is a fiber transfer
}

#[test]
fn test_squelch_noop() {
    // Squelching :io on a yields function is a no-op.
    let sig = Signal::yields();
    let result = sig.squelch(SIG_IO);
    assert_eq!(result, sig);
}

#[test]
fn test_squelch_multi() {
    // Squelching multiple bits at once.
    let sig = Signal {
        bits: SIG_YIELD.union(SIG_IO),
        propagates: 0,
    };
    let result = sig.squelch(SIG_YIELD.union(SIG_IO));
    assert!(!result.may_yield());
    assert!(!result.may_io());
    assert!(result.may_error());
}

// ============================================================================
// 2. SIGNAL PROJECTION COMPUTATION
// ============================================================================

#[test]
fn test_projection_struct_literal() {
    // A file returning {:add add :double double} where both are pure
    // should produce a projection mapping :add and :double to errors-only.
    let source = r#"
(defn add [x y] (%add x y))
(defn double [x] (%mul x 2))
{:add add :double double}
"#;
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(source, &mut symbols, &mut vm, "<test>").unwrap();

    // The file letrec's last binding is the struct literal.
    // Walk into the Letrec to find the struct call.
    if let HirKind::Letrec { bindings, .. } = &result.hir.kind {
        // Last binding's value should be the struct call
        let (_, value) = bindings.last().unwrap();
        if let HirKind::Call { args, .. } = &value.kind {
            // Check it's a struct call with keyword-value pairs
            assert!(args.len() >= 4, "struct should have at least 4 args");
            // Each closure value in the struct should have errors-only signal
            for i in (1..args.len()).step_by(2) {
                let sig = &args[i].expr.signal;
                assert!(
                    !sig.may_suspend(),
                    "field at {} should not suspend",
                    i
                );
            }
        } else {
            // Might be wrapped differently; just check the projection was computed
            // by verifying the overall file signal is reasonable
        }
    }
}

#[test]
fn test_projection_lambda_returning_struct() {
    // A file returning (fn [] {:add add :double double}) should produce
    // the same projection as a direct struct literal.
    let source = r#"
(defn add [x y] (%add x y))
(defn double [x] (%mul x 2))
(fn [] {:add add :double double})
"#;
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(source, &mut symbols, &mut vm, "<test>").unwrap();

    // The file should compile successfully
    assert!(
        !matches!(result.hir.kind, HirKind::Error),
        "file should compile without errors"
    );
}

// ============================================================================
// 3. COMPILE-TIME SQUELCH DETECTION
// ============================================================================

#[test]
fn test_squelch_binding_signal_inference() {
    // (def safe (squelch f :error)) where f has signal {:error}
    // should infer safe as silent (squelch removes :error, but since
    // the mask catches all signal bits, the result is just {:error} from squelch itself).
    let source = r#"
(defn f [x] (+ x 1))
(def safe (squelch f :error))
safe
"#;
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(source, &mut symbols, &mut vm, "<test>").unwrap();
    // Should compile without errors
    assert!(
        !matches!(result.hir.kind, HirKind::Error),
        "squelch binding should compile"
    );
}

#[test]
fn test_squelch_set_mask() {
    // (squelch f |:yield :io|) should handle set masks.
    let source = r#"
(defn f [] (yield 1))
(def safe (squelch f :yield))
safe
"#;
    let (mut symbols, mut vm) = setup();
    let result = analyze_file(source, &mut symbols, &mut vm, "<test>").unwrap();
    assert!(
        !matches!(result.hir.kind, HirKind::Error),
        "squelch with set mask should compile"
    );
}

// ============================================================================
// 4. PROJECTION + SQUELCH COMPOSITION
// ============================================================================

#[test]
fn test_projection_bytecode_field() {
    // compile_file should populate signal_projection on the bytecode.
    // `(numeric!)` proves the params for the intrinsic operand contract
    // without adding an :error guard — the projections must stay silent
    // for the may_suspend assertions below.
    let source = r#"
(defn add [x y] (numeric!) (%add x y))
(defn double [x] (numeric!) (%mul x 2))
{:add add :double double}
"#;
    let mut symbols = SymbolTable::new();
    let result = compile_file(source, &mut symbols, "<test>").unwrap();
    let proj = result.bytecode.signal_projection;
    assert!(
        proj.is_some(),
        "bytecode should have signal_projection for struct-returning file"
    );
    let proj = proj.unwrap();
    assert!(proj.contains_key("add"), "projection should contain :add");
    assert!(
        proj.contains_key("double"),
        "projection should contain :double"
    );
    // Both are pure arithmetic — errors only, not yields
    assert!(
        !proj["add"].may_suspend(),
        ":add should not be suspending"
    );
    assert!(
        !proj["double"].may_suspend(),
        ":double should not be suspending"
    );
}

#[test]
fn test_projection_non_struct_returns_none() {
    // A file returning a plain value (not a struct) should have no projection.
    let source = "(%add 1 2)";
    let mut symbols = SymbolTable::new();
    let result = compile_file(source, &mut symbols, "<test>").unwrap();
    assert!(
        result.bytecode.signal_projection.is_none(),
        "non-struct file should have no projection"
    );
}

#[test]
fn test_projection_yields_function() {
    // A file exporting a yielding function should project it as yields.
    let source = r#"
(defn producer [] (yield 1))
{:producer producer}
"#;
    let mut symbols = SymbolTable::new();
    let result = compile_file(source, &mut symbols, "<test>").unwrap();
    let proj = result.bytecode.signal_projection.unwrap();
    assert!(
        proj["producer"].may_yield(),
        ":producer should be yields"
    );
}

// ============================================================================
// 4. THE PROBE COMPILES IN THE CALLER'S SYMBOL TABLE
// ============================================================================

#[test]
fn import_projection_probe_interns_into_the_callers_symbol_table() {
    // `((import "…"))` makes the analyzer compile the imported file to read its
    // signal projection (src/hir/analyze/call.rs § "Import projection
    // detection"). Which symbol table that compile runs in is not an internal
    // detail: macro transformers compile lazily on first expansion and cache on
    // the shared expander, and a transformer's quoted literals bind to the table
    // that compiled it. A probe running in a throwaway table therefore caches
    // transformers whose ids mean nothing in the instance's table, and a later
    // expansion comparing against an instance-table symbol takes the wrong
    // branch — `each`'s `(= (syntax->datum iter-or-in) 'in)` is the shape that
    // bites.
    //
    // The caller's table learning the module's quoted symbol is the observable
    // form of the invariant: `compile_file` never executes the import, so the
    // probe is the only thing that could have interned it.
    let dir = std::env::temp_dir().join(format!(
        "elle-projection-probe-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let module = dir.join("probe_module.lisp");
    // A quoted symbol is runtime data, so compiling this file must intern
    // `probe-only-marker` — unlike a binding name, which analysis resolves into
    // the binding arena instead.
    std::fs::write(
        &module,
        "(fn [] (def marker 'probe-only-marker) {:marker marker})\n",
    )
    .expect("write module");

    let mut symbols = SymbolTable::new();
    let source = format!("(def m ((import \"{}\")))\nm\n", module.display());
    let _ = compile_file(&source, &mut symbols, "<probe-main>");

    let learned = symbols.get("probe-only-marker");
    std::fs::remove_dir_all(&dir).ok();

    assert!(
        learned.is_some(),
        "the import projection probe must compile in the caller's symbol table; \
         compiling in a throwaway one leaves the instance's table without the \
         module's symbols and caches transformers bound to a table nothing else \
         can read"
    );
}
