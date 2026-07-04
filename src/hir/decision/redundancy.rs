//! Or-pattern alternative redundancy.
//!
//! Left-to-right usefulness, at any nesting depth: an alternative is
//! dead when every value it matches is already matched by an earlier
//! arm, by an earlier alternative of the same or-pattern, or by an
//! earlier alternative of an enclosing or-pattern.
//!
//! Each query reuses the decision-tree compiler: build a matrix of the
//! earlier arms' rows, plus this arm pinned to each shadowing
//! alternative, plus this arm pinned to the tested alternative. The
//! alternative is dead iff its rows are all unreachable in the
//! compiled tree.
//!
//! Guards make coverage conservative in both directions. An earlier
//! arm's guard may fail, so its rows never fully cover (the matrix
//! machinery already models this). And on the arm under test, a failed
//! guard retries the remaining alternatives with fresh bindings — and
//! guards may be impure — so same-arm alternatives keep the guard flag
//! too: a guarded arm's alternatives are only ever killed by earlier
//! arms, never by each other.
//!
//! Pinning collapses every or-node on the path to the traversed
//! alternative, so nested alternatives are tested within their exact
//! enclosing choice; or-nodes elsewhere in the pattern stay intact and
//! expand normally (an alternative is dead only if no combination of
//! sibling choices can reach it).

use super::algo::find_reachable_arms;
use super::{expand_or_pattern, AccessPath, PatternMatrix, PatternRow};
use crate::hir::{Hir, HirPattern};

/// A dead or-pattern alternative: `alternative` (0-based) of an
/// or-node in arm `arm` (0-based).
pub(crate) struct DeadAlternative {
    pub arm: usize,
    pub alternative: usize,
}

/// Find the first dead or-pattern alternative, scanning arms in order
/// and or-nodes in pre-order within each arm.
pub(crate) fn first_dead_alternative(
    arms: &[(HirPattern, Option<Hir>, Hir)],
) -> Option<DeadAlternative> {
    for (arm_idx, (pattern, guard, _body)) in arms.iter().enumerate() {
        let or_nodes = enumerate_or_nodes(pattern);
        if or_nodes.is_empty() {
            continue;
        }
        let has_guard = guard.is_some();
        // Earlier arms' rows, expanded once per arm under review. Bodies
        // and guard expressions are never needed — only guard presence.
        let base: Vec<PatternRow> = arms[..arm_idx]
            .iter()
            .enumerate()
            .flat_map(|(i, (p, g, _))| {
                expand_or_pattern(p)
                    .into_iter()
                    .map(move |q| PatternRow::new(vec![q], g.is_some(), i))
            })
            .collect();

        for (path, n_alts) in &or_nodes {
            let mut rows = base.clone();
            push_ancestor_rows(pattern, path, has_guard, arm_idx, &mut rows);
            for alt in 0..*n_alts {
                // Unique id per probe; candidate rows share it.
                let candidate_id = arms.len() + alt;
                for q in expand_or_pattern(&pin(pattern, path, alt)) {
                    rows.push(PatternRow::new(vec![q], has_guard, candidate_id));
                }
                let tree = PatternMatrix { rows: rows.clone() }.compile(vec![AccessPath::Root]);
                if !find_reachable_arms(&tree).contains(&candidate_id) {
                    return Some(DeadAlternative {
                        arm: arm_idx,
                        alternative: alt,
                    });
                }
                // The candidate rows stay: they are the earlier-alternative
                // context for the next probe.
            }
        }
    }
    None
}

/// Push rows for the earlier alternatives of every or-node the path
/// steps through: within the pinned enclosing choice, those shadow the
/// probe exactly like earlier arms do.
fn push_ancestor_rows(
    pattern: &HirPattern,
    path: &[usize],
    has_guard: bool,
    arm_idx: usize,
    rows: &mut Vec<PatternRow>,
) {
    let mut node = pattern;
    for (depth, &step) in path.iter().enumerate() {
        if let HirPattern::Or(alts) = node {
            for j in 0..step {
                for q in expand_or_pattern(&pin(pattern, &path[..depth], j)) {
                    rows.push(PatternRow::new(vec![q], has_guard, arm_idx));
                }
            }
            node = &alts[step];
        } else {
            node = children(node)[step];
        }
    }
}

/// Path from a pattern root to an or-node: child indices per
/// `children()`. Steps through an or-node select an alternative.
type OrPath = Vec<usize>;

/// All or-nodes in the pattern, pre-order, with their alternative counts.
fn enumerate_or_nodes(pattern: &HirPattern) -> Vec<(OrPath, usize)> {
    let mut out = Vec::new();
    let mut path = Vec::new();
    walk(pattern, &mut path, &mut out);
    out
}

