//! Decision tree compilation for pattern matching.
//!
//! Implements Maranget's algorithm: "Compiling Pattern Matching to Good
//! Decision Trees" (2008). Converts a pattern matrix into a decision tree
//! that eliminates redundant checks when multiple arms share pattern
//! prefixes.
//!
//! This module is self-contained: it takes `HirPattern` as input and
//! produces a `DecisionTree` as output. No LIR dependencies — the tree
//! is lowered to LIR in a separate step (Chunk 6b).

// COUPLING: This module is consumed by `lower/control.rs` (builds
// the decision tree) and `lower/pattern.rs` (lowers it to LIR).

use crate::hir::{Binding, Hir, HirPattern, PatternKey, PatternLiteral};
use std::collections::HashSet;

// ── Data types ─────────────────────────────────────────────────────

/// How to reach a sub-value of the scrutinee.
///
/// `Root` is the scrutinee itself. Each variant descends one level
/// into a compound value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum AccessPath {
    /// The scrutinee itself.
    Root,
    /// Car (head) of a cons cell at the given path.
    Car(Box<AccessPath>),
    /// Cdr (tail) of a cons cell at the given path.
    Cdr(Box<AccessPath>),
    /// Element at index `i` of an array at the given path.
    Index(Box<AccessPath>, usize),
    /// Slice from index `i` to end of an array at the given path.
    /// Used for `& rest` patterns in array destructuring.
    Slice(Box<AccessPath>, usize),
    /// Value at keyword key in a struct at the given path.
    Key(Box<AccessPath>, PatternKey),
}

/// A constructor represents the "shape" that a pattern tests for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Constructor {
    /// Literal value (int, float, string, keyword, bool).
    Literal(PatternLiteral),
    /// Cons cell (pair).
    Cons,
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

