use super::*;

#[test]
fn compute_order_ranks_by_structure_not_hirid_magnitude() {
    use crate::syntax::Span;
    let sp = Span::synthetic();
    let mk = |kind, id| {
        let mut h = Hir::silent(kind, sp.clone());
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

/// Parse → expand → analyze → functionalize → ANF, returning the HIR.

#[test]
fn compute_order_call_func_follows_args() {
    // `lower_call` evaluates a call's ARGUMENTS first and its FUNC
    // expression last (both the plain and splice paths —
    // src/lir/lower/control/call.rs). `compute_order` is the analysis
    // side's structural execution order; it must agree for every Call,
    // or a binding whose last read sits in func position is released at
    // the arg-position read — the value-based release plus nil
    // slot-stamp lands after the first (arg) read and the later (func)
    // read sees nil (tests/elle/region-call-func-position-reread.lisp;
    // the compress.lisp `(z:unzstd (z:zstd ""))` failure).
    let hir = hir_of(
        "(let [z ((fn [] (def f (fn [x] 1)) (def g (fn [x] 2)) {:f f :g g}))]
           ((get z :g) ((get z :f) \"\")))",
    );
    let order = compute_order(&hir);
    fn check(h: &Hir, order: &HashMap<HirId, u32>, checked: &mut usize) {
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
