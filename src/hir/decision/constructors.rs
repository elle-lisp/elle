//! Constructor extraction, collection, and compatibility for pattern columns.

use super::*;

/// Collect constructors from a pattern into a set, looking inside or-patterns.
pub(super) fn collect_constructors_for_column(pat: &HirPattern, out: &mut HashSet<Constructor>) {
    if let HirPattern::Or(alts) = pat {
        for alt in alts {
            collect_constructors_for_column(alt, out);
        }
    } else if let Some(c) = pattern_constructor(pat) {
        out.insert(c);
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
pub(super) fn collect_constructors(matrix: &PatternMatrix, col: usize) -> Vec<Constructor> {
    let mut seen = Vec::new();
    for row in &matrix.rows {
        collect_constructors_from_pattern(&row.patterns[col], &mut seen);
    }
    merge_struct_table_constructors(&mut seen);
    seen
}

pub(super) fn collect_constructors_from_pattern(pat: &HirPattern, seen: &mut Vec<Constructor>) {
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
pub(super) fn merge_struct_table_constructors(ctors: &mut Vec<Constructor>) {
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
/// For list patterns, decomposes into head + tail (pair chain).
pub(super) fn extract_sub_patterns(pat: &HirPattern, ctor: &Constructor) -> Vec<HirPattern> {
    match pat {
        HirPattern::Wildcard | HirPattern::Var(_) => {
            vec![HirPattern::Wildcard; ctor.arity()]
        }
        HirPattern::Pair { head, tail } => {
            vec![*head.clone(), *tail.clone()]
        }
        HirPattern::List { elements, rest } => {
            if elements.is_empty() && rest.is_none() {
                vec![] // EmptyList — arity 0
            } else if !elements.is_empty() {
                // Pair chain decomposition: head is first element,
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
        HirPattern::Struct { entries, rest: _ } | HirPattern::Table { entries, rest: _ } => {
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
pub(super) fn constructor_compatible(pat_ctor: &Constructor, target: &Constructor) -> bool {
    match (pat_ctor, target) {
        (Constructor::Struct(_), Constructor::Struct(_)) => true,
        (Constructor::Table(_), Constructor::Table(_)) => true,
        _ => pat_ctor == target,
    }
}
