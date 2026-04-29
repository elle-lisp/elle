//! Tests for decision tree compilation.

use super::*;
use crate::hir::{HirPattern, PatternLiteral};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

// Helper: create a literal int pattern.
fn lit_int(n: i64) -> HirPattern {
    HirPattern::Literal(PatternLiteral::Int(n))
}

// Helper: create a keyword pattern.
fn lit_kw(s: &str) -> HirPattern {
    HirPattern::Literal(PatternLiteral::Keyword(s.to_string()))
}

#[test]
fn test_single_wildcard() {
    // Single arm: (_ body) → Leaf { arm_index: 0 }
    let matrix = PatternMatrix {
        rows: vec![PatternRow::new(vec![HirPattern::Wildcard], None, 0)],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    match tree {
        DecisionTree::Leaf {
            arm_index,
            bindings,
        } => {
            assert_eq!(arm_index, 0);
            assert!(bindings.is_empty());
        }
        _ => panic!("expected Leaf, got {:?}", tree),
    }
}

#[test]
fn test_two_literals() {
    // (match x (1 ...) (2 ...) (_ ...))
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![lit_int(1)], None, 0),
            PatternRow::new(vec![lit_int(2)], None, 1),
            PatternRow::new(vec![HirPattern::Wildcard], None, 2),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    match &tree {
        DecisionTree::Switch { cases, default, .. } => {
            assert_eq!(cases.len(), 2);
            assert_eq!(cases[0].0, Constructor::Literal(PatternLiteral::Int(1)));
            assert_eq!(cases[1].0, Constructor::Literal(PatternLiteral::Int(2)));
            assert!(default.is_some());
            // Default should be a Leaf for arm 2
            match default.as_deref().unwrap() {
                DecisionTree::Leaf { arm_index, .. } => assert_eq!(*arm_index, 2),
                _ => panic!("expected Leaf default"),
            }
        }
        _ => panic!("expected Switch, got {:?}", tree),
    }
}

#[test]
fn test_cons_pattern() {
    // (match x ((h . t) ...) (_ ...))
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(
                vec![HirPattern::Pair {
                    head: Box::new(HirPattern::Wildcard),
                    tail: Box::new(HirPattern::Wildcard),
                }],
                None,
                0,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], None, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    match &tree {
        DecisionTree::Switch { cases, default, .. } => {
            assert_eq!(cases.len(), 1);
            assert_eq!(cases[0].0, Constructor::Pair);
            assert!(default.is_some());
        }
        _ => panic!("expected Switch, got {:?}", tree),
    }
}

#[test]
fn test_or_pattern_expansion() {
    // Or(1, 2, 3) should expand to 3 patterns
    let or_pat = HirPattern::Or(vec![lit_int(1), lit_int(2), lit_int(3)]);
    let expanded = expand_or_pattern(&or_pat);
    assert_eq!(expanded.len(), 3);
}

#[test]
fn test_guard_node() {
    // A row with guard and all-wildcard patterns produces a Guard node.
    // We use a dummy Hir for the guard.
    use crate::syntax::Span;
    let dummy_guard = Hir::silent(crate::hir::HirKind::Bool(true), Span::synthetic());

    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![HirPattern::Wildcard], Some(dummy_guard), 0),
            PatternRow::new(vec![HirPattern::Wildcard], None, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    match &tree {
        DecisionTree::Guard {
            arm_index,
            otherwise,
            ..
        } => {
            assert_eq!(*arm_index, 0);
            match otherwise.as_ref() {
                DecisionTree::Leaf { arm_index, .. } => assert_eq!(*arm_index, 1),
                _ => panic!("expected Leaf otherwise"),
            }
        }
        _ => panic!("expected Guard, got {:?}", tree),
    }
}

#[test]
fn test_reachable_arms() {
    // Two distinct literals + wildcard → all 3 arms reachable
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![lit_int(1)], None, 0),
            PatternRow::new(vec![lit_int(2)], None, 1),
            PatternRow::new(vec![HirPattern::Wildcard], None, 2),
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
            PatternRow::new(vec![HirPattern::Wildcard], None, 0),
            PatternRow::new(vec![lit_int(1)], None, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    let reachable = find_reachable_arms(&tree);
    assert!(reachable.contains(&0));
    assert!(!reachable.contains(&1));
}

