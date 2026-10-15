// audited: 2026-10-14
//! Pins `compute_order`, the structural execution order every liveness decision compares, and the `BitSet` it runs on.
//!
//! docs/impl/region/anchors.md

use super::*;

/// An execution-order index ranks by STRUCTURE, not by `HirId` magnitude. ANF
/// appends synthetic `let` bindings with fresh, high ids even when they sit
/// inside a loop body, so a binding bound INSIDE a loop can carry an id LARGER
/// than the loop. The counter-factual: a `compute_order` that returns `HirId.0`
/// reads such a binding as bound outside, extends its region's `decref_point`
/// to the loop, and emits a phantom `DecrefRegion` on an empty iterator.
#[test]
fn compute_order_ranks_by_structure_not_hirid_magnitude() {
    use crate::syntax::Span;
    let sp = Span::synthetic();
    let mk = |kind, id| {
        let mut h = Hir::silent(kind, sp);
        h.id = HirId(id);
        h
    };
    let b = Binding(0);
    let acc = Binding(1);
    // Loop(id=10) { [acc = Int(3)] body:
    //   Let(id=99) { [b = Int(98)] body: Var(b)(id=97) } }
    // The inner Let's id (99) is LARGER than the enclosing Loop's
    // (10), exactly as ANF would assign them.
    let var = mk(HirKind::Var(b), 97);
    let let_init = mk(HirKind::Int(0), 98);
    let let_node = mk(
        HirKind::Let {
            bindings: vec![(b, let_init)],
            body: Box::new(var),
        },
        99,
    );
    let loop_init = mk(HirKind::Int(0), 3);
    let loop_node = mk(
        HirKind::Loop {
            bindings: vec![(acc, loop_init)],
            body: Box::new(let_node),
        },
        10,
    );

    let order = compute_order(&loop_node);
    let loop_ord = order[&HirId(10)];
    let let_ord = order[&HirId(99)];
    let var_ord = order[&HirId(97)];
    assert!(
        loop_ord > let_ord,
        "loop (ancestor, HirId 10) must rank after the inner let \
             (descendant, HirId 99) in execution order despite the smaller \
             HirId; got loop_ord={loop_ord} let_ord={let_ord}"
    );
    assert!(
        let_ord > var_ord,
        "let must rank after its body Var in execution order; \
             got let_ord={let_ord} var_ord={var_ord}"
    );
}

#[test]
fn compute_order_indexes_parameterize_key() {
    // `parameterize` evaluates each binding's PARAMETER (key) expression, then
    // its value, before the body (`lower_parameterize`). `for_each_child` — the
    // sole child enumeration `compute_order` walks — must therefore visit the
    // key. If it skips the key, the key's subtree gets no execution-order index,
    // so a binding whose LAST read sits in a parameterize key is invisible to
    // decref placement: its slot is reclaimed at an EARLIER read (e.g. a
    // fiber-captured use) and the parameterize then reads the nil'd slot
    // (tests/lang/parameters.lisp, "Creation snapshot is independent of
    // resumer's bindings").
    use crate::syntax::Span;
    let sp = Span::synthetic();
    let mk = |kind, id| {
        let mut h = Hir::silent(kind, sp);
        h.id = HirId(id);
        h
    };
    let p = Binding(0);
    // Parameterize(id=30) { [(key=Var(p)(id=10), value=Int(id=11))] body: Int(id=12) }
    let key = mk(HirKind::Var(p), 10);
    let value = mk(HirKind::Int(0), 11);
    let body = mk(HirKind::Int(0), 12);
    let param_node = mk(
        HirKind::Parameterize {
            bindings: vec![(key, value)],
            body: Box::new(body),
        },
        30,
    );

    let order = compute_order(&param_node);
    assert!(
        order.contains_key(&HirId(10)),
        "compute_order must index the parameterize KEY (@10); for_each_child \
         skipping it leaves the key with no execution-order index and its \
         binding's last read invisible to decref placement"
    );
    assert!(
        order[&HirId(30)] > order[&HirId(10)],
        "the parameterize node must rank after its key in execution order"
    );
    assert!(
        order[&HirId(11)] > order[&HirId(10)],
        "the binding value must rank after the key — lower_parameterize \
         evaluates the parameter expression before the value"
    );
}

