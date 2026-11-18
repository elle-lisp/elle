// audited: 2026-11-14
//! The arms in value position of a tail-call operand, and what the operand allocates off the prefix that runs on every path to each.
//!
//! docs/impl/region/operand.md

use super::super::*;
use crate::hir::region::Region;
use rustc_hash::{FxHashMap, FxHashSet};

/// An arm in value position of a tail-call operand, with its prefix: the
/// subtrees that run on each path to the arm.
struct PrefixedArm<'h> {
    /// The arm's body.
    arm: HirId,
    /// Each `let` init, `begin` expression but the last, condition, `cond` test
    /// up to the arm's clause, `match` scrutinee and earlier `and` or `or`
    /// operand between the operand and the arm.
    prefix: Vec<&'h Hir>,
}

impl<'a> Lowerer<'a> {
    /// Each arm of the tail-call operand `h`, with the regions `h` allocates
    /// outside the arm's prefix. One walk of `h` serves every arm: a prefix
    /// entry is never descended for an arm, so the entries of all the prefixes
    /// are disjoint subtrees, and each is read once.
    pub(in crate::lir::lower) fn operand_arm_allocations(
        &self,
        h: &Hir,
    ) -> FxHashMap<HirId, FxHashSet<Region>> {
        let arms = Self::operand_arms(h);
        let mut entries: FxHashMap<HirId, &Hir> = FxHashMap::default();
        for p in arms.iter().flat_map(|a| a.prefix.iter()) {
            entries.insert(p.id, p);
        }
        let skip: FxHashSet<HirId> = entries.keys().copied().collect();
        let mut outside = FxHashSet::default();
        self.region_info
            .allocated_regions_outside(h, &skip, &mut outside);
        let none = FxHashSet::default();
        let own: Vec<(HirId, FxHashSet<Region>)> = entries
            .values()
            .map(|e| {
                let mut regions = FxHashSet::default();
                self.region_info
                    .allocated_regions_outside(e, &none, &mut regions);
                (e.id, regions)
            })
            .collect();
        arms.iter()
            .map(|a| {
                let mut elsewhere = outside.clone();
                for (id, regions) in &own {
                    if !a.prefix.iter().any(|p| p.id == *id) {
                        elsewhere.extend(regions);
                    }
                }
                (a.arm, elsewhere)
            })
            .collect()
    }

    /// The arms in value position of the tail-call operand `h`: each arm of an
    /// `if`, a `cond` or a `match` and each operand of an `and` or `or`, reached
    /// through a `let` or `letrec` body, a `begin`'s tail, an arm's body and an
    /// `and` or `or`'s last operand. An arm inside an earlier `and` or `or` operand is none,
    /// because past its end the test can still run the next operand.
    fn operand_arms(h: &Hir) -> Vec<PrefixedArm<'_>> {
        let mut out = Vec::new();
        Self::collect_operand_arms(h, &mut Vec::new(), &mut out);
        out
    }

    /// [`Self::operand_arms`] below `h`, which `prefix` reaches.
    fn collect_operand_arms<'h>(
        h: &'h Hir,
        prefix: &mut Vec<&'h Hir>,
        out: &mut Vec<PrefixedArm<'h>>,
    ) {
        let depth = prefix.len();
        match &h.kind {
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                prefix.extend(bindings.iter().map(|(_, init)| init));
                Self::collect_operand_arms(body, prefix, out);
            }
            HirKind::Begin(exprs) => {
                if let Some((last, before)) = exprs.split_last() {
                    prefix.extend(before);
                    Self::collect_operand_arms(last, prefix, out);
                }
            }
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                prefix.push(cond);
                Self::collect_operand_arm(then_branch, prefix, out);
                Self::collect_operand_arm(else_branch, prefix, out);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (test, body) in clauses {
                    prefix.push(test);
                    Self::collect_operand_arm(body, prefix, out);
                }
                if let Some(e) = else_branch {
                    Self::collect_operand_arm(e, prefix, out);
                }
            }
            HirKind::Match { value, arms } => {
                prefix.push(value);
                for (_, _, body) in arms {
                    Self::collect_operand_arm(body, prefix, out);
                }
            }
            HirKind::And(exprs) | HirKind::Or(exprs) => match exprs.as_slice() {
                [] => {}
                [only] => Self::collect_operand_arms(only, prefix, out),
                [before @ .., last] => {
                    for e in before {
                        out.push(PrefixedArm {
                            arm: e.id,
                            prefix: prefix.clone(),
                        });
                        prefix.push(e);
                    }
                    Self::collect_operand_arm(last, prefix, out);
                }
            },
            _ => {}
        }
        prefix.truncate(depth);
    }

    /// Record the arm `body` with its prefix, then the arms inside it.
    fn collect_operand_arm<'h>(
        body: &'h Hir,
        prefix: &mut Vec<&'h Hir>,
        out: &mut Vec<PrefixedArm<'h>>,
    ) {
        out.push(PrefixedArm {
            arm: body.id,
            prefix: prefix.clone(),
        });
        Self::collect_operand_arms(body, prefix, out);
    }
}
