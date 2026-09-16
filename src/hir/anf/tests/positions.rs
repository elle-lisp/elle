// audited: 2026-09-16
// Which child positions the ANF lift names, and which it leaves alone.
//
// src/hir/anf.rs

use super::*;
use crate::hir::expr::HirKind;

// ── 1. (g (f x)): outer Call's arg lifted ────────────────────

#[test]
fn inline_call_arg_is_lifted() {
    let (hir, arena, symbols) = analyze_anf("(g (f 1))");
    let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
    assert_eq!(g_calls.len(), 1, "expected one call to g");
    let g_call = g_calls[0];
    let arg0 = match &g_call.kind {
        HirKind::Call { args, .. } => &args[0].expr,
        _ => unreachable!(),
    };
    assert!(
        is_anf_wrap(arg0),
        "expected (g (let [t (f 1)] t)) — got {:?}",
        arg0.kind
    );
    // And the wrapped init must be the original Call to f.
    let init = anf_wrap_init(arg0);
    match &init.kind {
        HirKind::Call { func, .. } => match &func.kind {
            HirKind::Var(b) => assert_eq!(symbols.name(arena.get(*b).name), Some("f")),
            _ => panic!("expected Call to f"),
        },
        _ => panic!("expected init to be a Call"),
    }
}

// ── 2. (let [a (f x)] a): init not re-lifted ──────────────────

#[test]
fn let_init_call_is_not_relifted() {
    let (hir, _arena, _symbols) = analyze_anf("(let [a (f 1)] a)");
    // Find every Let with a single binding whose init is a direct Call.
    // The user's outer Let — `(let [a (f 1)] a)` — qualifies and must
    // remain. Re-wrapping its init would produce
    // `(let [a (let [t (f 1)] t)] a)`: the init is already a name,
    // re-wrap is redundant and bloats the IR. We assert *some* such
    // Let exists in the post-ANF HIR.
    let mut found_direct_call_init = false;
    let mut visit = |node: &Hir| {
        if let HirKind::Let { bindings, .. } = &node.kind {
            if bindings.len() == 1 && matches!(&bindings[0].1.kind, HirKind::Call { .. }) {
                found_direct_call_init = true;
            }
        }
    };
    walk_pre(&hir, &mut visit);
    assert!(
        found_direct_call_init,
        "the user Let `(let [a (f 1)] a)` must keep its direct-Call init \
             (no re-wrap into `(let [a (let [t (f 1)] t)] a)`)"
    );
}

// ── 3. (g (%pair 1 2)): allocating intrinsic in Call arg lifted ──

#[test]
fn pair_intrinsic_in_call_arg_is_lifted() {
    let (hir, arena, symbols) = analyze_anf("(g (%pair 1 2))");
    let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
    assert_eq!(g_calls.len(), 1);
    let g_call = g_calls[0];
    let arg0 = match &g_call.kind {
        HirKind::Call { args, .. } => &args[0].expr,
        _ => unreachable!(),
    };
    assert!(
        is_anf_wrap(arg0),
        "expected (g (let [t (%pair 1 2)] t)) — got {:?}",
        arg0.kind
    );
    let init = anf_wrap_init(arg0);
    assert!(
        matches!(&init.kind, HirKind::Intrinsic { .. }),
        "expected init to be the %pair intrinsic"
    );
}

// ── 4. (h (g (f x))): nested calls chain into Lets ───────────

#[test]
fn nested_calls_chain_into_lets() {
    let (hir, arena, symbols) = analyze_anf("(h (g (f 1)))");
    let h_calls = find_calls_to(&hir, "h", &arena, &symbols);
    assert_eq!(h_calls.len(), 1);
    let h_call = h_calls[0];
    // h's arg: Let wrap around (g ...).
    let arg_to_h = match &h_call.kind {
        HirKind::Call { args, .. } => &args[0].expr,
        _ => unreachable!(),
    };
    assert!(is_anf_wrap(arg_to_h), "h's arg must be ANF wrap");
    let inner_g_call = anf_wrap_init(arg_to_h);
    match &inner_g_call.kind {
        HirKind::Call { func, args, .. } => {
            match &func.kind {
                HirKind::Var(b) => {
                    assert_eq!(symbols.name(arena.get(*b).name), Some("g"))
                }
                _ => panic!("expected g"),
            }
            // g's arg must itself be an ANF wrap around (f 1).
            assert!(is_anf_wrap(&args[0].expr), "g's arg must be ANF wrap");
            let f_call = anf_wrap_init(&args[0].expr);
            if let HirKind::Call { func: ff, .. } = &f_call.kind {
                if let HirKind::Var(b) = &ff.kind {
                    assert_eq!(symbols.name(arena.get(*b).name), Some("f"));
                } else {
                    panic!("expected Call to f");
                }
            }
        }
        _ => panic!("expected g Call"),
    }
}

