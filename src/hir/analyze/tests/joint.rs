// audited: 2026-10-25
//! A unit's signals settle jointly: each lambda takes the signal of what it calls, whatever the order, and each error reports once.
//!
//! docs/pipeline.md

use super::*;
use crate::pipeline::AnalyzeResult;
use crate::value::SIG_YIELD;

/// A function body that defines `walk`, which yields, and calls it from a
/// lambda inside its own body.
const WALK_IN_A_BODY: &str = "(defn outer []
  (defn walk [n]
    (if (= n 0) (yield :leaf) (list :back ((fn [k] (walk k)) (- n 1)))))
  walk)";

/// The same `walk` in a form compiled alone, outside every function.
const WALK_ALONE: &str = "(begin
  (defn walk [n]
    (if (= n 0) (yield :leaf) (list :back ((fn [k] (walk k)) (- n 1)))))
  walk)";

/// `source` analyzed as a file, as the linter and `compile/analyze` see it.
fn analyze_file(source: &str) -> AnalyzeResult {
    let mut rt = crate::runtime::Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    crate::pipeline::analyze_file(source, symbols, vm, cctx, "<joint>")
        .unwrap_or_else(|e| panic!("{source} did not analyze: {e}"))
}

/// `source` analyzed as one form, as `eval` sees it.
fn analyze_form(source: &str) -> AnalyzeResult {
    let mut rt = crate::runtime::Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    crate::pipeline::analyze(source, symbols, vm, cctx, "<joint>")
        .unwrap_or_else(|e| panic!("{source} did not analyze: {e}"))
}

/// The inferred signal of every lambda in `hir`, outermost first.
fn lambda_signals(hir: &Hir) -> Vec<Signal> {
    fn collect(hir: &Hir, out: &mut Vec<Signal>) {
        if let HirKind::Lambda(lambda) = &hir.kind {
            out.push(lambda.inferred_signals);
        }
        hir.for_each_child(|child| collect(child, out));
    }
    let mut out = Vec::new();
    collect(hir, &mut out);
    out
}

/// The signal of every call node in `hir`, in the order the walk meets them.
fn call_signals(hir: &Hir) -> Vec<Signal> {
    fn collect(hir: &Hir, out: &mut Vec<Signal>) {
        if let HirKind::Call { .. } = &hir.kind {
            out.push(hir.signal);
        }
        hir.for_each_child(|child| collect(child, out));
    }
    let mut out = Vec::new();
    collect(hir, &mut out);
    out
}

// Counter-factual: `walk` was analyzed once, against its own silent seed. The
// lambda inside it read that seed, inferred silent, and its yield broke the
// proof at run time.
#[test]
fn a_lambda_in_a_function_body_defn_takes_the_signal_of_the_defn() {
    let result = analyze_file(WALK_IN_A_BODY);
    let [_outer, walk, inner] = lambda_signals(&result.hir)[..] else {
        panic!("expected outer, walk and the lambda inside walk");
    };
    assert_eq!(walk, Signal::yields_errors(), "walk yields and may raise");
    assert_eq!(
        inner, walk,
        "the lambda that calls walk signals as walk does"
    );
}

#[test]
fn a_lambda_in_a_defn_compiled_alone_takes_the_signal_of_the_defn() {
    let result = analyze_form(WALK_ALONE);
    let [walk, inner] = lambda_signals(&result.hir)[..] else {
        panic!("expected walk and the lambda inside walk");
    };
    assert_eq!(walk, Signal::yields_errors(), "walk yields and may raise");
    assert_eq!(
        inner, walk,
        "the lambda that calls walk signals as walk does"
    );
}

// A joint solve must not inflate: a silent self-reference stays silent, or a
// correct `(silent!)` lambda would stop compiling.
#[test]
fn a_lambda_in_a_silent_defn_stays_silent() {
    let result = analyze_file(
        "(defn outer []
           (defn tally [n] (if (%eq n 0) 0 ((fn [k] (tally k)) (%sub n 1))))
           tally)",
    );
    let [_outer, tally, inner] = lambda_signals(&result.hir)[..] else {
        panic!("expected outer, tally and the lambda inside tally");
    };
    assert_eq!(tally, Signal::silent());
    assert_eq!(inner, Signal::silent());
}

// Counter-factual: a body seeded `g` only as it reached it, so `f` called a
// name with no signal yet and took every bit.
#[test]
fn a_lambda_that_calls_a_later_sibling_takes_its_signal() {
    let result = analyze_file("(defn outer [] (defn f [] (g)) (defn g [] (yield 1)) f)");
    let [_outer, f, g] = lambda_signals(&result.hir)[..] else {
        panic!("expected outer, f and g");
    };
    assert_eq!(g, Signal::yields(), "g yields");
    assert_eq!(f, g, "f signals as the later g does");
}

