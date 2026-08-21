use super::*;

/// Check if a pattern is a wildcard or variable (matches anything).
pub(super) fn is_wildcard(pat: &HirPattern) -> bool {
    matches!(pat, HirPattern::Wildcard | HirPattern::Var(_))
}

/// Extract the constructor from a pattern, if it has one.
///
/// List patterns are decomposed into cons chains: a non-empty list
/// `(a b c)` is treated as `Pair` at the top level, with the head
/// being the first element and the tail being the remaining list.
/// An empty list `()` maps to `EmptyList`.
pub(super) fn pattern_constructor(pat: &HirPattern) -> Option<Constructor> {
    match pat {
        HirPattern::Wildcard | HirPattern::Var(_) => None,
        HirPattern::Nil => Some(Constructor::Nil),
        HirPattern::Literal(lit) => Some(Constructor::Literal(lit.clone())),
        HirPattern::Pair { .. } => Some(Constructor::Pair),
        HirPattern::List { elements, rest } => {
            if elements.is_empty() && rest.is_none() {
                Some(Constructor::EmptyList)
            } else {
                // Non-empty list → cons chain decomposition.
                Some(Constructor::Pair)
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
        HirPattern::Struct { entries, rest: _ } => Some(Constructor::Struct(
            entries.iter().map(|(k, _)| k.clone()).collect(),
        )),
        HirPattern::Table { entries, rest: _ } => Some(Constructor::Table(
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
pub(super) fn collect_pattern_bindings(
    pat: &HirPattern,
    access: &AccessPath,
    out: &mut Vec<(Binding, AccessPath)>,
) {
    match pat {
        HirPattern::Var(binding) => {
            out.push((*binding, access.clone()));
        }
        HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
        HirPattern::Pair { head, tail } => {
            collect_pattern_bindings(head, &AccessPath::First(Box::new(access.clone())), out);
            collect_pattern_bindings(tail, &AccessPath::Rest(Box::new(access.clone())), out);
        }
        HirPattern::List { elements, rest } => {
            // Walk the list spine: car/cdr chain.
            let mut current = access.clone();
            for elem in elements {
                collect_pattern_bindings(elem, &AccessPath::First(Box::new(current.clone())), out);
                current = AccessPath::Rest(Box::new(current));
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
        HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
            for (key, sub_pat) in entries {
                collect_pattern_bindings(
                    sub_pat,
                    &AccessPath::Key(Box::new(access.clone()), key.clone()),
                    out,
                );
            }
            if let Some(rest_pat) = rest {
                let exclude: Vec<PatternKey> = entries.iter().map(|(k, _)| k.clone()).collect();
                collect_pattern_bindings(
                    rest_pat,
                    &AccessPath::StructRest(Box::new(access.clone()), exclude),
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
pub(super) fn select_column(matrix: &PatternMatrix) -> usize {
    let ncols = matrix.rows.first().map_or(0, |r| r.patterns.len());
    let mut best_col = 0;
    let mut best_count = 0;
    for col in 0..ncols {
        let mut constructors: HashSet<Constructor> = HashSet::new();
        for row in &matrix.rows {
            collect_constructors_for_column(&row.patterns[col], &mut constructors);
        }
        if constructors.len() > best_count {
            best_count = constructors.len();
            best_col = col;
        }
    }
    best_col
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
pub(super) fn specialize(
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
                has_guard: row.has_guard,
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
                        has_guard: row.has_guard,
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
            // Accumulate the rest binding (if any) for Struct/Table patterns.
            // The rest sub-pattern is NOT added to sub_patterns (no decision tree
            // constructor for it), so we accumulate it directly into bindings now.
            let mut new_bindings = row.bindings.clone();
            match pat {
                HirPattern::Struct {
                    entries,
                    rest: Some(rest_pat),
                }
                | HirPattern::Table {
                    entries,
                    rest: Some(rest_pat),
                } => {
                    let exclude: Vec<PatternKey> = entries.iter().map(|(k, _)| k.clone()).collect();
                    collect_pattern_bindings(
                        rest_pat,
                        &AccessPath::StructRest(Box::new(col_access.clone()), exclude),
                        &mut new_bindings,
                    );
                }
                _ => {}
            }
            rows.push(PatternRow {
                patterns: new_patterns,
                has_guard: row.has_guard,
                arm_index: row.arm_index,
                bindings: new_bindings,
            });
        }
        // else: different constructor → row is dropped
    }
    PatternMatrix { rows }
}

/// Default matrix: rows where the column is a wildcard/variable,
/// with that column removed. Variable bindings from the removed
/// column are accumulated in the row's `bindings` field.
pub(super) fn default_matrix(
    matrix: &PatternMatrix,
    col: usize,
    col_access: &AccessPath,
) -> PatternMatrix {
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
                has_guard: row.has_guard,
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
pub(super) fn expand_access(
    col_access: &[AccessPath],
    col: usize,
    ctor: &Constructor,
) -> Vec<AccessPath> {
    let base = &col_access[col];
    let mut new_access = col_access[..col].to_vec();
    match ctor {
        Constructor::Literal(_) | Constructor::Nil | Constructor::EmptyList => {
            // No sub-patterns, no new access paths.
        }
        Constructor::Pair => {
            new_access.push(AccessPath::First(Box::new(base.clone())));
            new_access.push(AccessPath::Rest(Box::new(base.clone())));
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
pub(super) fn remove_column(col_access: &[AccessPath], col: usize) -> Vec<AccessPath> {
    let mut result = col_access[..col].to_vec();
    result.extend_from_slice(&col_access[col + 1..]);
    result
}

// ── Core algorithm ─────────────────────────────────────────────────

/// Core Maranget compilation algorithm.
pub(super) fn compile_matrix(matrix: PatternMatrix, col_access: Vec<AccessPath>) -> DecisionTree {
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

        if first_row.has_guard {
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

// ── Reachability ───────────────────────────────────────────────────

/// Arm indices that appear as a Leaf or Guard node anywhere in the tree.
pub(super) fn find_reachable_arms(tree: &DecisionTree) -> HashSet<usize> {
    let mut reachable = HashSet::new();
    collect_reachable(tree, &mut reachable);
    reachable
}

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