// ── 5. (g (fn () 1)): Lambda in Call arg is lifted ───────────

#[test]
fn lambda_as_arg_is_lifted() {
    let (hir, arena, symbols) = analyze_anf("(g (fn () 1))");
    let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
    assert_eq!(g_calls.len(), 1);
    let g_call = g_calls[0];
    let arg0 = match &g_call.kind {
        HirKind::Call { args, .. } => &args[0].expr,
        _ => unreachable!(),
    };
    assert!(is_anf_wrap(arg0), "Lambda arg must be ANF wrap");
    let init = anf_wrap_init(arg0);
    assert!(
        matches!(&init.kind, HirKind::Lambda { .. }),
        "expected Lambda as the wrapped init — got {:?}",
        init.kind
    );
}

// ── 6. (g (%add 1 2)): non-allocating intrinsic NOT lifted ───

#[test]
fn non_allocating_intrinsic_is_not_lifted() {
    let (hir, arena, symbols) = analyze_anf("(g (%add 1 2))");
    let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
    assert_eq!(g_calls.len(), 1);
    let g_call = g_calls[0];
    let arg0 = match &g_call.kind {
        HirKind::Call { args, .. } => &args[0].expr,
        _ => unreachable!(),
    };
    assert!(
        !is_anf_wrap(arg0),
        "non-allocating intrinsic must NOT be wrapped — got {:?}",
        arg0.kind
    );
    assert!(
        matches!(&arg0.kind, HirKind::Intrinsic { .. }),
        "expected raw Intrinsic"
    );
}

// ── 7. (g (if c (f) (h))): branch results lifted in branch positions ──

#[test]
fn if_branch_result_lifted_when_used_as_value() {
    let (hir, arena, symbols) = analyze_anf("(g (if true (f) (h)))");
    let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
    assert_eq!(g_calls.len(), 1);
    let g_call = g_calls[0];
    let arg0 = match &g_call.kind {
        HirKind::Call { args, .. } => &args[0].expr,
        _ => unreachable!(),
    };
    // The If itself is not allocating, so it shouldn't be wrapped at
    // Call.args[0]. (See `if_does_not_allocate_itself` in expr.rs.)
    let if_node = arg0;
    let (then_b, else_b) = match &if_node.kind {
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => (then_branch.as_ref(), else_branch.as_ref()),
        _ => panic!("expected If node — got {:?}", arg0.kind),
    };
    assert!(
        is_anf_wrap(then_b),
        "then branch (f) must be ANF wrap — got {:?}",
        then_b.kind
    );
    assert!(
        is_anf_wrap(else_b),
        "else branch (h) must be ANF wrap — got {:?}",
        else_b.kind
    );
}

// ── 9. match value with rest pattern is lifted ────────────────

#[test]
fn match_with_rest_pattern_is_lifted() {
    // `(g (match v (@[& rest] rest)))` — the match value position
    // (Call.args[0]) is wrapped because Match allocates iff any arm
    // pattern allocates; @[& rest] (Array with rest) does. The
    // wrap lets the lowerer associate the match result's region
    // with the synthetic binding's slot.
    let (hir, arena, symbols) =
        analyze_anf("(let [v (f 1 2 3)] (g (match v @[& rest] rest _ nil)))");
    let g_calls = find_calls_to(&hir, "g", &arena, &symbols);
    assert_eq!(g_calls.len(), 1);
    let g_call = g_calls[0];
    let arg0 = match &g_call.kind {
        HirKind::Call { args, .. } => &args[0].expr,
        _ => unreachable!(),
    };
    assert!(
        is_anf_wrap(arg0),
        "Match with allocating rest pattern must be ANF wrap"
    );
    let init = anf_wrap_init(arg0);
    assert!(
        matches!(&init.kind, HirKind::Match { .. }),
        "expected Match as wrapped init — got {:?}",
        init.kind
    );
}
