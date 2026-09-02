//! What one activation owes the region system when it ends.
//!
//! Two obligations, one life. The **owner node** is the pages-less forest root
//! `AdoptIntoActivation` adopts members into; the **deferred set** is the
//! releases a frame-replacing tail call stranded, which the new activation took
//! over. Both must reach the activation's normal completion across however many
//! parks it takes on the way, and both must be discharged where the activation
//! is abandoned instead (docs/impl/region/owner.md § "A deferred tail-call
//! release has the node's life").
//!
//! Carrying them as one record is what makes the move discipline enforceable:
//! every site that takes the slot, parks it in a `BytecodeFrame`, restores it,
//! or releases it handles both or neither. Split into two carriers, a park that
//! moved one and dropped the other would be a leak on one side and a strand on
//! the other, and nothing would fail on it.

use crate::hir::region::RuntimeRegion;

/// The region obligations of one activation, held in `Fiber::activation_dues`
/// (one entry per activation frame, parallel to `activation_region_maps`) and
/// MOVED into `BytecodeFrame::activation_dues` for the duration of a park.
///
/// `Clone` exists because `BytecodeFrame` is cloneable, and a clone duplicates
/// obligations: two records naming one region release it twice. The clone sites
/// are the frame-chain hand-offs that drop the original in the same breath
/// (`resume_suspended` re-parking the frames it did not reach), never a way to
/// hold the same dues in two live places.
#[derive(Debug, Default, Clone)]
pub struct ActivationDues {
    /// The activation's owner node — the forest root minted lazily on the first
    /// `AdoptIntoActivation`, whose single decref subtree-drops every member the
    /// activation adopted. `None` for an activation that never adopted.
    pub owner_node: Option<RuntimeRegion>,
    /// The per-call regions this activation took over from frame-replacing tail
    /// calls: the callee closure's own region, and the merged closure-cycle
    /// arena a letrec body tail-called out of. Each is owed exactly ONE decref,
    /// which is why [`Self::defer`] dedupes — a tail-recursive `go` re-enters
    /// with the same closure every iteration and strands one release, not one
    /// per step.
    pub deferred: Vec<RuntimeRegion>,
}

impl ActivationDues {
    /// The dues of an activation that adopted `node` and deferred nothing — the
    /// shape a test builds a park from directly, where the production sites take
    /// the whole record off the live slot.
    pub fn with_owner_node(node: RuntimeRegion) -> Self {
        ActivationDues {
            owner_node: Some(node),
            deferred: Vec::new(),
        }
    }

    /// Whether this activation owes nothing at all.
    pub fn is_empty(&self) -> bool {
        self.owner_node.is_none() && self.deferred.is_empty()
    }

    /// Take over one stranded release, ignoring a region already owed. Called
    /// where the tail call is BUILT (`tail_call_inner`), so the deferral is
    /// recorded on the activation that owes it whether or not the interpreter
    /// trampoline is the loop that consumes the pending call.
    pub fn defer(&mut self, region: RuntimeRegion) {
        if !self.deferred.contains(&region) {
            self.deferred.push(region);
        }
    }

    /// Take the deferred set alone, leaving the node in place. The abandoned
    /// exits run these where the clean break would have; the node's own
    /// disposal there is a separate question (it rides out to the caller that
    /// may still park the frame).
    pub fn take_deferred(&mut self) -> Vec<RuntimeRegion> {
        std::mem::take(&mut self.deferred)
    }
}

#[cfg(test)]
mod tests;
