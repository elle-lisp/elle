//! Decision tree compilation for pattern matching.
//!
//! Implements Maranget's algorithm: "Compiling Pattern Matching to Good
//! Decision Trees" (2008). Converts a pattern matrix into a decision tree
//! that eliminates redundant checks when multiple arms share pattern
//! prefixes.
//!
//! This module is self-contained: it takes `HirPattern` as input and
//! produces a `DecisionTree` as output. No LIR dependencies — the tree
//! is lowered to LIR in a separate step.

// COUPLING: This module is consumed by `hir/analyze/special.rs`
// (arm-reachability check), `lir/lower/control.rs` (builds the
// decision tree), and `lir/lower/pattern.rs` (lowers it to LIR).

use crate::hir::{Binding, Hir, HirPattern, PatternKey, PatternLiteral};
use std::collections::HashSet;

mod algo;
use algo::*;
mod constructors;
use constructors::*;
mod redundancy;
pub(crate) use redundancy::first_dead_alternative;

// ── Data types ─────────────────────────────────────────────────────

/// How to reach a sub-value of the scrutinee.
///
/// `Root` is the scrutinee itself. Each variant descends one level
/// into a compound value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum AccessPath {
    /// The scrutinee itself.
    Root,
    /// First (head) of a cons cell at the given path.
    First(Box<AccessPath>),
    /// Rest (tail) of a cons cell at the given path.
    Rest(Box<AccessPath>),
    /// Element at index `i` of an array at the given path.
    Index(Box<AccessPath>, usize),
    /// Slice from index `i` to end of an array at the given path.
    /// Used for `& rest` patterns in array destructuring.
    Slice(Box<AccessPath>, usize),
    /// Value at keyword key in a struct at the given path.
    Key(Box<AccessPath>, PatternKey),
    /// All keys NOT in the given set, collected from a struct at the given path.
    /// Used for `& rest` patterns in struct destructuring.
    StructRest(Box<AccessPath>, Vec<PatternKey>),
}

/// A constructor represents the "shape" that a pattern tests for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Constructor {
    /// Literal value (int, float, string, keyword, bool).
    Literal(PatternLiteral),
    /// Pair cell (pair).
    Pair,
    /// Nil literal.
    Nil,
    /// Empty list `()`.
    EmptyList,
    /// Immutable array of exactly `n` elements.
    Array(usize),
    /// Immutable array of at least `n` fixed elements (has `& rest`).
    ArrayRest(usize),
    /// Mutable array of exactly `n` elements.
    ArrayMut(usize),
    /// Mutable array of at least `n` fixed elements (has `& rest`).
    ArrayMutRest(usize),
    /// Struct with these keys (open match — presence, not exclusivity).
    Struct(Vec<PatternKey>),
    /// @Struct with these keys (open match).
    Table(Vec<PatternKey>),
    /// Immutable set (type guard only, arity 1 — the binding gets the whole value).
    Set,
    /// Mutable set (type guard only, arity 1 — the binding gets the whole value).
    SetMut,
}

impl Eq for Constructor {}

impl std::hash::Hash for Constructor {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Constructor::Literal(lit) => lit.hash(state),
            Constructor::Array(n)
            | Constructor::ArrayRest(n)
            | Constructor::ArrayMut(n)
            | Constructor::ArrayMutRest(n) => n.hash(state),
            Constructor::Struct(keys) | Constructor::Table(keys) => keys.hash(state),
            Constructor::Pair
            | Constructor::Nil
            | Constructor::EmptyList
            | Constructor::Set
            | Constructor::SetMut => {}
        }
    }
}

impl Constructor {
    /// Number of sub-patterns this constructor expands to.
    pub fn arity(&self) -> usize {
        match self {
            Constructor::Literal(_) | Constructor::Nil | Constructor::EmptyList => 0,
            Constructor::Pair => 2,
            Constructor::Array(n) | Constructor::ArrayMut(n) => *n,
            // Rest variants include the rest element as an extra sub-pattern.
            Constructor::ArrayRest(n) | Constructor::ArrayMutRest(n) => *n + 1,
            Constructor::Struct(keys) | Constructor::Table(keys) => keys.len(),
            Constructor::Set | Constructor::SetMut => 1,
        }
    }
}

