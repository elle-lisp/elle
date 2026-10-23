// audited: 2026-10-21
//! What each graft bound to a parameter of a function this unit defines, which inference joins as a call site.
//!
//! docs/impl/graft.md
//! docs/impl/typeinfer.md

use crate::hir::binding::Binding;
use crate::hir::expr::HirId;
use crate::hir::types::TyId;
use rustc_hash::FxHashMap;

/// The operand one graft bound to one parameter of a function this unit
/// defines. The definition stays, and its parameter takes its type from what
/// its calls pass; a grafted call is no longer in the tree to say so.
#[derive(Debug, Clone)]
pub(crate) struct Contribution {
    /// The function binding whose call was grafted.
    pub(crate) function: Binding,
    /// That function's parameter.
    pub(crate) param: Binding,
    pub(crate) operand: Operand,
}

/// Where inference reads the type a grafted operand had.
#[derive(Debug, Clone)]
pub(crate) enum Operand {
    /// An immediate literal, or the list shape a rest parameter received.
    Type(TyId),
    /// A local the graft bound with a `let`: its type is its initializer's,
    /// read where the `let` sits.
    Local(Binding),
    /// The caller's own binding, written in the body in place of the
    /// parameter. Each occurrence's type is read where it stands, so a guard
    /// around the call site narrows what it contributes. With no occurrence
    /// left, the binding's own type answers.
    Occurrences(Vec<HirId>, Binding),
}

/// Every contribution the graft made, for inference to join.
#[derive(Debug, Default)]
pub(crate) struct GraftRecord {
    contributions: Vec<Contribution>,
}

impl GraftRecord {
    pub(crate) fn push(&mut self, c: Contribution) {
        self.contributions.push(c);
    }

    pub(crate) fn contributions(&self) -> &[Contribution] {
        &self.contributions
    }

    /// Every node id an `Occurrences` operand names, for a pass that rebuilds
    /// those nodes to report where they went.
    pub(crate) fn tracked_nodes(&self) -> impl Iterator<Item = HirId> + '_ {
        self.contributions
            .iter()
            .filter_map(|c| match &c.operand {
                Operand::Occurrences(ids, _) => Some(ids.iter().copied()),
                _ => None,
            })
            .flatten()
    }

    /// Follow a rebuild: each tracked id becomes the ids of the nodes rebuilt
    /// from it, several when the rebuild duplicated its subtree.
    pub(crate) fn remap(&mut self, moved: &FxHashMap<HirId, Vec<HirId>>) {
        for c in &mut self.contributions {
            if let Operand::Occurrences(ids, _) = &mut c.operand {
                *ids = ids
                    .iter()
                    .flat_map(|id| moved.get(id).cloned().unwrap_or_else(|| vec![*id]))
                    .collect();
            }
        }
    }
}
