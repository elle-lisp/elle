// audited: 2026-11-10
//! The branches of a merge frame, and whether every path through the frame defines a name.
//!
//! docs/bindings.md
//! docs/signals/inference.md
//!
//! Only an `if` has branches, and `cond` is a chain of them. A name every path
//! through a frame defines leaves out the binding before the frame from what a
//! read after it may find, and the frame around it sees a definition on that
//! path.

use super::super::owned::NameKey;
use super::super::*;

/// Where the analysis of a merge frame's form stands.
#[derive(Default, Clone, Copy)]
pub(super) enum Branch {
    /// Before any branch: an `if`'s condition, or a form with no branches.
    #[default]
    Before,
    Then,
    Else,
}

/// The branches of an `if` that define one name on every path through them.
#[derive(Default)]
pub(super) struct Defines {
    then: bool,
    otherwise: bool,
}

impl Defines {
    /// Whether every path through the form defines the name.
    pub(super) fn every_path(&self) -> bool {
        self.then && self.otherwise
    }
}

impl<'a> Analyzer<'a> {
    /// Mark where the innermost frame's branches start: each branch sees the
    /// scope as it stands here.
    pub(in crate::hir::analyze) fn merge_branch_point(&mut self) {
        let lens: Vec<usize> = self
            .innermost_merges()
            .iter()
            .map(|m| self.entry_count(&m.name))
            .collect();
        if let Some(frame) = self.scopes.last_mut().and_then(|s| s.merges.last_mut()) {
            for (merge, len) in frame.merges.iter_mut().zip(lens) {
                merge.branch_len = len;
            }
            frame.branch = Branch::Then;
        }
    }

    /// Start the next branch of the innermost frame: the bindings the last
    /// branch registered leave the scope.
    pub(in crate::hir::analyze) fn merge_branch(&mut self) {
        let cuts: Vec<(String, usize)> = self
            .innermost_merges()
            .iter()
            .map(|m| (m.name.clone(), m.branch_len))
            .collect();
        self.cut_entries(&cuts);
        self.discard_pending(self.scopes.len().saturating_sub(1));
        if let Some(frame) = self.scopes.last_mut().and_then(|s| s.merges.last_mut()) {
            frame.branch = Branch::Else;
        }
    }

    /// Note that the branch the innermost frame is in defines `key` on every
    /// path through it.
    pub(super) fn defines_in_branch(&mut self, key: &NameKey) {
        let Some(frame) = self.scopes.last_mut().and_then(|s| s.merges.last_mut()) else {
            return;
        };
        let branch = frame.branch;
        let Some(merge) = frame.merges.iter_mut().find(|m| m.key == *key) else {
            return;
        };
        match branch {
            Branch::Before => {}
            Branch::Then => merge.defines.then = true,
            Branch::Else => merge.defines.otherwise = true,
        }
    }
}
