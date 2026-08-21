//! SSA conversion: eliminate Assign, convert While to Loop/Recur,
//! explicit cell ops for CaptureCell bindings.
//!
//! Transforms imperative HIR (with While/Assign) into functional HIR
//! (with Loop/Recur, let-chains, and explicit cell operations). This is
//! the foundation for region inference, type inference, and signal inference.
//!
//! The transform handles three patterns:
//!
//! 1. **While + Assign → Loop/Recur:** mutable bindings assigned in a
//!    while body become loop parameters; assigns become recur arguments.
//!
//! 2. **Sequential Assign in Begin → Define of fresh SSA binding:**
//!    `(assign x val)` in a begin sequence becomes `(define x' val)`,
//!    renaming subsequent uses of x to x'.
//!
//! 3. **CaptureCell bindings → explicit cell ops:** bindings that
//!    `needs_capture()` get explicit DerefCell (for reads) and SetCell
//!    (for writes) in the HIR. The binding itself holds a cell; mutation
//!    goes through set-cell!, reading through deref-cell.
//!
//! Cell insertion is a **structural** decision (`needs_capture` = captured ∧
//! mutable/prebound), NOT an escape question: a captured *mutable* binding needs
//! a shared cell so mutations cross the closure boundary even when the capturing
//! closure is called in place and never escapes. So this consumer reads
//! `needs_capture()` (the structural-only role the capture flag has left), not the
//! authoritative escape analysis (`hir::escape`) — routing it through escape would
//! drop the cell for a captured-mutable non-escaping binding and silently lose the
//! shared mutation.
//!
//! **Branch phi-insertion:** Assigns inside if/cond/match arms in Begin
//! sequences get proper phi-insertion — condition temps bound once,
//! phi-selects via nested Ifs (cond) or duplicated match (match).
//! Non-Begin contexts use assign_preserved to keep assigns as runtime
//! slot mutations.

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{CallArg, Hir, HirKind};
use super::pattern::HirPattern;
use crate::signals::Signal;
use std::collections::{BTreeSet, HashMap};

/// Run the functionalize transform on a HIR tree.
///
/// Eliminates Assign (except in-branch) and converts While to
/// Loop/Recur. CaptureCell bindings get explicit DerefCell/SetCell
/// ops. Modifies the arena to create fresh bindings for SSA versions.
pub fn functionalize(hir: &mut Hir, arena: &mut BindingArena) {
    let mut ctx = FnCtx {
        arena,
        renames: HashMap::new(),
        cell_bindings: BTreeSet::new(),
        assign_preserved: BTreeSet::new(),
    };
    *hir = ctx.transform(hir);
}

struct FnCtx<'a> {
    arena: &'a mut BindingArena,
    renames: HashMap<Binding, Binding>,
    /// Bindings that have been wrapped in cells (needs_capture).
    /// References to these must go through DerefCell, assigns through SetCell.
    cell_bindings: BTreeSet<Binding>,
    /// Bindings whose assigns must NOT be SSA-converted. Includes loop
    /// parameters (threaded via Recur) and outer-scope variables assigned
    /// inside a loop body (maintained via slot mutation by the lowerer).
    /// This is a one-way door: once a binding is slot-mutated, no nested
    /// construct may fork a fresh SSA version of it — in particular a
    /// nested while must not promote it to its own loop parameter, since
    /// the fork's post-loop rename escapes the enclosing branch arm and
    /// paths that skip the nested loop would read an uninitialized slot.
    assign_preserved: BTreeSet<Binding>,
}

mod transform;

mod phi;
mod phimatch;

impl<'a> FnCtx<'a> {
    /// Create a fresh SSA version of a binding, copying its metadata.
    fn fresh_version(&mut self, original: Binding) -> Binding {
        let info = self.arena.get(original);
        let name = info.name;
        let scope = info.scope;
        let new_binding = self.arena.alloc(name, scope);
        self.arena.get_mut(new_binding).is_immutable = true;
        new_binding
    }

    /// Create a fresh synthetic binding with no connection to any real
    /// source binding. Used for phi-insertion condition temporaries.
    fn gensym(&mut self) -> Binding {
        let binding = self.arena.gensym();
        self.arena.get_mut(binding).is_immutable = true;
        binding
    }

    /// Look up the current SSA version of a binding, following chains.
    fn resolve(&self, b: Binding) -> Binding {
        let mut current = b;
        while let Some(&next) = self.renames.get(&current) {
            current = next;
        }
        current
    }

    /// Collect bindings that are Assign'd within a HIR subtree,
    /// excluding CaptureCell and cell_bindings. Uses BTreeSet for
    /// deterministic ordering (reproducible Loop binding order across runs).
    fn collect_assigned_bindings(&self, hir: &Hir, out: &mut BTreeSet<Binding>) {
        match &hir.kind {
            HirKind::Assign { target, value } => {
                if !self.arena.get(*target).needs_capture() && !self.cell_bindings.contains(target)
                {
                    out.insert(*target);
                }
                self.collect_assigned_bindings(value, out);
            }
            // Don't look inside lambdas — they have their own scope
            HirKind::Lambda { .. } => {}
            _ => {
                hir.for_each_child(|child| {
                    self.collect_assigned_bindings(child, out);
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;