/// A row in the pattern matrix: one match arm (or one or-pattern expansion).
#[derive(Debug, Clone)]
pub struct PatternRow {
    /// Patterns for each column (initially one: the scrutinee).
    pub patterns: Vec<HirPattern>,
    /// Whether the arm has a guard. The guard expression itself is never
    /// needed here — lowering fetches it from the arm by `arm_index` —
    /// and carrying just the flag keeps matrix recursion clone-free.
    pub has_guard: bool,
    /// Index into the original arms vec (for body lookup).
    pub arm_index: usize,
    /// Bindings accumulated from `Var` patterns in columns that were
    /// removed during specialization or default-matrix construction.
    /// These are carried forward so the Leaf node includes them.
    pub bindings: Vec<(Binding, AccessPath)>,
}

impl PatternRow {
    /// Create a new row with no accumulated bindings.
    pub fn new(patterns: Vec<HirPattern>, has_guard: bool, arm_index: usize) -> Self {
        PatternRow {
            patterns,
            has_guard,
            arm_index,
            bindings: Vec::new(),
        }
    }
}

/// The pattern matrix used by Maranget's algorithm.
#[derive(Debug)]
pub struct PatternMatrix {
    pub rows: Vec<PatternRow>,
}

/// The compiled decision tree.
#[derive(Debug)]
pub enum DecisionTree {
    /// Matched: execute the arm body.
    Leaf {
        arm_index: usize,
        bindings: Vec<(Binding, AccessPath)>,
    },
    /// No arms matched.
    Fail,
    /// Switch on the value at `access`.
    /// Each case tests a constructor and recurses.
    /// `default` handles values that don't match any case.
    Switch {
        access: AccessPath,
        cases: Vec<(Constructor, DecisionTree)>,
        default: Option<Box<DecisionTree>>,
    },
    /// Guard check: bindings are established, guard is evaluated.
    /// If the guard passes, execute the arm body; otherwise continue
    /// with `otherwise`.
    Guard {
        arm_index: usize,
        bindings: Vec<(Binding, AccessPath)>,
        otherwise: Box<DecisionTree>,
    },
}

// ── Or-pattern expansion ───────────────────────────────────────────

/// Expand top-level or-patterns into individual patterns.
pub fn expand_or_pattern(pattern: &HirPattern) -> Vec<HirPattern> {
    match pattern {
        HirPattern::Or(alts) => alts.iter().flat_map(expand_or_pattern).collect(),
        _ => vec![pattern.clone()],
    }
}

// ── PatternMatrix construction ─────────────────────────────────────

impl PatternMatrix {
    /// Create a pattern matrix from HIR match arms.
    /// Or-patterns are expanded into multiple rows.
    pub fn from_arms(arms: &[(HirPattern, Option<Hir>, Hir)]) -> Self {
        let mut rows = Vec::new();
        for (i, (pattern, guard, _body)) in arms.iter().enumerate() {
            for expanded in expand_or_pattern(pattern) {
                rows.push(PatternRow::new(vec![expanded], guard.is_some(), i));
            }
        }
        PatternMatrix { rows }
    }

    /// Compile the matrix into a decision tree.
    pub fn compile(self, col_access: Vec<AccessPath>) -> DecisionTree {
        compile_matrix(self, col_access)
    }
}

/// Indices of arms no value can reach: arms present in no Leaf or Guard
/// node of the compiled decision tree. Guarded arms are reachable
/// whenever they appear as a Guard node — guard truth is undecidable
/// statically. Or-pattern arms share one index across alternatives, so
/// an arm is unreachable only when every alternative is dead.
pub(crate) fn unreachable_arms(arms: &[(HirPattern, Option<Hir>, Hir)]) -> Vec<usize> {
    let tree = PatternMatrix::from_arms(arms).compile(vec![AccessPath::Root]);
    let reachable = find_reachable_arms(&tree);
    (0..arms.len()).filter(|i| !reachable.contains(i)).collect()
}

// ── Pattern classification ─────────────────────────────────────────

#[cfg(test)]
mod tests;
