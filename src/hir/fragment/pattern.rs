// audited: 2026-10-21
//! Rebuilds a `match` arm's pattern against a fragment's table, and names what a pattern spells.
//!
//! docs/impl/hir.md

use super::{FragmentName, Rebind};
use crate::hir::pattern::{HirPattern, PatternKey, PatternLiteral};

/// Rebuild `p`, introducing every variable it binds. An `or` binds the same
/// variables in each alternative, so the second alternative finds the binding
/// the first introduced.
pub(super) fn rebind_pattern(p: &HirPattern, r: &mut impl Rebind) -> Option<HirPattern> {
    let boxed = |p: &HirPattern, r: &mut _| rebind_pattern(p, r).map(Box::new);
    Some(match p {
        HirPattern::Wildcard => HirPattern::Wildcard,
        HirPattern::Nil => HirPattern::Nil,
        HirPattern::Literal(l) => HirPattern::Literal(l.clone()),
        HirPattern::Var(b) => HirPattern::Var(r.introduce_once(*b)),
        HirPattern::Pair { head, tail } => HirPattern::Pair {
            head: boxed(head, r)?,
            tail: boxed(tail, r)?,
        },
        HirPattern::List { elements, rest } => HirPattern::List {
            elements: rebind_patterns(elements, r)?,
            rest: rebind_rest(rest, r)?,
        },
        HirPattern::Tuple { elements, rest } => HirPattern::Tuple {
            elements: rebind_patterns(elements, r)?,
            rest: rebind_rest(rest, r)?,
        },
        HirPattern::Array { elements, rest } => HirPattern::Array {
            elements: rebind_patterns(elements, r)?,
            rest: rebind_rest(rest, r)?,
        },
        HirPattern::Struct { entries, rest } => HirPattern::Struct {
            entries: rebind_entries(entries, r)?,
            rest: rebind_rest(rest, r)?,
        },
        HirPattern::Table { entries, rest } => HirPattern::Table {
            entries: rebind_entries(entries, r)?,
            rest: rebind_rest(rest, r)?,
        },
        HirPattern::NamedStruct { entries } => HirPattern::NamedStruct {
            entries: rebind_entries(entries, r)?,
        },
        HirPattern::Set { binding } => HirPattern::Set {
            binding: boxed(binding, r)?,
        },
        HirPattern::SetMut { binding } => HirPattern::SetMut {
            binding: boxed(binding, r)?,
        },
        HirPattern::Or(alts) => HirPattern::Or(rebind_patterns(alts, r)?),
    })
}

fn rebind_patterns(ps: &[HirPattern], r: &mut impl Rebind) -> Option<Vec<HirPattern>> {
    ps.iter().map(|p| rebind_pattern(p, r)).collect()
}

/// A rest pattern: `Some(None)` is a pattern without one.
fn rebind_rest(
    rest: &Option<Box<HirPattern>>,
    r: &mut impl Rebind,
) -> Option<Option<Box<HirPattern>>> {
    match rest {
        Some(p) => Some(Some(Box::new(rebind_pattern(p, r)?))),
        None => Some(None),
    }
}

fn rebind_entries<K: Clone>(
    entries: &[(K, HirPattern)],
    r: &mut impl Rebind,
) -> Option<Vec<(K, HirPattern)>> {
    entries
        .iter()
        .map(|(k, p)| Some((k.clone(), rebind_pattern(p, r)?)))
        .collect()
}

/// Call `f` on every name `p` spells: a keyword literal, and each struct
/// pattern's keys. A variable is a binding, which the fragment's table names.
pub(super) fn pattern_names(p: &HirPattern, f: &mut impl FnMut(FragmentName<'_>)) {
    match p {
        HirPattern::Wildcard | HirPattern::Nil | HirPattern::Var(_) => {}
        HirPattern::Literal(PatternLiteral::Keyword(k)) => f(FragmentName::Keyword(k)),
        HirPattern::Literal(_) => {}
        HirPattern::Pair { head, tail } => {
            pattern_names(head, f);
            pattern_names(tail, f);
        }
        HirPattern::List { elements, rest }
        | HirPattern::Tuple { elements, rest }
        | HirPattern::Array { elements, rest } => {
            for e in elements {
                pattern_names(e, f);
            }
            if let Some(r) = rest {
                pattern_names(r, f);
            }
        }
        HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
            entry_names(entries, f);
            if let Some(r) = rest {
                pattern_names(r, f);
            }
        }
        HirPattern::NamedStruct { entries } => entry_names(entries, f),
        HirPattern::Set { binding } | HirPattern::SetMut { binding } => pattern_names(binding, f),
        HirPattern::Or(alts) => {
            for a in alts {
                pattern_names(a, f);
            }
        }
    }
}

fn entry_names(entries: &[(PatternKey, HirPattern)], f: &mut impl FnMut(FragmentName<'_>)) {
    for (key, p) in entries {
        f(match key {
            PatternKey::Keyword(k) => FragmentName::Keyword(k),
            PatternKey::Symbol(id) => FragmentName::Symbol(*id),
        });
        pattern_names(p, f);
    }
}