impl Constructor {
    /// Number of sub-patterns this constructor expands to.
    pub fn arity(&self) -> usize {
        match self {
            Constructor::Literal(_) | Constructor::Nil | Constructor::EmptyList => 0,
            Constructor::Cons => 2,
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
    /// Guard expression, if any.
    pub guard: Option<Hir>,
    /// Index into the original arms vec (for body lookup).
    pub arm_index: usize,
    /// Bindings accumulated from `Var` patterns in columns that were
    /// removed during specialization or default-matrix construction.
    /// These are carried forward so the Leaf node includes them.
    pub bindings: Vec<(Binding, AccessPath)>,
}

impl PatternRow {
    /// Create a new row with no accumulated bindings.
    pub fn new(patterns: Vec<HirPattern>, guard: Option<Hir>, arm_index: usize) -> Self {
        PatternRow {
            patterns,
            guard,
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
                rows.push(PatternRow::new(vec![expanded], guard.clone(), i));
            }
        }
        PatternMatrix { rows }
    }

    /// Compile the matrix into a decision tree.
    pub fn compile(self, col_access: Vec<AccessPath>) -> DecisionTree {
        compile_matrix(self, col_access)
    }
}

// ── Pattern classification ─────────────────────────────────────────

/// Check if a pattern is a wildcard or variable (matches anything).
fn is_wildcard(pat: &HirPattern) -> bool {
    matches!(pat, HirPattern::Wildcard | HirPattern::Var(_))
}

/// Extract the constructor from a pattern, if it has one.
///
/// List patterns are decomposed into cons chains: a non-empty list
/// `(a b c)` is treated as `Cons` at the top level, with the head
/// being the first element and the tail being the remaining list.
/// An empty list `()` maps to `EmptyList`.
fn pattern_constructor(pat: &HirPattern) -> Option<Constructor> {
    match pat {
        HirPattern::Wildcard | HirPattern::Var(_) => None,
        HirPattern::Nil => Some(Constructor::Nil),
        HirPattern::Literal(lit) => Some(Constructor::Literal(lit.clone())),
        HirPattern::Cons { .. } => Some(Constructor::Cons),
        HirPattern::List { elements, rest } => {
            if elements.is_empty() && rest.is_none() {
                Some(Constructor::EmptyList)
            } else {
                // Non-empty list → cons chain decomposition.
                Some(Constructor::Cons)
            }
        }
        HirPattern::Tuple { elements, rest } => {
            if rest.is_some() {
                Some(Constructor::ArrayRest(elements.len()))
            } else {
                Some(Constructor::Array(elements.len()))
            }
        }
        HirPattern::Array { elements, rest } => {
            if rest.is_some() {
                Some(Constructor::ArrayMutRest(elements.len()))
            } else {
                Some(Constructor::ArrayMut(elements.len()))
            }
        }
        HirPattern::Struct { entries } => Some(Constructor::Struct(
            entries.iter().map(|(k, _)| k.clone()).collect(),
        )),
        HirPattern::Table { entries } => Some(Constructor::Table(
            entries.iter().map(|(k, _)| k.clone()).collect(),
        )),
        HirPattern::Set { .. } => Some(Constructor::Set),
        HirPattern::SetMut { .. } => Some(Constructor::SetMut),
        HirPattern::Or(_) => {
            // Or-patterns should have been expanded before reaching here.
            None
        }
        HirPattern::NamedStruct { .. } => {
            // NamedStruct only appears in &named parameter destructuring, never in match.
            unreachable!("NamedStruct in pattern_constructor")
        }
    }
}

// ── Binding collection ─────────────────────────────────────────────

/// Collect bindings from a pattern with their access paths.
fn collect_pattern_bindings(
    pat: &HirPattern,
    access: &AccessPath,
    out: &mut Vec<(Binding, AccessPath)>,
) {
    match pat {
        HirPattern::Var(binding) => {
            out.push((*binding, access.clone()));
        }
        HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
        HirPattern::Cons { head, tail } => {
            collect_pattern_bindings(head, &AccessPath::Car(Box::new(access.clone())), out);
            collect_pattern_bindings(tail, &AccessPath::Cdr(Box::new(access.clone())), out);
        }
        HirPattern::List { elements, rest } => {
            // Walk the list spine: car/cdr chain.
            let mut current = access.clone();
            for elem in elements {
                collect_pattern_bindings(elem, &AccessPath::Car(Box::new(current.clone())), out);
                current = AccessPath::Cdr(Box::new(current));
            }
            if let Some(rest_pat) = rest {
                collect_pattern_bindings(rest_pat, &current, out);
            }
        }
        HirPattern::Tuple { elements, rest } | HirPattern::Array { elements, rest } => {
            for (i, elem) in elements.iter().enumerate() {
                collect_pattern_bindings(
                    elem,
                    &AccessPath::Index(Box::new(access.clone()), i),
                    out,
                );
            }
            if let Some(rest_pat) = rest {
                // Rest binds to a slice from index elements.len().
                collect_pattern_bindings(
                    rest_pat,
                    &AccessPath::Slice(Box::new(access.clone()), elements.len()),
                    out,
                );
            }
        }
        HirPattern::Struct { entries } | HirPattern::Table { entries } => {
            for (key, sub_pat) in entries {
                collect_pattern_bindings(
                    sub_pat,
                    &AccessPath::Key(Box::new(access.clone()), key.clone()),
                    out,
                );
            }
        }
        HirPattern::NamedStruct { .. } => {
            // NamedStruct only appears in &named parameter destructuring, never in match.
            unreachable!("NamedStruct in collect_pattern_bindings")
        }
        HirPattern::Set { binding } | HirPattern::SetMut { binding } => {
            // Set patterns bind the whole value — the binding sub-pattern
            // receives the same access path as the set itself.
            collect_pattern_bindings(binding, access, out);
        }
        HirPattern::Or(alts) => {
            // Should have been expanded. Collect from first alternative.
            if let Some(first) = alts.first() {
                collect_pattern_bindings(first, access, out);
            }
        }
    }
}

// ── Column selection heuristic ─────────────────────────────────────

/// Select the best column to split on.
///
/// Heuristic: pick the column with the most distinct constructors.
/// This reduces tree depth by maximizing branching factor.
fn select_column(matrix: &PatternMatrix) -> usize {
    let ncols = matrix.rows.first().map_or(0, |r| r.patterns.len());
    let mut best_col = 0;
    let mut best_count = 0;
    for col in 0..ncols {
        let mut constructors = HashSet::new();
        for row in &matrix.rows {
            // TECH DEBT: Using Debug formatting as a hash key because
            // Constructor doesn't impl Hash (it contains PatternLiteral
            // which has f64). Fine for the heuristic — worst case we
            // pick a suboptimal column.
            collect_constructor_strings(&row.patterns[col], &mut constructors);
        }
        if constructors.len() > best_count {
            best_count = constructors.len();
            best_col = col;
        }
    }
    best_col
}

/// Collect constructor debug strings from a pattern, looking inside or-patterns.
fn collect_constructor_strings(pat: &HirPattern, out: &mut HashSet<String>) {
    if let HirPattern::Or(alts) = pat {
        for alt in alts {
            collect_constructor_strings(alt, out);
        }
    } else if let Some(c) = pattern_constructor(pat) {
        out.insert(format!("{:?}", c));
    }
}

// ── Constructor collection ─────────────────────────────────────────

/// Collect distinct constructors in a column.
///
/// Looks inside or-patterns to find their constituent constructors.
/// Struct and @Struct constructors with different key sets are merged
/// into a single constructor with the union of all keys, because
/// struct patterns are "open" (a value can match multiple
/// patterns with different key sets).
fn collect_constructors(matrix: &PatternMatrix, col: usize) -> Vec<Constructor> {
    let mut seen = Vec::new();
    for row in &matrix.rows {
        collect_constructors_from_pattern(&row.patterns[col], &mut seen);
    }
    merge_struct_table_constructors(&mut seen);
    seen
}

fn collect_constructors_from_pattern(pat: &HirPattern, seen: &mut Vec<Constructor>) {
    if let HirPattern::Or(alts) = pat {
        for alt in alts {
            collect_constructors_from_pattern(alt, seen);
        }
    } else if let Some(c) = pattern_constructor(pat) {
        if !seen.iter().any(|s: &Constructor| s == &c) {
            seen.push(c);
        }
    }
}

/// Merge all Struct constructors into one with the union of keys,
/// and all @Struct constructors into one with the union of keys.
///
/// Struct/@struct patterns are "open" — they check for key presence,
/// not exclusivity. Two struct patterns with different key sets can
/// both match the same value, so they must be treated as the same
/// constructor to avoid the decision tree committing to one branch
/// and missing the other.
fn merge_struct_table_constructors(ctors: &mut Vec<Constructor>) {
    // Merge Struct keys
    let mut struct_keys: Vec<PatternKey> = Vec::new();
    let mut has_struct = false;
    for ctor in ctors.iter() {
        if let Constructor::Struct(keys) = ctor {
            has_struct = true;
            for k in keys {
                if !struct_keys.contains(k) {
                    struct_keys.push(k.clone());
                }
            }
        }
    }
    if has_struct {
        ctors.retain(|c| !matches!(c, Constructor::Struct(_)));
        ctors.push(Constructor::Struct(struct_keys));
    }

    // Merge @Struct keys
    let mut table_keys: Vec<PatternKey> = Vec::new();
    let mut has_table = false;
    for ctor in ctors.iter() {
        if let Constructor::Table(keys) = ctor {
            has_table = true;
            for k in keys {
                if !table_keys.contains(k) {
                    table_keys.push(k.clone());
                }
            }
        }
    }
    if has_table {
        ctors.retain(|c| !matches!(c, Constructor::Table(_)));
        ctors.push(Constructor::Table(table_keys));
    }
}

// ── Sub-pattern extraction ─────────────────────────────────────────

/// Extract sub-patterns from a pattern matching a given constructor.
///
/// For wildcards/variables, returns `arity` wildcards.
/// For list patterns, decomposes into head + tail (cons chain).
fn extract_sub_patterns(pat: &HirPattern, ctor: &Constructor) -> Vec<HirPattern> {
    match pat {
        HirPattern::Wildcard | HirPattern::Var(_) => {
            vec![HirPattern::Wildcard; ctor.arity()]
        }
        HirPattern::Cons { head, tail } => {
            vec![*head.clone(), *tail.clone()]
        }
        HirPattern::List { elements, rest } => {
            if elements.is_empty() && rest.is_none() {
                vec![] // EmptyList — arity 0
            } else if !elements.is_empty() {
                // Cons chain decomposition: head is first element,
                // tail is the remaining list pattern.
                let head = elements[0].clone();
                let tail = if elements.len() == 1 {
                    match rest {
                        Some(r) => *r.clone(),
                        None => HirPattern::List {
                            elements: vec![],
                            rest: None,
                        },
                    }
                } else {
                    HirPattern::List {
                        elements: elements[1..].to_vec(),
                        rest: rest.clone(),
                    }
                };
                vec![head, tail]
            } else {
                vec![]
            }
        }
        HirPattern::Tuple { elements, rest } | HirPattern::Array { elements, rest } => {
            let mut sub = elements.clone();
            // For rest constructors, include the rest pattern as an extra sub-pattern.
            if matches!(
                ctor,
                Constructor::ArrayRest(_) | Constructor::ArrayMutRest(_)
            ) {
                sub.push(rest.as_deref().cloned().unwrap_or(HirPattern::Wildcard));
            }
            sub
        }
        HirPattern::Struct { entries } | HirPattern::Table { entries } => {
            // The constructor carries the merged key set (union of all
            // struct patterns in the column). Produce a sub-pattern
            // for each key in the merged set: the pattern's sub-pattern
            // for keys it mentions, Wildcard for keys it doesn't.
            let merged_keys = match ctor {
                Constructor::Struct(keys) | Constructor::Table(keys) => keys,
                _ => return entries.iter().map(|(_, p)| p.clone()).collect(),
            };
            merged_keys
                .iter()
                .map(|key| {
                    entries
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, p)| p.clone())
                        .unwrap_or(HirPattern::Wildcard)
                })
                .collect()
        }
        HirPattern::Set { binding } | HirPattern::SetMut { binding } => {
            vec![*binding.clone()]
        }
        _ => vec![],
    }
}

