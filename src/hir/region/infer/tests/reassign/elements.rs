// audited: 2026-11-17
//! A `%get`, `%first` or `%rest` out of a 1-slot container is a counted read, and so is a branch arm that is one.
//!
//! docs/impl/region/reads.md

use super::*;

/// A `while` over `c`, which holds an array of arrays. The body binds `t` to
/// `{read}`, then assigns `c`, then reads `t`.
fn element_reader(read: &str) -> String {
    format!(
        "(def @h (fn (n)\n\
           (begin (var c (array (array 0)))\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (begin (def t {read})\n\
                           (assign c (array (array i)))\n\
                           (length t)\n\
                           (assign i (%add i 1))))\n\
                  c)))\n\
         (h 3)"
    )
}

/// The `%get` operand of `node`, the binding it reads.
fn get_operand(node: &Hir) -> Binding {
    let HirKind::Intrinsic { args, .. } = &node.kind else {
        panic!("precondition: the read is an intrinsic");
    };
    let HirKind::Var(c) = args[0].kind else {
        panic!("precondition: the intrinsic reads a bare name");
    };
    c
}

/// An element of what a 1-slot container holds lives as long as that value,
/// which the container's next store releases. A borrow cannot move that store,
/// so `t` takes a counted reference of its own and holds none of the
/// container's regions. The `%first` cursor is the same read with a list for
/// the container.
///
/// The counter-factual: the element read took no count, and `%get` handed `t`
/// the container's regions, so `t` read an element of the freed array on the
/// next pass (tests/impl/region-container-element-uaf.lisp).
#[test]
fn reassign_gate_counts_an_element_read_of_a_container() {
    let (hir, arena, info) = pipeline(&element_reader("(%get c 0)"));
    let (t, init) = bound_by_name(&hir, &arena, "t");
    let c = get_operand(init);
    assert!(
        is_loop_parameter(&hir, c),
        "precondition: the %get reads the loop's version of c"
    );
    assert!(
        info.binding_source_regions
            .get(&c)
            .is_some_and(|rs| !rs.is_empty()),
        "precondition: the container holds a region the reader could hold"
    );
    assert_counted_reader(&info, t, init);

    let (hir, arena, info) = pipeline(
        "(def @h (fn ()\n\
           (begin (var cur (list (array 0) (array 1)))\n\
                  (while (%pair? cur)\n\
                    (begin (def x (%first cur))\n\
                           (assign cur (%rest cur))\n\
                           (length x)))\n\
                  cur)))\n\
         (h)",
    );
    let (x, init) = bound_by_name(&hir, &arena, "x");
    assert!(
        matches!(
            init.kind,
            HirKind::Intrinsic {
                op: crate::hir::expr::IntrinsicOp::First,
                ..
            }
        ),
        "precondition: x's init is the %first"
    );
    assert_counted_reader(&info, x, init);
}

/// A branch is a read of whichever arms read, and an arm that is an element
/// read is a reading arm. The `nil` arm reads nothing and holds no region, so
/// the reader holds the placeholder alone.
#[test]
fn reassign_gate_counts_a_branch_element_read() {
    let (hir, arena, info) = pipeline(&element_reader("(if (%lt i 0) nil (%get c 0))"));
    let (t, init) = bound_by_name(&hir, &arena, "t");
    assert!(
        matches!(init.kind, HirKind::If { .. }),
        "precondition: t's init is the branch"
    );
    assert_counted_reader(&info, t, init);
}