#[test]
fn test_nested_patterns() {
    // (match x ((1 . _) ...) ((2 . _) ...) (_ ...))
    // Should produce a Switch on Root (IsPair), then inside the Pair
    // case, a Switch on First(Root) for the literal values.
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(
                vec![HirPattern::Pair {
                    head: Box::new(lit_int(1)),
                    tail: Box::new(HirPattern::Wildcard),
                }],
                None,
                0,
            ),
            PatternRow::new(
                vec![HirPattern::Pair {
                    head: Box::new(lit_int(2)),
                    tail: Box::new(HirPattern::Wildcard),
                }],
                None,
                1,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], None, 2),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);

    // Top level: Switch on Root for Pair
    match &tree {
        DecisionTree::Switch {
            access,
            cases,
            default,
        } => {
            assert_eq!(*access, AccessPath::Root);
            assert_eq!(cases.len(), 1); // One constructor: Pair
            assert_eq!(cases[0].0, Constructor::Pair);
            assert!(default.is_some());

            // Inside the Pair case: Switch on First(Root) for literals
            match &cases[0].1 {
                DecisionTree::Switch {
                    access,
                    cases: inner_cases,
                    ..
                } => {
                    assert_eq!(*access, AccessPath::First(Box::new(AccessPath::Root)));
                    assert_eq!(inner_cases.len(), 2);
                    assert_eq!(
                        inner_cases[0].0,
                        Constructor::Literal(PatternLiteral::Int(1))
                    );
                    assert_eq!(
                        inner_cases[1].0,
                        Constructor::Literal(PatternLiteral::Int(2))
                    );
                }
                _ => panic!("expected nested Switch"),
            }
        }
        _ => panic!("expected Switch, got {:?}", tree),
    }
}

#[test]
fn test_nil_pattern() {
    // (match x (nil ...) (_ ...))
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![HirPattern::Nil], None, 0),
            PatternRow::new(vec![HirPattern::Wildcard], None, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    match &tree {
        DecisionTree::Switch { cases, .. } => {
            assert_eq!(cases.len(), 1);
            assert_eq!(cases[0].0, Constructor::Nil);
        }
        _ => panic!("expected Switch"),
    }
}

#[test]
fn test_empty_list_pattern() {
    // (match x (() ...) (_ ...))
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(
                vec![HirPattern::List {
                    elements: vec![],
                    rest: None,
                }],
                None,
                0,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], None, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    match &tree {
        DecisionTree::Switch { cases, .. } => {
            assert_eq!(cases.len(), 1);
            assert_eq!(cases[0].0, Constructor::EmptyList);
        }
        _ => panic!("expected Switch"),
    }
}

#[test]
fn test_list_pattern_as_cons_chain() {
    // (match x ((a b) ...) (_ ...))
    // A 2-element list pattern should decompose as Pair at the top level.
    use crate::hir::arena::{BindingArena, BindingScope};
    use crate::value::SymbolId;

    let mut arena = BindingArena::new();
    let binding_a = arena.alloc(SymbolId(0), BindingScope::Local);
    let binding_b = arena.alloc(SymbolId(1), BindingScope::Local);

    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(
                vec![HirPattern::List {
                    elements: vec![HirPattern::Var(binding_a), HirPattern::Var(binding_b)],
                    rest: None,
                }],
                None,
                0,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], None, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);

    // Top level should be Switch with Pair constructor
    match &tree {
        DecisionTree::Switch { cases, .. } => {
            assert_eq!(cases[0].0, Constructor::Pair);
        }
        _ => panic!("expected Switch"),
    }
}

#[test]
fn test_tuple_pattern() {
    // (match x ([1 2] ...) (_ ...))
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(
                vec![HirPattern::Tuple {
                    elements: vec![lit_int(1), lit_int(2)],
                    rest: None,
                }],
                None,
                0,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], None, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    match &tree {
        DecisionTree::Switch { cases, .. } => {
            assert_eq!(cases.len(), 1);
            assert_eq!(cases[0].0, Constructor::Array(2));
        }
        _ => panic!("expected Switch"),
    }
}

#[test]
fn test_struct_pattern() {
    // (match x ({:x _ :y _} ...) (_ ...))
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(
                vec![HirPattern::Struct {
                    entries: vec![
                        (PatternKey::Keyword("x".to_string()), HirPattern::Wildcard),
                        (PatternKey::Keyword("y".to_string()), HirPattern::Wildcard),
                    ],
                    rest: None,
                }],
                None,
                0,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], None, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    match &tree {
        DecisionTree::Switch { cases, .. } => {
            assert_eq!(
                cases[0].0,
                Constructor::Struct(vec![
                    PatternKey::Keyword("x".to_string()),
                    PatternKey::Keyword("y".to_string()),
                ])
            );
        }
        _ => panic!("expected Switch"),
    }
}