// ── Constructor compatibility ───────────────────────────────────────

/// Check if a pattern's constructor is compatible with a given constructor.
///
/// For most constructors, this is exact equality. For Struct and @Struct,
/// any struct pattern is compatible with any Struct constructor (and
/// similarly for @Struct), because struct patterns are "open" —
/// they check key presence, not exclusivity. The merged constructor
/// carries the union of all keys.
fn constructor_compatible(pat_ctor: &Constructor, target: &Constructor) -> bool {
    match (pat_ctor, target) {
        (Constructor::Struct(_), Constructor::Struct(_)) => true,
        (Constructor::Table(_), Constructor::Table(_)) => true,
        _ => pat_ctor == target,
    }
}

// ── Matrix specialization ──────────────────────────────────────────

/// Specialize the matrix for a given constructor in a given column.
///
/// - Rows whose pattern in `col` matches `ctor`: kept, column replaced
///   by the constructor's sub-patterns.
/// - Rows whose pattern in `col` is a wildcard/variable: kept, column
///   replaced by `arity` wildcards.
/// - Rows whose pattern in `col` is a different constructor: dropped.
/// - Or-patterns: each matching alternative becomes a separate row.
fn specialize(
    matrix: &PatternMatrix,
    col: usize,
    ctor: &Constructor,
    col_access: &AccessPath,
) -> PatternMatrix {
    let mut rows = Vec::new();
    for row in &matrix.rows {
        let pat = &row.patterns[col];
        if is_wildcard(pat) {
            // Carry forward any binding from a Var pattern in this column.
            let mut new_bindings = row.bindings.clone();
            if let HirPattern::Var(binding) = pat {
                new_bindings.push((*binding, col_access.clone()));
            }
            let arity = ctor.arity();
            let mut new_patterns = row.patterns[..col].to_vec();
            for _ in 0..arity {
                new_patterns.push(HirPattern::Wildcard);
            }
            new_patterns.extend_from_slice(&row.patterns[col + 1..]);
            rows.push(PatternRow {
                patterns: new_patterns,
                guard: row.guard.clone(),
                arm_index: row.arm_index,
                bindings: new_bindings,
            });
        } else if let HirPattern::Or(alts) = pat {
            for alt in alts {
                if is_wildcard(alt)
                    || pattern_constructor(alt)
                        .as_ref()
                        .is_some_and(|c| constructor_compatible(c, ctor))
                {
                    let mut new_bindings = row.bindings.clone();
                    if let HirPattern::Var(binding) = alt {
                        new_bindings.push((*binding, col_access.clone()));
                    }
                    let sub_patterns = extract_sub_patterns(alt, ctor);
                    let mut new_patterns = row.patterns[..col].to_vec();
                    new_patterns.extend(sub_patterns);
                    new_patterns.extend_from_slice(&row.patterns[col + 1..]);
                    rows.push(PatternRow {
                        patterns: new_patterns,
                        guard: row.guard.clone(),
                        arm_index: row.arm_index,
                        bindings: new_bindings,
                    });
                }
            }
        } else if pattern_constructor(pat)
            .as_ref()
            .is_some_and(|c| constructor_compatible(c, ctor))
        {
            let sub_patterns = extract_sub_patterns(pat, ctor);
            let mut new_patterns = row.patterns[..col].to_vec();
            new_patterns.extend(sub_patterns);
            new_patterns.extend_from_slice(&row.patterns[col + 1..]);
            rows.push(PatternRow {
                patterns: new_patterns,
                guard: row.guard.clone(),
                arm_index: row.arm_index,
                bindings: row.bindings.clone(),
            });
        }
        // else: different constructor → row is dropped
    }
    PatternMatrix { rows }
}

