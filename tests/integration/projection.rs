// audited: 2026-10-06
// A module file's signal projection, the import-file probe that reads it, and compile-time squelch.
//
// docs/signals/inference.md
// docs/modules.md
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
use elle::value::SymbolId;
use elle::vm::VM;

fn setup() -> (SymbolTable, VM) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _signals = register_primitives(&mut vm, &mut symbols);
    (symbols, vm)
}

// Local `analyze_file` shim. These tests analyze one module file alone (no
// stdlib, no execution), so each call takes a fresh `CompileCtx` (primitives,
// core and prelude), and no projection or compile-time state is shared across
// calls.
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
fn a_file_returning_a_lambda_over_a_struct_projects_its_fields() {
    // The closure-as-module convention returns a lambda whose body ends in the
    // export struct, and the projection reads through the lambda to the struct.
    // `test_projection_bytecode_field` below holds the bare struct literal.
    let source = r#"
(defn add [x y] (numeric!) (%add x y))
(defn double [x] (numeric!) (%mul x 2))
(fn [] {:add add :double double})
"#;
    let mut symbols = SymbolTable::new();
    let result = compile_file(source, &mut symbols, "<test>").unwrap();
    let proj = result
        .bytecode
        .signal_projection
        .expect("a lambda over a struct literal has a projection");
    assert!(proj.contains_key("add"), "projection should contain :add");
    assert!(
        proj.contains_key("double"),
        "projection should contain :double"
    );
}

// ============================================================================
// 3. COMPILE-TIME SQUELCH DETECTION
// ============================================================================

#[test]
fn test_squelch_binding_signal_inference() {
    // A binding whose value is a compile-time squelch analyzes. The squelch
    // algebra itself is pinned by the `Signal::squelch` tests above.
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
    // A squelch whose mask is a set literal analyzes.
    let source = r#"
(defn f [] (yield 1))
(def safe (squelch f |:yield :io|))
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
// 4. THE PROJECTION ON THE BYTECODE
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
    assert!(!proj["add"].may_suspend(), ":add should not be suspending");
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
    assert!(proj["producer"].may_yield(), ":producer should be yields");
}

// ============================================================================
// 5. THE IMPORT PROJECTION PROBE
// ============================================================================

#[test]
fn import_projection_probe_compiles_the_imported_module() {
    // `((import-file "…"))` makes the analyzer compile the imported file to read
    // its signal projection (src/hir/analyze/call.rs). `compile_file` never
    // executes the import, so the probe is the only thing that reaches the
    // module's source at all.
    //
    // The trap: `SymbolId::of` derives an id without recording anything, so the
    // name answers here only if some compile interned it. The module's marker is
    // a quoted symbol — runtime data, which compiling must intern — unlike a
    // binding name, which analysis resolves into the binding arena instead. The
    // spelling has to stay unique across this test binary: the registry is
    // process-global, so any other test interning it would answer here too.
    let tmp = tempfile::tempdir().expect("scratch dir");
    let dir = tmp.path();
    let module = dir.join("probe_module.lisp");
    std::fs::write(
        &module,
        "(fn [] (def marker 'probe-only-marker) {:marker marker})\n",
    )
    .expect("write module");

    let mut symbols = SymbolTable::new();
    let source = format!("(def m ((import-file \"{}\")))\nm\n", module.display());
    compile_file(&source, &mut symbols, "<probe-main>").expect("main file compiles");

    assert!(
        symbols.name(SymbolId::of("probe-only-marker")).is_some(),
        "compiling a file whose import is probed must compile the module: with \
         no probe nothing reads the module's source, so its quoted data is never \
         interned and the import falls back to the conservative projection"
    );
}

