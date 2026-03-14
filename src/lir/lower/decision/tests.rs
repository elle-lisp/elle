//! Tests for decision tree compilation.

use super::*;
use crate::hir::{HirPattern, PatternLiteral};

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
                vec![HirPattern::Cons {
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
            assert_eq!(cases[0].0, Constructor::Cons);
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
    // Should produce a Switch on Root (IsPair), then inside the Cons
    // case, a Switch on Car(Root) for the literal values.
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(
                vec![HirPattern::Cons {
                    head: Box::new(lit_int(1)),
                    tail: Box::new(HirPattern::Wildcard),
                }],
                None,
                0,
            ),
            PatternRow::new(
                vec![HirPattern::Cons {
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

    // Top level: Switch on Root for Cons
    match &tree {
        DecisionTree::Switch {
            access,
            cases,
            default,
        } => {
            assert_eq!(*access, AccessPath::Root);
            assert_eq!(cases.len(), 1); // One constructor: Cons
            assert_eq!(cases[0].0, Constructor::Cons);
            assert!(default.is_some());

            // Inside the Cons case: Switch on Car(Root) for literals
            match &cases[0].1 {
                DecisionTree::Switch {
                    access,
                    cases: inner_cases,
                    ..
                } => {
                    assert_eq!(*access, AccessPath::Car(Box::new(AccessPath::Root)));
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
    // A 2-element list pattern should decompose as Cons at the top level.
    use crate::hir::Binding;
    use crate::value::heap::BindingScope;
    use crate::value::SymbolId;

    let binding_a = Binding::new(SymbolId(0), BindingScope::Local);
    let binding_b = Binding::new(SymbolId(1), BindingScope::Local);

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

    // Top level should be Switch with Cons constructor
    match &tree {
        DecisionTree::Switch { cases, .. } => {
            assert_eq!(cases[0].0, Constructor::Cons);
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
    assert_eq!(Constructor::Cons.arity(), 2);
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
    use crate::hir::Binding;
    use crate::value::heap::BindingScope;
    use crate::value::SymbolId;

    let binding = Binding::new(SymbolId(42), BindingScope::Local);
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
