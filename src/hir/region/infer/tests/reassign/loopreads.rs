// audited: 2026-11-19
//! A read of a loop's var inside the loop is counted, whatever the var held before the loop.
//!
//! docs/impl/region/reads.md

use super::*;

/// How far the `let` that binds `t` reaches in the loop body.
#[derive(Clone, Copy, Debug)]
enum Reach {
    /// The `let` spans the rest of the pass, the counter's step included.
    RestOfPass,
    /// The `let` closes before the counter steps.
    BeforeStep,
}

/// A `while` over `c`, whose body binds `t` to `c` with a `let` that reaches as
/// `reach` says, then assigns `c`, then reads `t`. `{init}` is what `c` starts
/// as.
fn loop_reader(init: &str, reach: Reach) -> String {
    let body = match reach {
        Reach::RestOfPass => {
            "(let [t c]\n\
               (assign c (array i))\n\
               (length t)\n\
               (assign i (%add i 1)))"
        }
        Reach::BeforeStep => {
            "(begin (let [t c]\n\
                      (assign c (array i))\n\
                      (length t))\n\
                    (assign i (%add i 1)))"
        }
    };
    format!(
        "(def @h (fn (n)\n\
           (begin (var c {init})\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    {body})\n\
                  c)))\n\
         (h 3)"
    )
}

/// Functionalization makes `c` a loop parameter, and the body's `assign` a
/// slot write of it. The write drops what the parameter held, so `t` takes a
/// counted reference of its own, however far the `let` that binds it reaches.
///
/// The counter-factual: the parameter read immutable, so the walk asked no
/// count of the read, `t` held the parameter's regions, and the next pass's
/// assign freed the value under `t`
/// (tests/impl/region-container-reader-uaf.lisp).
#[test]
fn reassign_gate_counts_a_read_of_a_loop_parameter() {
    for reach in [Reach::RestOfPass, Reach::BeforeStep] {
        let (hir, arena, info) = pipeline(&loop_reader("(array 0)", reach));
        let (t, init) = bound_by_name(&hir, &arena, "t");
        let HirKind::Var(c) = init.kind else {
            panic!("precondition ({reach:?}): t's init is a bare read of c");
        };
        assert!(
            is_loop_parameter(&hir, c),
            "precondition ({reach:?}): the read names the loop's version of c"
        );
        assert_counted_reader(&info, t, init);
    }
}

/// A var that starts as `nil` holds no region before the loop, and the walk
/// passes the body once, so at the read it has seen no region for the var.
/// Every pass after the first still reads the array the pass before stored,
/// so the read is counted all the same, for a `var` and for a `let`-bound
/// mutable.
///
/// The counter-factual: the count skipped a read whose regions the walk had
/// not seen, so `t` held nothing and the assign freed its array.
#[test]
fn reassign_gate_counts_a_read_of_a_nil_started_container() {
    let let_bound = "(def @h (fn (n)\n\
           (let [@c nil @i 0]\n\
             (while (%lt i n)\n\
               (let [t c]\n\
                 (assign c (array i))\n\
                 (length t)\n\
                 (assign i (%add i 1))))\n\
             c)))\n\
         (h 3)";
    for source in [loop_reader("nil", Reach::RestOfPass), let_bound.to_string()] {
        let (hir, arena, info) = pipeline(&source);
        let (t, init) = bound_by_name(&hir, &arena, "t");
        assert!(
            matches!(init.kind, HirKind::Var(_)),
            "precondition: t's init is a bare read of c"
        );
        assert_counted_reader(&info, t, init);
    }
}
