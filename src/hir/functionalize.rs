// audited: 2026-10-06
//! SSA conversion: `assign` becomes a fresh binding where it can, `while` a `Loop`, and cell reads and writes explicit.
//!
//! docs/impl/hir.md
//! docs/impl/escape.md
//!
//! Region inference and type inference run on the functional HIR this pass
//! leaves. The transform handles three patterns:
//!
//! 1. **While + Assign → Loop/Recur:** mutable bindings assigned in a
//!    while body become loop parameters; assigns become recur arguments.
//!
//! 2. **Sequential Assign in Begin → Let of a fresh SSA binding:**
//!    `(assign x val)` in a begin sequence becomes a `let` of a fresh `x'`
//!    around the rest of the sequence, which reads `x'` in place of `x`.
//!
//! 3. **Cell bindings → explicit cell ops:** a binding that
//!    `needs_capture()` holds a cell, so its reads become `DerefCell` and its
//!    writes become `SetCell`.
//!
//! Cell insertion reads `needs_capture()`, a structural fact, and never the
//! escape analysis: a captured mutable binding needs a shared cell even when
//! the capturing closure never escapes.
//!
//! **Branch phi-insertion:** Assigns inside if/cond/match arms in Begin
//! sequences get phi-insertion: condition temps bound once, and phi-selects
//! via nested Ifs (cond) or a duplicated match (match). Outside a Begin,
//! `assign_preserved` keeps assigns as runtime slot mutations.

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{CallArg, Hir, HirId, HirKind};
use super::pattern::HirPattern;
use crate::signals::Signal;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::{BTreeSet, HashMap};

/// Run the functionalize transform on a HIR tree.
///
/// Eliminates Assign (except in-branch) and converts While to
/// Loop/Recur. CaptureCell bindings get explicit DerefCell/SetCell
/// ops. Modifies the arena to create fresh bindings for SSA versions.
pub fn functionalize(hir: &mut Hir, arena: &mut BindingArena) {
    functionalize_tracking(hir, arena, &FxHashSet::default());
}

/// [`functionalize`], answering for each `Var` node id in `tracked` the ids
/// of the nodes rebuilt from it — several when a branch's subtree was
/// duplicated. The graft names the occurrences whose types a later pass reads
/// by id, and the rebuild gives every node a fresh one.
pub fn functionalize_tracking(
    hir: &mut Hir,
    arena: &mut BindingArena,
    tracked: &FxHashSet<HirId>,
) -> FxHashMap<HirId, Vec<HirId>> {
    let mut ctx = FnCtx {
        arena,
        renames: HashMap::new(),
        cell_bindings: BTreeSet::new(),
        assign_preserved: BTreeSet::new(),
        tracked,
        moved: FxHashMap::default(),
    };
    *hir = ctx.transform(hir);
    ctx.moved
}

struct FnCtx<'a> {
    arena: &'a mut BindingArena,
    /// The `Var` node ids whose rebuilt ids `moved` reports.
    tracked: &'a FxHashSet<HirId>,
    moved: FxHashMap<HirId, Vec<HirId>>,
    renames: HashMap<Binding, Binding>,
    /// Bindings that have been wrapped in cells (needs_capture).
    /// References to these must go through DerefCell, assigns through SetCell.
    cell_bindings: BTreeSet<Binding>,
    /// Bindings whose assigns must NOT be SSA-converted. Includes loop
    /// parameters (threaded via Recur) and outer-scope variables assigned
    /// inside a loop body (maintained via slot mutation by the lowerer).
    /// This is a one-way door: once a binding is slot-mutated, no nested
    /// construct may fork a fresh SSA version of it. In particular, a nested
    /// while must not promote it to its own loop parameter. The fork's
    /// post-loop rename would escape the enclosing branch arm, and a path that
    /// skips the nested loop would read an uninitialized slot.
    assign_preserved: BTreeSet<Binding>,
}

mod branches;
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
