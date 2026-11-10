// audited: 2026-11-10
// src/hir/AGENTS.md
// docs/impl/graft/captures.md
//! The capture a graft adds: a body of this unit grafted inside a lambda names
//! module bindings through captures the graft appends to that lambda and to
//! every lambda between it and the file.
//!
//! These compile against a booted runtime, because the stdlib source compiled
//! as one unit is among the subjects.

use super::{compiled, count_calls_to, count_emits, count_intrinsics, run_booted};
use crate::hir::binding::CaptureKind;
use crate::hir::{BindingArena, Hir, HirKind, Lambda};
use crate::value::{SymbolId, Value};

/// Every lambda in `hir` whose parameters include one spelled `param`.
fn lambdas_with_param<'h>(hir: &'h Hir, arena: &BindingArena, param: &str) -> Vec<&'h Lambda> {
    fn walk<'h>(h: &'h Hir, arena: &BindingArena, param: SymbolId, out: &mut Vec<&'h Lambda>) {
        if let HirKind::Lambda(lambda) = &h.kind {
            if lambda.params.iter().any(|p| arena.get(*p).name == param) {
                out.push(lambda);
            }
        }
        h.for_each_child(|c| walk(c, arena, param, out));
    }
    let mut out = Vec::new();
    walk(hir, arena, SymbolId::of(param), &mut out);
    out
}

/// Every lambda in `hir` with no parameter at all.
fn lambdas_without_params(hir: &Hir) -> Vec<&Lambda> {
    fn walk<'h>(h: &'h Hir, out: &mut Vec<&'h Lambda>) {
        if let HirKind::Lambda(lambda) = &h.kind {
            if lambda.params.is_empty() {
                out.push(lambda);
            }
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(hir, &mut out);
    out
}

/// The kind of `lambda`'s capture of the binding spelled `name`, if it has one.
fn capture_of(lambda: &Lambda, arena: &BindingArena, name: &str) -> Option<CaptureKind> {
    let name = SymbolId::of(name);
    lambda
        .captures
        .iter()
        .find(|c| arena.get(c.binding).name == name)
        .map(|c| c.kind)
}

/// The initializer of the first `let`, `letrec` or `define` binding in `hir`
/// spelled `name`.
fn binding_init<'h>(hir: &'h Hir, arena: &BindingArena, name: &str) -> Option<&'h Hir> {
    fn walk<'h>(h: &'h Hir, arena: &BindingArena, name: SymbolId) -> Option<&'h Hir> {
        match &h.kind {
            HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
                for (b, init) in bindings {
                    if arena.get(*b).name == name {
                        return Some(init);
                    }
                }
            }
            HirKind::Define { binding, value } if arena.get(*binding).name == name => {
                return Some(value);
            }
            _ => {}
        }
        let mut found = None;
        h.for_each_child(|c| {
            if found.is_none() {
                found = walk(c, arena, name);
            }
        });
        found
    }
    walk(hir, arena, SymbolId::of(name))
}

/// `h` names `k`, and the lambda inside `f` names neither. The call to `h`
/// grafts, `k` grafts inside it, and the lambda now captures `k`, the one
/// binding the grafted body named that it did not hold.
///
/// The counter-factual: the lambda captures only `h`, so the graft declines
/// and the call to `h` stays.
#[test]
fn a_same_unit_body_grafts_inside_a_lambda_and_the_lambda_captures_what_it_names() {
    let src = "(defn k [x] x) (defn h [x] (k x)) (defn f [] (fn [y] (h y))) (f)";
    let (hir, arena) = compiled(src);
    assert_eq!(
        count_calls_to(&hir, &arena, "h"),
        0,
        "{src}: the call to h grafts"
    );
    let [inner] = lambdas_with_param(&hir, &arena, "y")[..] else {
        panic!("{src}: one lambda takes y");
    };
    assert_eq!(
        capture_of(inner, &arena, "k"),
        Some(CaptureKind::Local),
        "{src}: the lambda captures k from its parent's slot"
    );
}

/// The lambda that grafts sits two lambdas below the file, and neither of the
/// two holds `k`. Both capture it, because the inner one loads its capture
/// from the outer one's environment.
///
/// The counter-factual: a capture added to the innermost lambda alone names a
/// slot its parent never allocated.
#[test]
fn every_lambda_between_the_graft_and_the_file_captures_the_name() {
    let src = "(defn k [x] x) (defn h [x] (k x)) (defn f [] (fn [] (fn [y] (h y)))) (((f)) 7)";
    let (hir, arena) = compiled(src);
    assert_eq!(count_calls_to(&hir, &arena, "h"), 0, "{src}");
    let outer = lambdas_without_params(&hir);
    assert_eq!(outer.len(), 2, "{src}: f and the lambda it returns");
    for lambda in outer {
        assert_eq!(
            capture_of(lambda, &arena, "k"),
            Some(CaptureKind::Local),
            "{src}: every lambda on the way to the file captures k"
        );
    }
    assert_eq!(run_booted(src), Value::int(7), "{src}");
}