/// Default matrix: rows where the column is a wildcard/variable,
/// with that column removed. Variable bindings from the removed
/// column are accumulated in the row's `bindings` field.
fn default_matrix(matrix: &PatternMatrix, col: usize, col_access: &AccessPath) -> PatternMatrix {
    let mut rows = Vec::new();
    for row in &matrix.rows {
        if is_wildcard(&row.patterns[col]) {
            let mut new_bindings = row.bindings.clone();
            if let HirPattern::Var(binding) = &row.patterns[col] {
                new_bindings.push((*binding, col_access.clone()));
            }
            let mut new_patterns = row.patterns[..col].to_vec();
            new_patterns.extend_from_slice(&row.patterns[col + 1..]);
            rows.push(PatternRow {
                patterns: new_patterns,
                guard: row.guard.clone(),
                arm_index: row.arm_index,
                bindings: new_bindings,
            });
        }
    }
    PatternMatrix { rows }
}

// ── Access path expansion ──────────────────────────────────────────

/// Expand access paths when specializing a column.
///
/// The column being split is replaced by sub-paths corresponding to
/// the constructor's sub-components.
fn expand_access(col_access: &[AccessPath], col: usize, ctor: &Constructor) -> Vec<AccessPath> {
    let base = &col_access[col];
    let mut new_access = col_access[..col].to_vec();
    match ctor {
        Constructor::Literal(_) | Constructor::Nil | Constructor::EmptyList => {
            // No sub-patterns, no new access paths.
        }
        Constructor::Cons => {
            new_access.push(AccessPath::Car(Box::new(base.clone())));
            new_access.push(AccessPath::Cdr(Box::new(base.clone())));
        }
        Constructor::Array(n) | Constructor::ArrayMut(n) => {
            for i in 0..*n {
                new_access.push(AccessPath::Index(Box::new(base.clone()), i));
            }
        }
        Constructor::ArrayRest(n) | Constructor::ArrayMutRest(n) => {
            for i in 0..*n {
                new_access.push(AccessPath::Index(Box::new(base.clone()), i));
            }
            // Extra access path for the rest slice.
            new_access.push(AccessPath::Slice(Box::new(base.clone()), *n));
        }
        Constructor::Struct(keys) | Constructor::Table(keys) => {
            for key in keys {
                new_access.push(AccessPath::Key(Box::new(base.clone()), key.clone()));
            }
        }
        Constructor::Set | Constructor::SetMut => {
            // Set patterns have arity 1 — the binding gets the whole value.
            new_access.push(base.clone());
        }
    }
    new_access.extend_from_slice(&col_access[col + 1..]);
    new_access
}

