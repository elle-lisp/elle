// audited: 2026-10-12
// What an analysis hands a signal solver: each lambda's declarations, and a file analyzed without compiling its imports.
//
// docs/impl/solver.md
// docs/pipeline.md

use super::*;
use elle::hir::{Hir, LambdaDecl};
use elle::signals::{SIG_ERROR, SIG_YIELD};
use elle::value::fiber::SignalBits;
use elle::value::SymbolId;

/// A module file whose compile interns `marker`, a quoted symbol nothing else
/// in this test binary spells. Interning is the only trace a compile of the
/// module leaves, because no test here runs the import.
fn module_with_marker(dir: &std::path::Path, marker: &str) -> String {
    let module = dir.join(format!("{marker}.lisp"));
    std::fs::write(
        &module,
        format!("(fn [] (def marker '{marker}) {{:marker marker}})\n"),
    )
    .expect("write module");
    format!("(def m ((import-file \"{}\")))\nm\n", module.display())
}

#[test]
fn compiling_the_module_interns_its_marker() {
    // The control for the two tests below: an absent marker means the module
    // never compiled only if a compile of the module interns it.
    let tmp = tempfile::tempdir().expect("scratch dir");
    module_with_marker(tmp.path(), "compiled-solver-marker");
    let module = std::fs::read_to_string(tmp.path().join("compiled-solver-marker.lisp"))
        .expect("read module");
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    compile_file(&module, symbols, cctx, "<module>").expect("the module compiles");
    assert!(
        symbols
            .name(SymbolId::of("compiled-solver-marker"))
            .is_some(),
        "a compile of the module interns its quoted marker"
    );
}

#[test]
fn analysis_never_compiles_a_literal_import() {
    // A solver states facts per file and links them, so it must be able to
    // analyze a file whose import graph has a cycle. A compile of each import
    // recurses on such a cycle until the stack overflows.
    let tmp = tempfile::tempdir().expect("scratch dir");
    let source = module_with_marker(tmp.path(), "analyzed-solver-marker");
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let analysis = elle::pipeline::analyze_file(&source, symbols, vm, cctx, "<analyzed>")
        .expect("the importer analyzes");
    assert!(analysis.errors.is_empty(), "{:?}", analysis.errors);
    assert!(
        symbols
            .name(SymbolId::of("analyzed-solver-marker"))
            .is_none(),
        "analyze_file must not compile the imported module, so its quoted \
         marker is never interned"
    );
}

#[test]
fn compiling_a_file_never_compiles_its_literal_import() {
    // The loader compiles the module when the import runs, and this compile
    // runs nothing. The counter-factual is the projection probe, which
    // compiled each literal import while the importer was analyzed.
    let tmp = tempfile::tempdir().expect("scratch dir");
    let source = module_with_marker(tmp.path(), "importer-solver-marker");
    let mut rt = setup();
    let (_, symbols, cctx) = rt.parts();
    compile_file(&source, symbols, cctx, "<importer>").expect("the importer compiles");
    assert!(
        symbols
            .name(SymbolId::of("importer-solver-marker"))
            .is_none(),
        "compile_file must not compile the imported module, so its quoted \
         marker is never interned"
    );
}

/// The declarations of the lambda that `name` is bound to, anywhere in `hir`.
fn decl_of(hir: &Hir, analysis: &elle::pipeline::AnalyzeResult, name: &str) -> Option<LambdaDecl> {
    let lambda_of = |binding: &elle::hir::Binding, value: &Hir| -> Option<LambdaDecl> {
        let named = analysis.arena.get(*binding).name == SymbolId::of(name);
        let lambda = matches!(value.kind, HirKind::Lambda { .. });
        if named && lambda {
            Some(
                *analysis
                    .lambda_decls
                    .get(&value.id)
                    .expect("every lambda has an entry"),
            )
        } else {
            None
        }
    };
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => bindings
            .iter()
            .find_map(|(b, v)| lambda_of(b, v).or_else(|| decl_of(v, analysis, name)))
            .or_else(|| decl_of(body, analysis, name)),
        HirKind::Define { binding, value } => {
            lambda_of(binding, value).or_else(|| decl_of(value, analysis, name))
        }
        HirKind::Begin(exprs) => exprs.iter().find_map(|e| decl_of(e, analysis, name)),
        HirKind::Lambda(lambda) => decl_of(&lambda.body, analysis, name),
        _ => None,
    }
}