/// A node's interval `[low, index]` holds exactly its subtree. The expected
/// subtree comes from a walk of its own, so the test does not read the
/// interval back from the code that built it. The counter-factual: a `low`
/// taken from the node's own index alone holds no descendant, and one taken
/// from the parent's `low` also holds an earlier sibling.
#[test]
fn an_interval_holds_exactly_the_subtree() {
    let hir = hir_of(
        "(defn f [xs]
           (var acc 0)
           (def g (fn [y] (+ y 1)))
           (each x in xs
             (let [h (fn [] (g x))]
               (if (> x 1) (assign acc (+ acc (h))) (assign acc acc))))
           acc)
         (f [1 2 3])",
    );
    let order = compute_order(&hir);

    fn ids(h: &Hir, out: &mut Vec<HirId>) {
        out.push(h.id);
        h.for_each_child(|c| ids(c, out));
    }
    fn subtrees(h: &Hir, out: &mut Vec<(HirId, Vec<HirId>)>) {
        let mut below = Vec::new();
        ids(h, &mut below);
        out.push((h.id, below));
        h.for_each_child(|c| subtrees(c, out));
    }
    let mut all = Vec::new();
    ids(&hir, &mut all);
    let mut nodes = Vec::new();
    subtrees(&hir, &mut nodes);
    assert!(
        nodes.len() > 20,
        "the tree is large enough to mean something"
    );

    for (n, below) in &nodes {
        let low = order.low(n).expect("every node has a low");
        let top = order[n];
        assert!(low <= top, "@{}: low {low} above index {top}", n.0);
        for x in &all {
            let held = (low..=top).contains(&order[x]);
            assert_eq!(
                held,
                below.contains(x),
                "@{} [{low}, {top}] and @{} at {}: the interval must hold \
                 exactly the subtree",
                n.0,
                x.0,
                order[x],
            );
        }
    }
}

#[test]
fn test_bitset_basic() {
    let mut bs = BitSet::new(128);
    assert!(!bs.contains(0));
    bs.set(0);
    assert!(bs.contains(0));
    bs.set(65);
    assert!(bs.contains(65));
    bs.clear(0);
    assert!(!bs.contains(0));
    assert!(bs.contains(65));
}

#[test]
fn test_bitset_union() {
    let mut a = BitSet::new(128);
    let mut b = BitSet::new(128);
    a.set(0);
    b.set(1);
    let changed = a.union_with(&b);
    assert!(changed);
    assert!(a.contains(0));
    assert!(a.contains(1));
    let changed2 = a.union_with(&b);
    assert!(!changed2);
}

#[test]
fn test_bitset_iter() {
    let mut bs = BitSet::new(128);
    bs.set(3);
    bs.set(67);
    bs.set(100);
    let bits: Vec<usize> = bs.iter().collect();
    assert_eq!(bits, vec![3, 67, 100]);
}

// ── compute_order vs lower_call evaluation order ─────────────────

#[test]
fn compute_order_call_func_follows_args() {
    // `lower_call` evaluates a call's ARGUMENTS first and its FUNC
    // expression last (both the plain and splice paths —
    // src/lir/lower/control/call.rs). `compute_order` is the analysis
    // side's structural execution order; it must agree for every Call,
    // or a binding whose last read sits in func position is released at
    // the arg-position read — the value-based release plus nil
    // slot-stamp lands after the first (arg) read and the later (func)
    // read sees nil (tests/impl/region-call-func-position-reread.lisp).
    let hir = hir_of(
        "(let [z ((fn [] (def f (fn [x] 1)) (def g (fn [x] 2)) {:f f :g g}))]
           ((get z :g) ((get z :f) \"\")))",
    );
    let order = compute_order(&hir);
    fn check(h: &Hir, order: &Order, checked: &mut usize) {
        if let HirKind::Call { func, args, .. } = &h.kind {
            for a in args {
                assert!(
                    order[&func.id] > order[&a.expr.id],
                    "Call @{}: func subtree (@{} ord {}) must follow arg subtree \
                     (@{} ord {}) in execution order — args evaluate first",
                    h.id.0,
                    func.id.0,
                    order[&func.id],
                    a.expr.id.0,
                    order[&a.expr.id],
                );
            }
            *checked += 1;
        }
        h.for_each_child(|c| check(c, order, checked));
    }
    let mut checked = 0;
    check(&hir, &order, &mut checked);
    assert!(checked >= 2, "expected to check at least the two get calls");
}