/// `h` and `k` are defined after the lambda that names `h`. The file letrec
/// holds their cells before any initializer runs, so the lambda captures `k`
/// and the program answers what it answered through the call.
///
/// The counter-factual: the graft declines a binding not yet initialized where
/// the lambda is written, as it must outside every lambda.
#[test]
fn a_body_naming_a_sibling_defined_later_grafts_and_runs() {
    let src = "(defn f [] (fn [y] (h y))) (defn h [x] (k x)) (defn k [x] x) ((f) 7)";
    let (hir, arena) = compiled(src);
    assert_eq!(count_calls_to(&hir, &arena, "h"), 0, "{src}");
    let [inner] = lambdas_with_param(&hir, &arena, "y")[..] else {
        panic!("{src}: one lambda takes y");
    };
    assert_eq!(
        capture_of(inner, &arena, "k"),
        Some(CaptureKind::Local),
        "{src}"
    );
    assert_eq!(run_booted(src), Value::int(7), "{src}");
}

/// Outside every lambda a body may name a module binding only once its
/// initializer ran, because the grafted body reads the cell at once. The call
/// to `h` at the top level names `k` through `h`'s body before `k` is bound,
/// so it stays a call.
#[test]
fn outside_every_lambda_a_body_naming_a_later_binding_keeps_the_call() {
    let src = "(defn h [x] (k x)) (def r (h 1)) (defn k [x] x) r";
    let (hir, arena) = compiled(src);
    assert_eq!(
        count_calls_to(&hir, &arena, "h"),
        1,
        "{src}: the top-level call stays"
    );
}

/// `rec` calls itself, so its lambda holds `rec` as a self-reference, which
/// resolves to the executing closure and not to the cell. The lambda inside it
/// grafts `peek`, whose body names `rec` as a value, so it needs the cell: the
/// self-reference becomes a plain capture, as the analyzer makes it when a
/// nested lambda names the enclosing binding.
///
/// The counter-factual: the inner lambda loads its parent's self slot, which
/// holds nil, and `(first got)` is nil rather than the closure.
#[test]
fn a_self_reference_the_graft_reaches_through_becomes_a_plain_capture() {
    let src = "(defn peek [y] (list rec y))
               (defn rec [n] (if (%lt n 1) (fn [y] (peek y)) (rec (%sub n 1))))
               (def got ((rec 2) 5))
               (and (fn? (first got)) (= (second got) 5))";
    let (hir, arena) = compiled(src);
    assert_eq!(count_calls_to(&hir, &arena, "peek"), 0, "{src}");
    let [outer] = lambdas_with_param(&hir, &arena, "n")[..] else {
        panic!("{src}: one lambda takes n");
    };
    assert_eq!(
        capture_of(outer, &arena, "rec"),
        Some(CaptureKind::Local),
        "{src}: rec's own lambda now captures rec's cell"
    );
    let [inner] = lambdas_with_param(&hir, &arena, "y")
        .into_iter()
        .filter(|l| {
            l.captures
                .iter()
                .any(|c| arena.get(c.binding).name == SymbolId::of("rec"))
        })
        .collect::<Vec<_>>()[..]
    else {
        panic!("{src}: the lambda inside rec captures rec");
    };
    assert_eq!(capture_of(inner, &arena, "rec"), Some(CaptureKind::Local));
    assert_eq!(run_booted(src), Value::bool(true), "{src}");
}

/// `under?` names the mutable `limit`, so the lambda that grafts it captures
/// `limit`'s cell and reads it live: the reassignment after the closure is
/// built is what the call sees.
///
/// The counter-factual: a capture by value reads 10, and `(g 5)` is true.
#[test]
fn a_mutable_module_binding_is_read_live_through_the_graft() {
    let src = "(var limit 10)
               (defn under? [x] (< x limit))
               (defn f [] (fn [y] (under? y)))
               (def g (f))
               (assign limit 3)
               (g 5)";
    let (hir, arena) = compiled(src);
    assert_eq!(count_calls_to(&hir, &arena, "under?"), 0, "{src}");
    assert_eq!(run_booted(src), Value::bool(false), "{src}");
}

/// The stdlib source compiled as one unit, the shape the boot compiles. The
/// array arm of `any?` walks with a `loop` lambda whose step is `(+ i 1)` and
/// whose test is `(>= i (length coll))`. Both wrappers name a sibling of the
/// stdlib, so both graft only through an added capture, and over the proven
/// int `i` each dissolves to its op with no guard left.
///
/// The counter-factual: two wrapper calls per element, each a rest-list
/// dispatch with two type tests.
#[test]
fn the_stdlib_search_loop_reaches_its_ops_without_a_wrapper_call() {
    let (hir, arena) = compiled(crate::pipeline::sources::STDLIB);
    let any = binding_init(&hir, &arena, "any?").expect("the stdlib defines any?");
    let walk = binding_init(any, &arena, "loop").expect("any? walks an array with a loop");
    for wrapper in [">=", "+"] {
        assert_eq!(
            count_calls_to(walk, &arena, wrapper),
            0,
            "the loop in any? holds no call to {wrapper}"
        );
    }
    assert_eq!(count_intrinsics(walk, "%ge"), 1, "the test is one %ge");
    assert_eq!(count_intrinsics(walk, "%add"), 1, "the step is one %add");
    assert_eq!(
        count_emits(walk),
        0,
        "i is a proven int, so no guard is left"
    );
}
