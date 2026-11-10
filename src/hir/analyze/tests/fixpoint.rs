// audited: 2026-11-10
//! The signal fixpoint: a lambda that read a seed takes the signal its binding settles on, and each error reports once.
//!
//! docs/pipeline.md

use super::*;
use crate::pipeline::AnalyzeResult;

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

/// `source` analyzed as one form, as `eval` sees it.
fn analyze_form(source: &str) -> AnalyzeResult {
    let mut rt = crate::runtime::Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    crate::pipeline::analyze(source, symbols, vm, cctx, "<fixpoint>")
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

// Convergence must not inflate: a silent self-reference stays silent, or a
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

// Counter-factual: the file fixpoint kept the error the first analysis of `f`
// reported, and again the one each re-analysis reported.
#[test]
fn a_top_level_function_reports_an_undefined_name_once() {
    let result = analyze_file("(defn f [] (zork))");
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}

// Counter-factual: the file fixpoint never cleared its errors between passes.
// `c` emits, so `b` and then `a` move off the seed, and the loop makes three
// passes over `a`: three reports.
#[test]
fn an_undefined_name_reports_once_however_many_passes_the_file_fixpoint_makes() {
    let result =
        analyze_file("(defn a [] (b) (nope1)) (defn b [] (c)) (defn c [] (emit :yield 1))");
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}

// Counter-factual: a letrec inside a top-level function is walked by both
// loops, and each kept its own copies. `nope1` reported twice and `nope2`
// four times.
#[test]
fn an_undefined_name_in_a_nested_letrec_reports_once() {
    let result = analyze_file("(defn f [] (letrec [p (fn [] (nope1)) q (fn [] (nope2))] 1))");
    assert_eq!(result.errors.len(), 2, "{:?}", result.errors);
}

// Counter-factual: the letrec fixpoint kept every error of its first pass, and
// again the errors of the last entry it re-analyzed. The raising lambda comes
// last here, so it reported twice.
#[test]
fn a_letrec_lambda_reports_an_undefined_name_once() {
    let result = analyze_form("(letrec [g (fn [] (f)) f (fn [] (zork))] (g))");
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}

// The analysis of a defn that converges runs its body more than once, and each
// run reports the undefined name again.
#[test]
fn a_converging_defn_reports_an_undefined_name_once() {
    let result = analyze_form(
        "(begin
           (defn walk [n] (if (= n 0) (zork) ((fn [k] (walk k)) (- n 1))))
           walk)",
    );
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}

// Counter-factual: a lambda a def binds inside a top-level form was analyzed
// once, in Pass 2, against the silent seed of the later `g`, and Pass 3 never
// took it up. `f` kept that silence, and a caller trusting it broke its proof
// at run time.
#[test]
fn a_lambda_def_nested_in_a_top_level_form_takes_its_callee_s_settled_signal() {
    let result = analyze_file("(def k (do (defn f [] (g)) 1)) (defn g [] (yield 1))");
    assert_eq!(signal_of(&result, "g"), Signal::yields());
    assert_eq!(
        signal_of(&result, "f"),
        Signal::yields(),
        "f yields as g does"
    );
}

#[test]
fn a_caller_of_a_nested_lambda_def_takes_its_settled_signal() {
    let result =
        analyze_file("(def k (do (defn f [] (g)) 1)) (defn h [] (f)) (defn g [] (yield 1))");
    assert_eq!(
        signal_of(&result, "h"),
        Signal::yields(),
        "h yields as f does"
    );
}

#[test]
fn a_lambda_def_nested_in_a_top_level_expression_converges_too() {
    let result = analyze_file("(identity (do (defn f [] (g)) 1)) (defn g [] (yield 1))");
    assert_eq!(
        signal_of(&result, "f"),
        Signal::yields(),
        "f yields as g does"
    );
}

// Counter-factual: `early` reads `f` before its nested `def`, so the file sets
// `f` first and the `def` assigns it. The fixpoint took up only a nested `def`
// that defines its binding, so `f` kept `g`'s silent seed, and its call to `g`
// broke its proof at run time.
#[test]
fn a_hoisted_lambda_def_nested_in_a_top_level_form_converges_too() {
    let result =
        analyze_file("(defn early [] (f)) (def k (do (defn f [] (g)) 1)) (defn g [] (yield 1))");
    let [_early, f, _g] = lambda_signals(&result.hir)[..] else {
        panic!("expected early, f and g");
    };
    assert_eq!(f, Signal::yields(), "f yields as g does");
}

// Counter-factual: the merged bindings the file's last form leaves waiting
// went to the first lambda the fixpoint analyzed again, whose value was a
// `Begin` in that pass. The fixpoint saw no lambda there and recorded no move
// for `a`, so `h` kept the silence it read in Pass 2.
#[test]
fn a_merge_the_last_form_leaves_hides_no_signal_that_moved() {
    let result = analyze_file(
        "(defn a [] (b) :a)
         (defn h [] (a) :h)
         (defn b [] (yield 1))
         (def c true)
         (when c (defn f [] 1))",
    );
    assert_eq!(signal_of(&result, "a"), Signal::yields());
    assert_eq!(
        signal_of(&result, "h"),
        Signal::yields(),
        "h yields as a does"
    );
}