#[test]
fn test_guard_arm_not_unreachable() {
    // Guard arm before same pattern without guard → both reachable
    // (guard may fail, so the second arm is reachable)
    use crate::syntax::Span;
    let dummy_guard = Hir::silent(crate::hir::HirKind::Bool(true), Span::synthetic());

    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![HirPattern::Wildcard], Some(dummy_guard), 0),
            PatternRow::new(vec![HirPattern::Wildcard], None, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    let reachable = find_reachable_arms(&tree);
    assert!(reachable.contains(&0));
    assert!(reachable.contains(&1));
}

// ── Hash/Eq consistency tests ──────────────────────────────────────

#[test]
fn test_pattern_literal_eq_implies_same_hash() {
    // Equal values must produce equal hashes (Hash contract).
    let pairs: &[(PatternLiteral, PatternLiteral)] = &[
        (PatternLiteral::Bool(true), PatternLiteral::Bool(true)),
        (PatternLiteral::Bool(false), PatternLiteral::Bool(false)),
        (PatternLiteral::Int(42), PatternLiteral::Int(42)),
        (PatternLiteral::Int(-1), PatternLiteral::Int(-1)),
        (PatternLiteral::Float(1.5), PatternLiteral::Float(1.5)),
        (
            PatternLiteral::String("hello".into()),
            PatternLiteral::String("hello".into()),
        ),
        (
            PatternLiteral::Keyword("ok".into()),
            PatternLiteral::Keyword("ok".into()),
        ),
    ];
    for (a, b) in pairs {
        assert_eq!(a, b, "equality failed for {:?}", a);
        assert_eq!(
            hash_of(a),
            hash_of(b),
            "hash mismatch for equal values {:?}",
            a
        );
    }
}

#[test]
fn test_pattern_literal_float_uses_bits() {
    // Hash must be derived from bit representation, not f64 equality.
    // Two values constructed from the same bits must have the same hash,
    // even for values that don't satisfy f64 equality (e.g. NaN).
    let nan = f64::NAN;
    let a = PatternLiteral::Float(nan);
    let b = PatternLiteral::Float(f64::from_bits(nan.to_bits()));
    // NaN != NaN under PartialEq, so we do NOT assert_eq!(a, b).
    // But their hashes must agree because to_bits() is the same.
    assert_eq!(hash_of(&a), hash_of(&b));

    // Normal floats: same bits → same hash, and they are also PartialEq-equal.
    let x = PatternLiteral::Float(2.5);
    let y = PatternLiteral::Float(2.5);
    assert_eq!(x, y);
    assert_eq!(hash_of(&x), hash_of(&y));
}

#[test]
fn test_pattern_literal_distinct_variants_differ() {
    // Different variants must not be equal (and their hashes are allowed to differ).
    assert_ne!(PatternLiteral::Int(1), PatternLiteral::Float(1.0));
    assert_ne!(
        PatternLiteral::String("x".into()),
        PatternLiteral::Keyword("x".into())
    );
}

#[test]
fn test_constructor_eq_implies_same_hash() {
    let pairs: &[(Constructor, Constructor)] = &[
        (Constructor::Nil, Constructor::Nil),
        (Constructor::EmptyList, Constructor::EmptyList),
        (Constructor::Pair, Constructor::Pair),
        (Constructor::Set, Constructor::Set),
        (Constructor::SetMut, Constructor::SetMut),
        (Constructor::Array(3), Constructor::Array(3)),
        (Constructor::ArrayMut(0), Constructor::ArrayMut(0)),
        (Constructor::ArrayRest(2), Constructor::ArrayRest(2)),
        (Constructor::ArrayMutRest(1), Constructor::ArrayMutRest(1)),
        (
            Constructor::Literal(PatternLiteral::Int(7)),
            Constructor::Literal(PatternLiteral::Int(7)),
        ),
        (
            Constructor::Literal(PatternLiteral::Float(2.5)),
            Constructor::Literal(PatternLiteral::Float(2.5)),
        ),
        (
            Constructor::Struct(vec![PatternKey::Keyword("a".into())]),
            Constructor::Struct(vec![PatternKey::Keyword("a".into())]),
        ),
        (
            Constructor::Table(vec![PatternKey::Keyword("b".into())]),
            Constructor::Table(vec![PatternKey::Keyword("b".into())]),
        ),
    ];
    for (a, b) in pairs {
        assert_eq!(a, b, "equality failed for {:?}", a);
        assert_eq!(
            hash_of(a),
            hash_of(b),
            "hash mismatch for equal Constructor {:?}",
            a
        );
    }
}