// The lowerer reads a call node's own signal to choose its calling convention,
// so the forward call must carry the yield as well as the lambda around it.
#[test]
fn a_call_to_a_later_sibling_carries_its_signal() {
    let result = analyze_file("(defn outer [] (defn f [] (g)) (defn g [] (yield 1)) f)");
    let calls = call_signals(&result.hir);
    assert!(
        calls.contains(&Signal::yields()),
        "the call to g reads g's yield: {calls:?}"
    );
    assert!(
        calls.iter().all(|call| call.bits == SIG_YIELD),
        "no call takes a bit g does not raise: {calls:?}"
    );
}

#[test]
fn mutual_siblings_in_a_body_settle_on_the_raise() {
    let result = analyze_file(
        "(defn outer []
           (defn f [n] (if n (g false) 0))
           (defn g [n] (yield n) (f n))
           f)",
    );
    let [_outer, f, g] = lambda_signals(&result.hir)[..] else {
        panic!("expected outer, f and g");
    };
    assert_eq!(f, Signal::yields(), "f reaches g's yield");
    assert_eq!(g, Signal::yields(), "g yields and calls f");
}

#[test]
fn silent_mutual_siblings_in_a_body_stay_silent() {
    let result = analyze_file(
        "(defn outer []
           (defn f [n] (if n (g false) 0))
           (defn g [n] (if n (f n) 1))
           f)",
    );
    let [_outer, f, g] = lambda_signals(&result.hir)[..] else {
        panic!("expected outer, f and g");
    };
    assert_eq!(f, Signal::silent());
    assert_eq!(g, Signal::silent());
}

// Counter-factual: the pass read the first lambda `h` holds, so `f` inferred
// silent, and the yield of the lambda the assign stored broke the proof.
#[test]
fn a_call_through_a_reassigned_var_takes_every_bit() {
    let result = analyze_file(
        "(defn outer []
           (var h (fn [] 1))
           (defn f [] (h))
           (assign h (fn [] (yield 1)))
           f)",
    );
    let [_outer, _first, f, _second] = lambda_signals(&result.hir)[..] else {
        panic!("expected outer, the first h, f and the second h");
    };
    assert!(
        Signal::unknown().bits.subtract(f.bits).is_empty(),
        "f may raise anything: {f:?}"
    );
}

// Counter-factual: the pass checked the forward call's count against the
// first lambda `h` held, and the analysis failed.
#[test]
fn a_forward_call_through_a_reassigned_var_checks_its_count_at_run_time() {
    analyze_file(
        "(defn outer []
           (defn f [] (h 1))
           (var h (fn [] 0))
           (assign h (fn [x] x))
           f)",
    );
}

#[test]
fn a_top_level_function_reports_an_undefined_name_once() {
    let result = analyze_file("(defn f [] (zork))");
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}

// Counter-factual: each re-analysis of a definition reported its errors again.
// `c` emits, so `b` and then `a` move off the seed, and a loop that analyzes
// `a` until the chain settles reports three times.
#[test]
fn an_undefined_name_in_a_chain_of_callers_reports_once() {
    let result =
        analyze_file("(defn a [] (b) (nope1)) (defn b [] (c)) (defn c [] (emit :yield 1))");
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}

// Counter-factual: a letrec inside a top-level function was analyzed by both
// the file and the letrec, and each kept its own copies. `nope1` reported
// twice and `nope2` four times.
#[test]
fn an_undefined_name_in_a_nested_letrec_reports_once() {
    let result = analyze_file("(defn f [] (letrec [p (fn [] (nope1)) q (fn [] (nope2))] 1))");
    assert_eq!(result.errors.len(), 2, "{:?}", result.errors);
}

// Counter-factual: the letrec kept every error of its first analysis, and again
// the errors of the last entry it analyzed again. The raising lambda comes last
// here, so it reported twice.
#[test]
fn a_letrec_lambda_reports_an_undefined_name_once() {
    let result = analyze_form("(letrec [g (fn [] (f)) f (fn [] (zork))] (g))");
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}

#[test]
fn a_self_referring_defn_reports_an_undefined_name_once() {
    let result = analyze_form(
        "(begin
           (defn walk [n] (if (= n 0) (zork) ((fn [k] (walk k)) (- n 1))))
           walk)",
    );
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}
