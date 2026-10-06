// audited: 2026-10-06
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
        HirKind::Lambda { body, .. } => decl_of(body, analysis, name),
        _ => None,
    }
}

#[test]
fn a_lambda_keeps_its_declared_ceiling_and_muffle() {
    // The counter-factual: `quiet` infers silent whether or not its body could
    // raise, because its ceiling replaces what the body raises. A solver that
    // read only the HIR's inferred signal could not tell a ceiling from a body
    // that raises nothing, and it would let a raise past the ceiling go
    // unreported.
    let source = "(defn quiet [x] (silence) (muffle :error) (%add x 1))\n\
                  (defn tuned [x] (attune! :yield) x)\n\
                  (defn plain [x] x)\n";
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let analysis = elle::pipeline::analyze_file(source, symbols, vm, cctx, "<decls>")
        .expect("the file analyzes");
    assert!(analysis.errors.is_empty(), "{:?}", analysis.errors);

    let quiet = decl_of(&analysis.hir, &analysis, "quiet").expect("quiet is a lambda");
    assert_eq!(quiet.ceiling.map(|c| c.bits), Some(SignalBits::EMPTY));
    assert_eq!(quiet.muffle, SIG_ERROR);

    let tuned = decl_of(&analysis.hir, &analysis, "tuned").expect("tuned is a lambda");
    assert_eq!(tuned.ceiling.map(|c| c.bits), Some(SIG_YIELD));
    assert_eq!(tuned.muffle, SignalBits::EMPTY);

    let plain = decl_of(&analysis.hir, &analysis, "plain").expect("plain is a lambda");
    assert!(plain.ceiling.is_none(), "plain declares no ceiling");
    assert_eq!(plain.muffle, SignalBits::EMPTY);
}
