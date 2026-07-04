use super::*;
use crate::hir::decision::*;

#[test]
fn test_or_pattern_expansion() {
    // Or(1, 2, 3) should expand to 3 patterns
    let or_pat = HirPattern::Or(vec![lit_int(1), lit_int(2), lit_int(3)]);
    let expanded = expand_or_pattern(&or_pat);
    assert_eq!(expanded.len(), 3);
}
#[test]
fn test_reachable_arms() {
    // Two distinct literals + wildcard → all 3 arms reachable
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![lit_int(1)], false, 0),
            PatternRow::new(vec![lit_int(2)], false, 1),
            PatternRow::new(vec![HirPattern::Wildcard], false, 2),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    let reachable = find_reachable_arms(&tree);
    assert_eq!(reachable.len(), 3);
    assert!(reachable.contains(&0));
    assert!(reachable.contains(&1));
    assert!(reachable.contains(&2));
}
#[test]
fn test_unreachable_arm_detected() {
    // Wildcard before literal → literal is unreachable
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![HirPattern::Wildcard], false, 0),
            PatternRow::new(vec![lit_int(1)], false, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    let reachable = find_reachable_arms(&tree);
    assert!(reachable.contains(&0));
    assert!(!reachable.contains(&1));
}
#[test]
fn test_unreachable_arms_from_arms() {
    use crate::syntax::Span;
    let body = || Hir::silent(crate::hir::HirKind::Bool(true), Span::synthetic());

    // (match x (_ ...) (1 ...)) → arm 1 unreachable
    let arms = vec![
        (HirPattern::Wildcard, None, body()),
        (lit_int(1), None, body()),
    ];
    assert_eq!(unreachable_arms(&arms), vec![1]);

    // Duplicate literal: (match x (1 a) (1 b) (_ c)) → arm 1 unreachable
    let arms = vec![
        (lit_int(1), None, body()),
        (lit_int(1), None, body()),
        (HirPattern::Wildcard, None, body()),
    ];
    assert_eq!(unreachable_arms(&arms), vec![1]);

    // Guarded catch-all keeps later arms reachable, and the guarded
    // arm itself stays reachable (Guard node).
    let arms = vec![
        (HirPattern::Wildcard, Some(body()), body()),
        (lit_int(1), None, body()),
    ];
    assert_eq!(unreachable_arms(&arms), Vec::<usize>::new());

    // Or-pattern arm with one live alternative is reachable:
    // (match x (1 a) ((or 1 2) b))
    let arms = vec![
        (lit_int(1), None, body()),
        (HirPattern::Or(vec![lit_int(1), lit_int(2)]), None, body()),
    ];
    assert_eq!(unreachable_arms(&arms), Vec::<usize>::new());

    // Or-pattern arm with every alternative dead is unreachable:
    // (match x (1 a) (2 b) ((or 1 2) c))
    let arms = vec![
        (lit_int(1), None, body()),
        (lit_int(2), None, body()),
        (HirPattern::Or(vec![lit_int(1), lit_int(2)]), None, body()),
    ];
    assert_eq!(unreachable_arms(&arms), vec![2]);
}
#[test]
fn test_dead_or_alternatives() {
    use crate::syntax::Span;
    let body = || Hir::silent(crate::hir::HirKind::Bool(true), Span::synthetic());
    let or = |alts: Vec<HirPattern>| HirPattern::Or(alts);
    let pair = |head: HirPattern, tail: HirPattern| HirPattern::Pair {
        head: Box::new(head),
        tail: Box::new(tail),
    };
    let dead = |arms: &[(HirPattern, Option<Hir>, Hir)]| {
        first_dead_alternative(arms).map(|d| (d.arm, d.alternative))
    };

    // Alternative covered by an earlier arm:
    // (match x (1 a) ((or 1 2) b)) → arm 1, alternative 0 dead
    let arms = vec![
        (lit_int(1), None, body()),
        (or(vec![lit_int(1), lit_int(2)]), None, body()),
    ];
    assert_eq!(dead(&arms), Some((1, 0)));

    // Duplicate alternative: ((or 1 1) a) → alternative 1 dead
    let arms = vec![(or(vec![lit_int(1), lit_int(1)]), None, body())];
    assert_eq!(dead(&arms), Some((0, 1)));

    // Alternative after a wildcard alternative: ((or _ 1) a) → alt 1 dead
    let arms = vec![(or(vec![HirPattern::Wildcard, lit_int(1)]), None, body())];
    assert_eq!(dead(&arms), Some((0, 1)));

    // Nested: ((1 . (or _ 2)) a) → inner alternative 1 dead
    let arms = vec![(
        pair(lit_int(1), or(vec![HirPattern::Wildcard, lit_int(2)])),
        None,
        body(),
    )];
    assert_eq!(dead(&arms), Some((0, 1)));

    // Both alternatives match every pair: ((or (x . _) (_ . x)) a) → alt 1 dead
    let arms = vec![(
        or(vec![
            pair(var(0), HirPattern::Wildcard),
            pair(HirPattern::Wildcard, var(0)),
        ]),
        None,
        body(),
    )];
    assert_eq!(dead(&arms), Some((0, 1)));

    // Sibling or-patterns are independent: (((or 1 2) . (or :a :b)) x) → all alive
    let arms = vec![(
        pair(
            or(vec![lit_int(1), lit_int(2)]),
            or(vec![lit_kw("a"), lit_kw("b")]),
        ),
        None,
        body(),
    )];
    assert_eq!(dead(&arms), None);

    // A guarded earlier arm never fully covers: alternatives stay alive
    let arms = vec![
        (HirPattern::Wildcard, Some(body()), body()),
        (or(vec![lit_int(1), lit_int(2)]), None, body()),
    ];
    assert_eq!(dead(&arms), None);

    // Nested under an or-alternative: the enclosing choice is pinned, so
    // a later catch-all alternative does not hide the inner dead one:
    // ((or (1 . (or :a :a)) x) body) → inner alternative 1 dead
    let arms = vec![(
        or(vec![
            pair(lit_int(1), or(vec![lit_kw("a"), lit_kw("a")])),
            var(0),
        ]),
        None,
        body(),
    )];
    assert_eq!(dead(&arms), Some((0, 1)));

    // Earlier alternatives of an ANCESTOR or-node count as coverage:
    // ((or 1 (or 1 2)) a) → the inner 1 (alternative 0 of the inner
    // or-node) is dead via the outer alternative 0
    let arms = vec![(
        or(vec![lit_int(1), or(vec![lit_int(1), lit_int(2)])]),
        None,
        body(),
    )];
    assert_eq!(dead(&arms), Some((0, 0)));

    // A guarded arm's alternatives are never killed by each other: a
    // failed guard retries later alternatives with fresh bindings.
    // ((or (x . _) (_ . x)) when g body) → all alive
    let arms = vec![(
        or(vec![
            pair(var(0), HirPattern::Wildcard),
            pair(HirPattern::Wildcard, var(0)),
        ]),
        Some(body()),
        body(),
    )];
    assert_eq!(dead(&arms), None);

    // ...but earlier ARMS still kill a guarded arm's alternatives:
    // (match x ((_ . _) a) ((or (y . _) 1) when g b)) → alt 0 dead
    let arms = vec![
        (
            pair(HirPattern::Wildcard, HirPattern::Wildcard),
            None,
            body(),
        ),
        (
            or(vec![pair(var(0), HirPattern::Wildcard), lit_int(1)]),
            Some(body()),
            body(),
        ),
    ];
    assert_eq!(dead(&arms), Some((1, 0)));
}
#[test]
fn child_order_agreement() {
    // children() and with_child() must agree on child indexing: for
    // every child slot of every compound variant, replacing slot i and
    // re-reading slot i yields the replacement.
    let marker = || lit_kw("marker");
    let w = || HirPattern::Wildcard;
    let samples = vec![
        HirPattern::Pair {
            head: Box::new(w()),
            tail: Box::new(w()),
        },
        HirPattern::List {
            elements: vec![w(), w()],
            rest: Some(Box::new(w())),
        },
        HirPattern::Tuple {
            elements: vec![w()],
            rest: Some(Box::new(w())),
        },
        HirPattern::Array {
            elements: vec![w(), w(), w()],
            rest: None,
        },
        HirPattern::Struct {
            entries: vec![
                (PatternKey::Keyword("a".into()), w()),
                (PatternKey::Keyword("b".into()), w()),
            ],
            rest: Some(Box::new(w())),
        },
        HirPattern::Table {
            entries: vec![(PatternKey::Keyword("k".into()), w())],
            rest: None,
        },
        HirPattern::NamedStruct {
            entries: vec![(PatternKey::Keyword("n".into()), w())],
        },
        HirPattern::Set {
            binding: Box::new(w()),
        },
        HirPattern::SetMut {
            binding: Box::new(w()),
        },
    ];
    for sample in &samples {
        let n = super::redundancy::children_for_test(sample).len();
        assert!(n > 0, "compound variant reports no children: {:?}", sample);
        for i in 0..n {
            let rebuilt = super::redundancy::with_child_for_test(sample, i, marker());
            let read_back = super::redundancy::children_for_test(&rebuilt)[i];
            assert!(
                matches!(
                    read_back,
                    HirPattern::Literal(PatternLiteral::Keyword(k)) if k == "marker"
                ),
                "children()/with_child() disagree at slot {} of {:?}: got {:?}",
                i,
                sample,
                read_back
            );
        }
    }
}
#[test]
fn test_guard_arm_not_unreachable() {
    // Guard arm before same pattern without guard → both reachable
    // (guard may fail, so the second arm is reachable)

    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![HirPattern::Wildcard], true, 0),
            PatternRow::new(vec![HirPattern::Wildcard], false, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    let reachable = find_reachable_arms(&tree);
    assert!(reachable.contains(&0));
    assert!(reachable.contains(&1));
}
#[test]
fn test_or_pattern_in_matrix() {
    // Or-pattern should be expanded into multiple rows in from_arms.
    // We simulate this by constructing the matrix directly with
    // an or-pattern that was NOT expanded (to test specialize).
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![HirPattern::Or(vec![lit_int(1), lit_int(2)])], false, 0),
            PatternRow::new(vec![HirPattern::Wildcard], false, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    // The or-pattern is not a wildcard, so it should trigger
    // specialization. The constructors should be Int(1) and Int(2).
    let reachable = find_reachable_arms(&tree);
    assert!(reachable.contains(&0));
    assert!(reachable.contains(&1));
}