#[test]
fn a_lambda_keeps_its_declared_ceiling() {
    // The counter-factual: `quiet` and `plain` infer the same signal, because a
    // ceiling checks the body and the HIR keeps only what the body raises. A
    // solver that read only the HIR could not tell a ceiling from no ceiling,
    // and it could not check its own model against the declaration.
    let source = "(defn quiet [x] (silent!) x)\n\
                  (defn tuned [x] (attune! :yield) x)\n\
                  (defn plain [x] x)\n";
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let analysis = elle::pipeline::analyze_file(source, symbols, vm, cctx, "<decls>")
        .expect("the file analyzes");
    assert!(analysis.errors.is_empty(), "{:?}", analysis.errors);

    let quiet = decl_of(&analysis.hir, &analysis, "quiet").expect("quiet is a lambda");
    assert_eq!(quiet.ceiling.map(|c| c.bits), Some(SignalBits::EMPTY));

    let tuned = decl_of(&analysis.hir, &analysis, "tuned").expect("tuned is a lambda");
    assert_eq!(tuned.ceiling.map(|c| c.bits), Some(SIG_YIELD));

    let plain = decl_of(&analysis.hir, &analysis, "plain").expect("plain is a lambda");
    assert!(plain.ceiling.is_none(), "plain declares no ceiling");
}

#[test]
fn a_lambda_reports_what_it_raises_by_itself() {
    // A solver computes what a call raises from the callee, and needs from
    // the analyzer only what each body raises on its own. The counter-factual
    // reads the inferred signal instead, which also holds what the body's
    // calls raise, so `caller` would report `gen`'s :yield as its own.
    let source = "(defn strict [x] (def [a b] x) a)\n\
                  (defn gen [] (yield 1))\n\
                  (defn caller [] (gen))\n\
                  (defn guarded [f] (bound f ||) 1)\n\
                  (defn maker [] (fn [] (yield 1)))\n";
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let analysis = elle::pipeline::analyze_file(source, symbols, vm, cctx, "<direct>")
        .expect("the file analyzes");
    assert!(analysis.errors.is_empty(), "{:?}", analysis.errors);
    let direct = |name: &str| {
        decl_of(&analysis.hir, &analysis, name)
            .unwrap_or_else(|| panic!("{name} is a lambda"))
            .direct
    };
    assert_eq!(
        direct("strict"),
        SIG_ERROR,
        "strict destructuring may raise"
    );
    assert_eq!(direct("gen"), SIG_YIELD, "an emit is the body's own");
    assert_eq!(
        direct("caller"),
        SignalBits::EMPTY,
        "a call to a known function raises nothing of the caller's own"
    );
    assert_eq!(
        direct("guarded"),
        SIG_ERROR,
        "a bound's entry check may raise"
    );
    assert_eq!(
        direct("maker"),
        SignalBits::EMPTY,
        "a nested lambda's emit is the nested lambda's own"
    );
}

#[test]
fn the_top_level_reports_what_it_raises_by_itself() {
    // The code outside every lambda is a context of its own for a solver.
    // The counter-factual leaves it with nothing, so a strict destructure at
    // the top level would raise nothing the solver can see.
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let strict = elle::pipeline::analyze_file("(def [a b] [1 2])\na\n", symbols, vm, cctx, "<top>")
        .expect("the file analyzes");
    assert!(strict.errors.is_empty(), "{:?}", strict.errors);
    assert_eq!(strict.top_direct, SIG_ERROR, "the destructure may raise");

    let (vm, symbols, cctx) = rt.parts();
    let plain =
        elle::pipeline::analyze_file("(defn f [] (yield 1))\nf\n", symbols, vm, cctx, "<top>")
            .expect("the file analyzes");
    assert!(plain.errors.is_empty(), "{:?}", plain.errors);
    assert_eq!(
        plain.top_direct,
        SignalBits::EMPTY,
        "the lambda's emit is the lambda's own"
    );
}