/// Remove a column from the access path list.
fn remove_column(col_access: &[AccessPath], col: usize) -> Vec<AccessPath> {
    let mut result = col_access[..col].to_vec();
    result.extend_from_slice(&col_access[col + 1..]);
    result
}

// ── Core algorithm ─────────────────────────────────────────────────

/// Core Maranget compilation algorithm.
fn compile_matrix(matrix: PatternMatrix, col_access: Vec<AccessPath>) -> DecisionTree {
    // Base case 1: empty matrix — no arms match.
    if matrix.rows.is_empty() {
        return DecisionTree::Fail;
    }

    // Base case 2: first row is all wildcards/variables — it matches.
    let first_row = &matrix.rows[0];
    if first_row.patterns.iter().all(is_wildcard) {
        // Start with bindings accumulated from previously removed columns.
        let mut bindings = first_row.bindings.clone();
        // Add bindings from the remaining patterns.
        for (pat, access) in first_row.patterns.iter().zip(col_access.iter()) {
            collect_pattern_bindings(pat, access, &mut bindings);
        }

        if first_row.guard.is_some() {
            let remaining = PatternMatrix {
                rows: matrix.rows[1..].to_vec(),
            };
            return DecisionTree::Guard {
                arm_index: first_row.arm_index,
                bindings,
                otherwise: Box::new(compile_matrix(remaining, col_access)),
            };
        }

        return DecisionTree::Leaf {
            arm_index: first_row.arm_index,
            bindings,
        };
    }

    // Recursive case: select a column to split on.
    let col = select_column(&matrix);
    let constructors = collect_constructors(&matrix, col);

    let mut cases = Vec::new();
    for ctor in &constructors {
        let specialized = specialize(&matrix, col, ctor, &col_access[col]);
        let new_access = expand_access(&col_access, col, ctor);
        cases.push((ctor.clone(), compile_matrix(specialized, new_access)));
    }

    let def_matrix = default_matrix(&matrix, col, &col_access[col]);
    let def_access = remove_column(&col_access, col);
    let default = if def_matrix.rows.is_empty() {
        None
    } else {
        Some(Box::new(compile_matrix(def_matrix, def_access)))
    };

    DecisionTree::Switch {
        access: col_access[col].clone(),
        cases,
        default,
    }
}

// ── Reachability analysis ──────────────────────────────────────────

/// Find which arm indices are reachable in the decision tree.
#[allow(dead_code)]
pub fn find_reachable_arms(tree: &DecisionTree) -> HashSet<usize> {
    let mut reachable = HashSet::new();
    collect_reachable(tree, &mut reachable);
    reachable
}

#[allow(dead_code)]
fn collect_reachable(tree: &DecisionTree, out: &mut HashSet<usize>) {
    match tree {
        DecisionTree::Leaf { arm_index, .. } => {
            out.insert(*arm_index);
        }
        DecisionTree::Fail => {}
        DecisionTree::Switch { cases, default, .. } => {
            for (_, subtree) in cases {
                collect_reachable(subtree, out);
            }
            if let Some(d) = default {
                collect_reachable(d, out);
            }
        }
        DecisionTree::Guard {
            arm_index,
            otherwise,
            ..
        } => {
            out.insert(*arm_index);
            collect_reachable(otherwise, out);
        }
    }
}

#[cfg(test)]
mod tests;
