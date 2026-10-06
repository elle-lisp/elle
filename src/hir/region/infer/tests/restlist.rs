// audited: 2026-10-06
//! The rest-list gate: which lambdas build their rest list in one region.
//!
//! docs/impl/region/restlist.md
//!
//! Each test binds one lambda to `h` and asks the region analysis, run with
//! the real primitive classification, for that lambda's rest-list layout. The
//! wrapping letrec of `analyze_with_class` supplies `g`, a closure.
use super::*;
use crate::value::{RestListLayout, SymbolId};

/// The `HirId` of the lambda a `let` or `letrec` binds to `name`.
fn lambda_bound_to(hir: &Hir, arena: &BindingArena, name: &str) -> HirId {
    fn walk(hir: &Hir, arena: &BindingArena, want: SymbolId) -> Option<HirId> {
        if let HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } = &hir.kind {
            for (b, init) in bindings {
                if arena.get(*b).name == want && matches!(init.kind, HirKind::Lambda { .. }) {
                    return Some(init.id);
                }
            }
        }
        let mut found = None;
        hir.for_each_child(|c| {
            if found.is_none() {
                found = walk(c, arena, want);
            }
        });
        found
    }
    walk(hir, arena, SymbolId::of(name)).unwrap_or_else(|| panic!("no lambda bound to {name}"))
}

/// The layout the analysis gives the rest list of the lambda bound to `h`.
fn layout_of_h(source: &str) -> RestListLayout {
    let (hir, arena, _symbols, info) = analyze_with_class(source);
    info.rest_list_layout(lambda_bound_to(&hir, &arena, "h"))
}

fn assert_one_region(body: &str) {
    let source = format!("(let [h (fn [& xs] {body})] (h 1 2 3))");
    assert_eq!(
        layout_of_h(&source),
        RestListLayout::OneRegion,
        "a callee whose body is `{body}` only reads its rest list"
    );
}

fn assert_per_cell(source: &str, why: &str) {
    assert_eq!(layout_of_h(source), RestListLayout::PerCell, "{why}");
}

// ── Admitted: every reference reads the list and keeps no cell ──────

#[test]
fn an_unused_rest_list_takes_one_region() {
    assert_one_region("0");
}

#[test]
fn an_immediate_primitive_reads_the_list() {
    assert_one_region("(length xs)");
    assert_one_region("(empty? xs)");
    assert_one_region("(= xs xs)");
}

#[test]
fn first_and_second_answer_an_element() {
    assert_one_region("(first xs)");
    assert_one_region("(second xs)");
}

#[test]
fn an_intrinsic_that_needs_no_proof_reads_the_list() {
    assert_one_region("(%type-of xs)");
    assert_one_region("(%eq xs xs)");
    assert_one_region("(%ne xs xs)");
    assert_one_region("(%identical? xs xs)");
    assert_one_region("(%not xs)");
    assert_one_region("(%pair? xs)");
}

#[test]
fn a_spliced_argument_hands_over_the_elements() {
    assert_one_region("(string ;xs)");
    assert_one_region("(apply string xs)");
}

#[test]
fn several_admitted_reads_on_several_paths() {
    assert_one_region("(if (empty? xs) 0 (first xs))");
}

// ── Refused: some reference could keep a cell past the head ─────────

#[test]
fn a_returned_list_keeps_one_region_per_cell() {
    assert_per_cell(
        "(let [h (fn [& xs] xs)] (h 1 2 3))",
        "the caller may keep a tail",
    );
}

#[test]
fn rest_answers_a_tail() {
    assert_per_cell(
        "(let [h (fn [& xs] (rest xs))] (h 1 2 3))",
        "rest hands out a tail that outlives the head",
    );
}

#[test]
fn a_primitive_that_is_not_admitted_refuses_the_list() {
    assert_per_cell(
        "(let [h (fn [& xs] (->array xs))] (h 1 2 3))",
        "->array is Opaque: its result may be the argument itself",
    );
}

#[test]
fn a_closure_call_refuses_the_list() {
    assert_per_cell(
        "(let [h (fn [& xs] (g xs))] (h 1 2 3))",
        "a closure may take a tail and keep it",
    );
}

#[test]
fn an_alias_refuses_the_list() {
    assert_per_cell(
        "(let [h (fn [& xs] (let [ys xs] (length ys)))] (h 1 2 3))",
        "a second name for the list is a reference the gate does not follow",
    );
}

#[test]
fn a_capture_refuses_the_list() {
    assert_per_cell(
        "(let [h (fn [& xs] ((fn [] (length xs))))] (h 1 2 3))",
        "a nested lambda holds the list beyond the gate's sight",
    );
}

#[test]
fn a_store_refuses_the_list() {
    assert_per_cell(
        "(let [h (fn [& xs] (%pair 1 xs))] (h 1 2 3))",
        "%pair stores the list into a fresh cell",
    );
}

#[test]
fn a_destructure_refuses_the_list() {
    assert_per_cell(
        "(let [h (fn [& xs] (let [[a b] xs] a))] (h 1 2 3))",
        "a pattern reads the list's cells",
    );
}

#[test]
fn a_reassigned_rest_parameter_refuses_the_list() {
    assert_per_cell(
        "(let [h (fn [& @xs] (assign xs (rest xs)) 0)] (h 1 2 3))",
        "the parameter comes to hold a tail",
    );
}

#[test]
fn a_shadowing_closure_is_not_the_primitive() {
    assert_per_cell(
        "(let [length (fn [l] (rest l))
               h (fn [& xs] (length xs))]
           (h 1 2 3))",
        "a closure named length is a closure, whatever its name",
    );
}

#[test]
fn a_keyword_collector_is_never_a_list() {
    assert_per_cell(
        "(let [h (fn [&keys opts] (length opts))] (h :a 1))",
        "&keys collects a struct, which the gate does not lay out",
    );
}
