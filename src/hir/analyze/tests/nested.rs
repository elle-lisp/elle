// audited: 2026-11-09
//! A `def` binds in its scope however deep it sits, a redefinition is a new binding, and a read after a skippable def resolves past it.
//!
//! docs/bindings.md

use super::*;
use crate::pipeline::AnalyzeResult;

/// `source` analyzed as a file, as the linter and `compile/analyze` see it.
pub(super) fn analyze_file(source: &str) -> AnalyzeResult {
    let mut rt = crate::runtime::Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    crate::pipeline::analyze_file(source, symbols, vm, cctx, "<nested>")
        .unwrap_or_else(|e| panic!("{source} did not analyze: {e}"))
}

/// Every node of `hir`, outermost first.
pub(super) fn nodes(hir: &Hir) -> Vec<&Hir> {
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

/// The expression a body's value comes from: the last form, through `Begin`.
fn last_expr(hir: &Hir) -> &Hir {
    match &hir.kind {
        HirKind::Begin(exprs) if !exprs.is_empty() => last_expr(&exprs[exprs.len() - 1]),
        _ => hir,
    }
}

/// The body of the lambda the file binds to `name`.
pub(super) fn body_of<'r>(result: &'r AnalyzeResult, name: &str) -> &'r Hir {
    file_entries(result)
        .iter()
        .find_map(|(b, init)| match &init.kind {
            HirKind::Lambda(lambda) if named(result, *b, name) => Some(&lambda.body),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the file defines the function {name}"))
}

/// The binding a `Define` of `name` to the integer `n` makes under `hir`.
fn define_of_int(result: &AnalyzeResult, hir: &Hir, name: &str, n: i64) -> Binding {
    nodes(hir)
        .into_iter()
        .find_map(|node| match &node.kind {
            HirKind::Define { binding, value }
                if named(result, *binding, name)
                    && matches!(value.kind, HirKind::Int(v) if v == n) =>
            {
                Some(*binding)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no Define of {name} to {n}"))
}

// Counter-factual: the read after the `when` resolved to the def's own
// binding, and the lowerer read the def's literal there, the run where the def
// never ran included.
#[test]
fn a_read_after_a_skippable_def_resolves_past_its_binding() {
    let result = analyze_file("(defn f [c] (when c (def x 1)) x)");
    let body = body_of(&result, "f");
    let def = define_of_int(&result, body, "x", 1);
    let HirKind::Var(read) = last_expr(body).kind else {
        panic!("f's value is a read of x, got {:?}", last_expr(body).kind);
    };
    assert_ne!(
        read, def,
        "the read after the when is not the def's binding"
    );
    assert!(named(&result, read, "x"), "the read still names x");
}

// A def in a branch keeps its Define, and its binding is never assigned, so
// the facts the def establishes hold wherever the binding is read.
#[test]
fn a_skippable_def_keeps_its_define() {
    let result = analyze_file("(defn f [c] (when c (def x 1) x))");
    let body = body_of(&result, "f");
    let def = define_of_int(&result, body, "x", 1);
    assert!(
        !result.arena.get(def).is_mutated,
        "the def's binding is never assigned"
    );
}

// Counter-factual: a function body rejected a second def of a name, and a
// lambda between the two would have read the second.
#[test]
fn a_lambda_before_a_body_redefinition_reads_the_first() {
    let result = analyze_file("(defn f [] (def x 1) (def p (fn [] x)) (def x 2) [x (p)])");
    let body = body_of(&result, "f");
    let first = define_of_int(&result, body, "x", 1);
    let second = define_of_int(&result, body, "x", 2);
    let p = nodes(body)
        .into_iter()
        .find_map(|node| match &node.kind {
            HirKind::Define { binding, value } if named(&result, *binding, "p") => {
                Some(value.as_ref())
            }
            _ => None,
        })
        .expect("f defines p");
    let seen = reads(p);
    assert!(seen.contains(&first), "p reads the first x");
    assert!(!seen.contains(&second), "p does not read the second x");
}

// A def every path reaches keeps its Define, so its literal still folds.
#[test]
fn a_def_in_a_begin_stays_a_define() {
    let result = analyze_file("(defn f [] (begin (def x 1)) x)");
    assert!(nodes(&result.hir).iter().any(|n| matches!(&n.kind,
        HirKind::Define { binding, value }
            if named(&result, *binding, "x") && matches!(value.kind, HirKind::Int(1)))));
}