fn walk(pattern: &HirPattern, path: &mut OrPath, out: &mut Vec<(OrPath, usize)>) {
    if let HirPattern::Or(alts) = pattern {
        out.push((path.clone(), alts.len()));
    }
    for (i, child) in children(pattern).into_iter().enumerate() {
        path.push(i);
        walk(child, path, out);
        path.pop();
    }
}

/// Rebuild `pattern` with the or-node at `path` collapsed to its
/// `alternative`-th alternative. Every or-node the path steps through
/// is likewise collapsed to the traversed alternative, pinning the
/// enclosing choice chain; or-nodes off the path are left intact.
fn pin(pattern: &HirPattern, path: &[usize], alternative: usize) -> HirPattern {
    match (pattern, path.split_first()) {
        (HirPattern::Or(alts), None) => alts[alternative].clone(),
        (HirPattern::Or(alts), Some((&step, rest))) => pin(&alts[step], rest, alternative),
        (_, None) => unreachable!("or-path must end at an or-node"),
        (_, Some((&step, rest))) => with_child(
            pattern,
            step,
            pin(children(pattern)[step], rest, alternative),
        ),
    }
}

/// Immediate sub-patterns, in a fixed order `with_child` agrees with:
/// elements/entries first, then the rest pattern if present.
/// `child_order_agreement` in tests.rs pins that agreement.
fn children(pattern: &HirPattern) -> Vec<&HirPattern> {
    match pattern {
        HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) | HirPattern::Var(_) => {
            Vec::new()
        }
        HirPattern::Pair { head, tail } => vec![head, tail],
        HirPattern::List { elements, rest }
        | HirPattern::Tuple { elements, rest }
        | HirPattern::Array { elements, rest } => {
            elements.iter().chain(rest.iter().map(|r| &**r)).collect()
        }
        HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => entries
            .iter()
            .map(|(_, p)| p)
            .chain(rest.iter().map(|r| &**r))
            .collect(),
        HirPattern::NamedStruct { entries } => entries.iter().map(|(_, p)| p).collect(),
        HirPattern::Set { binding } | HirPattern::SetMut { binding } => vec![binding],
        HirPattern::Or(alts) => alts.iter().collect(),
    }
}

/// Clone `pattern` with child `index` (per `children()` order) replaced.
fn with_child(pattern: &HirPattern, index: usize, new: HirPattern) -> HirPattern {
    // Shared splice rule: positional children first, then `rest`.
    fn splice(
        elements: &[HirPattern],
        rest: &Option<Box<HirPattern>>,
        index: usize,
        new: HirPattern,
    ) -> (Vec<HirPattern>, Option<Box<HirPattern>>) {
        let mut elements = elements.to_vec();
        let mut rest = rest.clone();
        if index < elements.len() {
            elements[index] = new;
        } else {
            rest = Some(Box::new(new));
        }
        (elements, rest)
    }

    match pattern {
        HirPattern::Pair { head, tail } => {
            let (mut head, mut tail) = (head.clone(), tail.clone());
            if index == 0 {
                head = Box::new(new);
            } else {
                tail = Box::new(new);
            }
            HirPattern::Pair { head, tail }
        }
        HirPattern::List { elements, rest }
        | HirPattern::Tuple { elements, rest }
        | HirPattern::Array { elements, rest } => {
            let (elements, rest) = splice(elements, rest, index, new);
            match pattern {
                HirPattern::List { .. } => HirPattern::List { elements, rest },
                HirPattern::Tuple { .. } => HirPattern::Tuple { elements, rest },
                _ => HirPattern::Array { elements, rest },
            }
        }
        HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
            // Entries follow the same splice rule, keyed values in place.
            let mut entries = entries.clone();
            let mut rest = rest.clone();
            if index < entries.len() {
                entries[index].1 = new;
            } else {
                rest = Some(Box::new(new));
            }
            if matches!(pattern, HirPattern::Struct { .. }) {
                HirPattern::Struct { entries, rest }
            } else {
                HirPattern::Table { entries, rest }
            }
        }
        HirPattern::NamedStruct { entries } => {
            let mut entries = entries.clone();
            entries[index].1 = new;
            HirPattern::NamedStruct { entries }
        }
        HirPattern::Set { .. } => HirPattern::Set {
            binding: Box::new(new),
        },
        HirPattern::SetMut { .. } => HirPattern::SetMut {
            binding: Box::new(new),
        },
        HirPattern::Or(_) => unreachable!("pin() collapses or-nodes instead of rebuilding them"),
        HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) | HirPattern::Var(_) => {
            unreachable!("leaf patterns have no children")
        }
    }
}

#[cfg(test)]
pub(super) fn children_for_test(pattern: &HirPattern) -> Vec<&HirPattern> {
    children(pattern)
}

#[cfg(test)]
pub(super) fn with_child_for_test(
    pattern: &HirPattern,
    index: usize,
    new: HirPattern,
) -> HirPattern {
    with_child(pattern, index, new)
}
