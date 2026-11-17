// audited: 2026-11-17
//! A read of a loop's var inside the loop is counted, whatever the var held before the loop.
//!
//! docs/impl/region/reads.md

use super::*;

/// A `while` over `c`, whose body binds `t` to `c` with `binder`, then assigns
/// `c`, then reads `t`. `{init}` is what `c` starts as.
fn loop_reader(init: &str, binder: &str) -> String {
    let body = match binder {
        "def" => "(begin (def t c)\n\
                         (assign c (array i))\n\
                         (length t)\n\
                         (assign i (%add i 1)))"
            .to_string(),
        "let" => "(begin (let [t c]\n\
                           (assign c (array i))\n\
                           (length t))\n\
                         (assign i (%add i 1)))"
            .to_string(),
        _ => unreachable!("a def or a let reader"),
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
/// counted reference of its own, whether a `def` or a `let` binds it.
///
/// The counter-factual: the parameter read immutable, so the walk asked no
/// count of the read, `t` held the parameter's regions, and the next pass's
/// assign freed the value under `t`
/// (tests/impl/region-container-reader-uaf.lisp).
#[test]
fn reassign_gate_counts_a_read_of_a_loop_parameter() {
    for binder in ["def", "let"] {
        let (hir, arena, info) = pipeline(&loop_reader("(array 0)", binder));
        let (t, init) = bound_by_name(&hir, &arena, "t");
        let HirKind::Var(c) = init.kind else {
            panic!("precondition ({binder}): t's init is a bare read of c");
        };
        assert!(
            is_loop_parameter(&hir, c),
            "precondition ({binder}): the read names the loop's version of c"
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
               (begin (def t c)\n\
                      (assign c (array i))\n\
                      (length t)\n\
                      (assign i (%add i 1))))\n\
             c)))\n\
         (h 3)";
    for source in [loop_reader("nil", "def"), let_bound.to_string()] {
        let (hir, arena, info) = pipeline(&source);
        let (t, init) = bound_by_name(&hir, &arena, "t");
        assert!(
            matches!(init.kind, HirKind::Var(_)),
            "precondition: t's init is a bare read of c"
        );
        assert_counted_reader(&info, t, init);
    }
}