#[test]
fn test_constructor_float_literal_hash_consistency() {
    // The tricky case: float literals inside Constructor::Literal.
    // Use a value that won't trigger clippy::approx_constant.
    let v = 1.23456789_f64;
    let a = Constructor::Literal(PatternLiteral::Float(v));
    let b = Constructor::Literal(PatternLiteral::Float(v));
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
}

#[test]
fn test_constructor_in_hashset() {
    // Constructors can be stored and looked up in a HashSet correctly.
    let mut set = std::collections::HashSet::new();
    set.insert(Constructor::Literal(PatternLiteral::Int(1)));
    set.insert(Constructor::Literal(PatternLiteral::Int(2)));
    set.insert(Constructor::Pair);
    set.insert(Constructor::Pair); // duplicate — should not grow the set

    assert_eq!(set.len(), 3);
    assert!(set.contains(&Constructor::Literal(PatternLiteral::Int(1))));
    assert!(set.contains(&Constructor::Literal(PatternLiteral::Int(2))));
    assert!(set.contains(&Constructor::Pair));
    assert!(!set.contains(&Constructor::Nil));
}

#[test]
fn test_empty_matrix_produces_fail() {
    let matrix = PatternMatrix { rows: vec![] };
    let tree = matrix.compile(vec![AccessPath::Root]);
    assert!(matches!(tree, DecisionTree::Fail));
}

#[test]
fn test_constructor_arity() {
    assert_eq!(Constructor::Literal(PatternLiteral::Int(1)).arity(), 0);
    assert_eq!(Constructor::Nil.arity(), 0);
    assert_eq!(Constructor::EmptyList.arity(), 0);
    assert_eq!(Constructor::Pair.arity(), 2);
    assert_eq!(Constructor::Array(3).arity(), 3);
    assert_eq!(Constructor::ArrayMut(2).arity(), 2);
    assert_eq!(
        Constructor::Struct(vec![
            PatternKey::Keyword("a".into()),
            PatternKey::Keyword("b".into())
        ])
        .arity(),
        2
    );
    assert_eq!(
        Constructor::Table(vec![PatternKey::Keyword("x".into())]).arity(),
        1
    );
}

#[test]
fn test_keyword_literals_distinct() {
    // (match x (:a ...) (:b ...) (_ ...))
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![lit_kw("a")], None, 0),
            PatternRow::new(vec![lit_kw("b")], None, 1),
            PatternRow::new(vec![HirPattern::Wildcard], None, 2),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    match &tree {
        DecisionTree::Switch { cases, .. } => {
            assert_eq!(cases.len(), 2);
            assert_eq!(
                cases[0].0,
                Constructor::Literal(PatternLiteral::Keyword("a".to_string()))
            );
            assert_eq!(
                cases[1].0,
                Constructor::Literal(PatternLiteral::Keyword("b".to_string()))
            );
        }
        _ => panic!("expected Switch"),
    }
}

#[test]
fn test_or_pattern_in_matrix() {
    // Or-pattern should be expanded into multiple rows in from_arms.
    // We simulate this by constructing the matrix directly with
    // an or-pattern that was NOT expanded (to test specialize).
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![HirPattern::Or(vec![lit_int(1), lit_int(2)])], None, 0),
            PatternRow::new(vec![HirPattern::Wildcard], None, 1),
        ],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    // The or-pattern is not a wildcard, so it should trigger
    // specialization. The constructors should be Int(1) and Int(2).
    let reachable = find_reachable_arms(&tree);
    assert!(reachable.contains(&0));
    assert!(reachable.contains(&1));
}

#[test]
fn test_var_binding_collected() {
    // A variable pattern should produce a binding in the Leaf.
    use crate::hir::arena::{BindingArena, BindingScope};
    use crate::value::SymbolId;

    let mut arena = BindingArena::new();
    let binding = arena.alloc(SymbolId(42), BindingScope::Local);
    let matrix = PatternMatrix {
        rows: vec![PatternRow::new(vec![HirPattern::Var(binding)], None, 0)],
    };
    let tree = matrix.compile(vec![AccessPath::Root]);
    match &tree {
        DecisionTree::Leaf {
            arm_index,
            bindings,
        } => {
            assert_eq!(*arm_index, 0);
            assert_eq!(bindings.len(), 1);
            assert_eq!(bindings[0].0, binding);
            assert_eq!(bindings[0].1, AccessPath::Root);
        }
        _ => panic!("expected Leaf with binding"),
    }
}
