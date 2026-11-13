// audited: 2026-11-13
//! The arms in value position of a tail-call operand, each with the prefix that runs on every path to it.
//!
//! docs/impl/region/operand.md

use super::super::*;

/// An arm in value position of a tail-call operand, with its prefix: the
/// subtrees that run on each path to the arm.
pub(in crate::lir::lower) struct OperandArm<'h> {
    /// The arm's body.
    pub(in crate::lir::lower) arm: HirId,
    /// Each `let` init, `begin` expression but the last, condition, `cond` test
    /// up to the arm's clause, `match` scrutinee and earlier `and` or `or`
    /// operand between the operand and the arm.
    pub(in crate::lir::lower) prefix: Vec<&'h Hir>,
}

impl<'a> Lowerer<'a> {
    /// The arms in value position of the tail-call operand `h`: each arm of an
    /// `if`, a `cond` or a `match` and each operand of an `and` or `or`, reached
    /// through a `let` or `letrec` body, a `begin`'s tail and an `and` or `or`'s
    /// last operand. An arm inside an earlier `and` or `or` operand is none,
    /// because past its end the test can still run the next operand.
    pub(in crate::lir::lower) fn operand_arms(h: &Hir) -> Vec<OperandArm<'_>> {
        let mut out = Vec::new();
        Self::collect_operand_arms(h, &mut Vec::new(), &mut out);
        out
    }

    /// [`Self::operand_arms`] below `h`, which `prefix` reaches.
    fn collect_operand_arms<'h>(
        h: &'h Hir,
        prefix: &mut Vec<&'h Hir>,
        out: &mut Vec<OperandArm<'h>>,
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
                        out.push(OperandArm {
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
        out: &mut Vec<OperandArm<'h>>,
    ) {
        out.push(OperandArm {
            arm: body.id,
            prefix: prefix.clone(),
        });
        Self::collect_operand_arms(body, prefix, out);
    }
}