#[test]
fn the_analyzer_reads_no_projection_through_the_import_macro() {
    // `import` is a macro over `import/resolve`, which a program may replace, so
    // the compiler does not know its file and must not compile one. The
    // counter-factual: an analyzer that still probes a literal `import` spec
    // compiles the module, and the module's quoted marker is interned. The
    // marker spelling is unique for the reason the probe test above gives.
    //
    // `import/resolve` lives in the stdlib, so this runtime loads it.
    let tmp = tempfile::tempdir().expect("scratch dir");
    let module = tmp.path().join("unprobed_module.lisp");
    std::fs::write(
        &module,
        "(fn [] (def marker 'import-macro-unprobed-marker) {:marker marker})\n",
    )
    .expect("write module");

    let mut rt = elle::runtime::Runtime::new();
    let (_, symbols, cctx) = rt.parts();
    let main = format!("(def m ((import \"{}\")))\nm\n", module.display());
    elle::pipeline::compile_file(&main, symbols, cctx, "<unprobed-main>")
        .expect("main file compiles");

    assert!(
        symbols
            .name(SymbolId::of("import-macro-unprobed-marker"))
            .is_none(),
        "compiling a file that imports through the `import` macro must not \
         compile the module: the compiler knows the file of a literal \
         `import-file` alone"
    );
}

#[test]
fn each_keeps_its_collection_when_an_import_probe_expanded_the_macro_first() {
    // The trap: `each` decides whether it was written in `in` form with
    // `(= (syntax->datum iter-or-in) 'in)` (src/prelude.lisp § each). That `'in`
    // is a quoted literal in the transformer's own bytecode, and a transformer
    // compiles once, lazily, on its first expansion, then caches on the shared
    // expander (`ensure_transformer`, src/syntax/expand/macro_expand.rs). So
    // whichever compile expands `each` first fixes the literal that every later
    // expansion compares against — including a compile the program never asked
    // for.
    //
    // Here the import projection probe is that first compile: this runtime loads
    // no stdlib, so nothing has warmed the transformer, and compiling the main
    // file runs the probe over a module that uses `each`. The probe compiles in
    // its own symbol table (`CompileCtx::get_or_compile_projection`), so this is
    // also what pins that the two tables agree. The second compile, on the same
    // context, is the one that must still read its `in`.
    //
    // The counter-factual: the assertion names the elements the loop visited
    // rather than settling for a successful compile. When the comparison against
    // `'in` fails, `each` takes the symbol `in` itself as the collection and
    // shifts `'(1 2 3)` into the body — which raises `undefined variable: in`
    // only where `in` is unbound, and is silent everywhere else.
    //
    // Both files define `pair?` themselves: `each`'s `:list` arm calls it, it is
    // the one name in the expansion that is not a primitive, and no stdlib is
    // loaded here. Prelude template symbols carry `ScopeId(0)`, so this
    // file-scope definition is what the expansion resolves.
    const PAIR_P: &str = "(defn pair? [x] (%pair? x))\n";

    let tmp = tempfile::tempdir().expect("scratch dir");
    let module = tmp.path().join("each_module.lisp");
    std::fs::write(
        &module,
        format!(
            "{PAIR_P}\
             (fn []\n  \
             {{:visit (fn [xs]\n    \
             (def @seen ())\n    \
             (each x in xs (assign seen (%pair x seen)))\n    \
             seen)}})\n"
        ),
    )
    .expect("write module");

    let mut rt = elle::runtime::Runtime::without_stdlib();
    let (vm, symbols, cctx) = rt.parts();

    let main = format!("(def m ((import-file \"{}\")))\nm\n", module.display());
    elle::pipeline::compile_file(&main, symbols, cctx, "<each-probe-main>")
        .expect("main file compiles");

    let visited = elle::eval_all(
        &format!(
            "{PAIR_P}\
             (def @seen ())\n\
             (each x in '(1 2 3) (assign seen (%pair x seen)))\n\
             seen\n"
        ),
        symbols,
        vm,
        cctx,
        "<each-after-probe>",
    );

    assert_eq!(
        visited.as_ref().map(|v| v.to_string()).as_deref(),
        Ok("(3 2 1)"),
        "`each` must still read its `in` form after the import projection probe \
         expanded the macro; when the cached transformer's quoted `'in` stops \
         matching the one at the use site, the collection is dropped from the \
         loop and the body iterates over the symbol `in`"
    );
}
