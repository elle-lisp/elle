use super::*;
use crate::hir::decision::*;

#[test]
fn test_single_wildcard() {
    // Single arm: (_ body) → Leaf { arm_index: 0 }
    let matrix = PatternMatrix {
        rows: vec![PatternRow::new(vec![HirPattern::Wildcard], false, 0)],
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
            PatternRow::new(vec![lit_int(1)], false, 0),
            PatternRow::new(vec![lit_int(2)], false, 1),
            PatternRow::new(vec![HirPattern::Wildcard], false, 2),
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
                false,
                0,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], false, 1),
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
fn test_guard_node() {
    // A guarded all-wildcard row produces a Guard node.
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![HirPattern::Wildcard], true, 0),
            PatternRow::new(vec![HirPattern::Wildcard], false, 1),
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
                false,
                0,
            ),
            PatternRow::new(
                vec![HirPattern::Pair {
                    head: Box::new(lit_int(2)),
                    tail: Box::new(HirPattern::Wildcard),
                }],
                false,
                1,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], false, 2),
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
            PatternRow::new(vec![HirPattern::Nil], false, 0),
            PatternRow::new(vec![HirPattern::Wildcard], false, 1),
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
                false,
                0,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], false, 1),
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
                false,
                0,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], false, 1),
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
                false,
                0,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], false, 1),
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
                false,
                0,
            ),
            PatternRow::new(vec![HirPattern::Wildcard], false, 1),
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
fn test_empty_matrix_produces_fail() {
    let matrix = PatternMatrix { rows: vec![] };
    let tree = matrix.compile(vec![AccessPath::Root]);
    assert!(matches!(tree, DecisionTree::Fail));
}
#[test]
fn test_keyword_literals_distinct() {
    // (match x (:a ...) (:b ...) (_ ...))
    let matrix = PatternMatrix {
        rows: vec![
            PatternRow::new(vec![lit_kw("a")], false, 0),
            PatternRow::new(vec![lit_kw("b")], false, 1),
            PatternRow::new(vec![HirPattern::Wildcard], false, 2),
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
fn test_var_binding_collected() {
    // A variable pattern should produce a binding in the Leaf.
    use crate::hir::arena::{BindingArena, BindingScope};
    use crate::value::SymbolId;

    let mut arena = BindingArena::new();
    let binding = arena.alloc(SymbolId(42), BindingScope::Local);
    let matrix = PatternMatrix {
        rows: vec![PatternRow::new(vec![HirPattern::Var(binding)], false, 0)],
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
