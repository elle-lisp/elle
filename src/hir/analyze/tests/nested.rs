// audited: 2026-11-09
//! A `def` nested in a form that opens no scope binds in the body around it, and one the body may skip binds a `var` set to nil first.
//!
//! docs/bindings.md

use super::*;
use crate::pipeline::AnalyzeResult;

/// `source` analyzed as a file, as the linter and `compile/analyze` see it.
fn analyze_file(source: &str) -> AnalyzeResult {
    let mut rt = crate::runtime::Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    crate::pipeline::analyze_file(source, symbols, vm, cctx, "<nested>")
        .unwrap_or_else(|e| panic!("{source} did not analyze: {e}"))
}

/// Every node of `hir`, outermost first.
fn nodes(hir: &Hir) -> Vec<&Hir> {
    fn collect<'a>(hir: &'a Hir, out: &mut Vec<&'a Hir>) {
        out.push(hir);
        hir.for_each_child(|child| collect(child, out));
    }
    let mut out = Vec::new();
    collect(hir, &mut out);
    out
}

/// The file's letrec entries.
fn file_entries(result: &AnalyzeResult) -> &[(Binding, Hir)] {
    let HirKind::Letrec { bindings, .. } = &result.hir.kind else {
        panic!("a file analyzes to a letrec, got {:?}", result.hir.kind);
    };
    bindings
}

/// Whether `binding` is spelled `name`.
fn named(result: &AnalyzeResult, binding: Binding, name: &str) -> bool {
    result.arena.get(binding).name == SymbolId::of(name)
}

/// The file entry named `name` whose initializer is the integer `n`.
fn entry_of_int(result: &AnalyzeResult, name: &str, n: i64) -> Binding {
    file_entries(result)
        .iter()
        .find(|(b, init)| named(result, *b, name) && matches!(init.kind, HirKind::Int(v) if v == n))
        .map(|(b, _)| *b)
        .unwrap_or_else(|| panic!("no entry {name} = {n}"))
}

/// The bindings every `Var` under `hir` reads.
fn reads(hir: &Hir) -> Vec<Binding> {
    nodes(hir)
        .into_iter()
        .filter_map(|n| match n.kind {
            HirKind::Var(b) => Some(b),
            _ => None,
        })
        .collect()
}

// Counter-factual: the nested def bound a second `num` while the analyzer
// walked the `when`, and the read after `(def num 7)` resolved to it.
#[test]
fn a_read_after_a_skipped_nested_def_resolves_to_the_later_def() {
    let result = analyze_file("(when false (def num 1)) (def num 7) num");
    let seven = entry_of_int(&result, "num", 7);
    let (_, last) = file_entries(&result).last().expect("the file has entries");
    let HirKind::Var(read) = last.kind else {
        panic!("the last form reads num, got {:?}", last.kind);
    };
    assert_eq!(read, seven, "the read resolves to (def num 7)");
}

// Counter-factual: the fixpoint re-analyzed every lambda against the scope
// after the first pass, which held the first `again` alone.
#[test]
fn a_lambda_after_a_redefinition_reads_the_redefinition() {
    let result = analyze_file("(def again 1) (def again 2) (defn read [] again)");
    let first = entry_of_int(&result, "again", 1);
    let second = entry_of_int(&result, "again", 2);
    let (_, read) = file_entries(&result)
        .iter()
        .find(|(b, _)| named(&result, *b, "read"))
        .expect("the file defines read");
    let seen = reads(read);
    assert!(seen.contains(&second), "read reads the second again");
    assert!(!seen.contains(&first), "read does not read the first again");
}

// Counter-factual: the file classified `(begin (def a 1) (def b 2))` as one
// expression, and its defs bound in the middle of analyzing it.
#[test]
fn a_top_level_begin_splices_its_defs_into_the_file() {
    let result = analyze_file("(begin (def a 1) (def b 2))");
    entry_of_int(&result, "a", 1);
    entry_of_int(&result, "b", 2);
}

// Counter-factual: a function body pre-bound the defs it held directly, so a
// lambda before the begin could not resolve `h`.
#[test]
fn a_def_in_a_begin_in_a_function_body_is_prebound() {
    let result = analyze_file("(defn f [] (defn g [] (h)) (begin (defn h [] 1)) (g))");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}

// Counter-factual: the skipped def was a `Define` of the literal, which the
// lowerer then read at every later use, the run where it never ran included.
#[test]
fn a_skippable_def_sets_a_nil_var_and_assigns_it() {
    let result = analyze_file("(defn f [c] (when c (def x 1)) x)");
    let all = nodes(&result.hir);
    let x = all
        .iter()
        .find_map(|n| match &n.kind {
            HirKind::Define { binding, value }
                if named(&result, *binding, "x") && matches!(value.kind, HirKind::Nil) =>
            {
                Some(*binding)
            }
            _ => None,
        })
        .expect("the body sets x to nil first");
    assert!(
        all.iter().any(|n| matches!(&n.kind,
            HirKind::Assign { target, value } if *target == x && matches!(value.kind, HirKind::Int(1)))),
        "the def assigns x"
    );
    assert!(
        !all.iter().any(|n| matches!(&n.kind,
            HirKind::Define { binding, value } if *binding == x && matches!(value.kind, HirKind::Int(1)))),
        "no Define binds x to the literal"
    );
    let inner = result.arena.get(x);
    assert!(inner.is_mutated && !inner.is_immutable, "x is a var");
}

// A def every path reaches keeps its Define, so its literal still folds.
#[test]
fn a_def_in_a_begin_stays_a_define() {
    let result = analyze_file("(defn f [] (begin (def x 1)) x)");
    assert!(nodes(&result.hir).iter().any(|n| matches!(&n.kind,
        HirKind::Define { binding, value }
            if named(&result, *binding, "x") && matches!(value.kind, HirKind::Int(1)))));
}
