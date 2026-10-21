// audited: 2026-10-20
// docs/impl/jit.md
//! The jumps still to come into each LIR block, which say when the translator
//! may seal it.

use std::collections::HashMap;

use crate::lir::{Label, LirView, Terminator};

/// The blocks a terminator's translation jumps to. An `Emit` resumes in the
/// interpreter, so compiled code never jumps to its resume block.
fn jump_targets(term: &Terminator) -> impl Iterator<Item = Label> {
    let (first, second) = match *term {
        Terminator::Jump(label) => (Some(label), None),
        Terminator::Branch {
            then_label,
            else_label,
            ..
        } => (Some(then_label), Some(else_label)),
        Terminator::Return(_) | Terminator::Emit { .. } | Terminator::Unreachable => (None, None),
    };
    first.into_iter().chain(second)
}

/// How many jumps into each LIR block the translator has yet to emit.
///
/// Only a terminator jumps to an LIR block, and every jump a block makes is
/// emitted while that block is translated. So a block's last predecessor is
/// known once the last block that names it as a target has been translated.
pub(super) struct Unsealed {
    /// Each block's label, in the order the view lists the blocks.
    labels: Vec<Label>,
    /// Each block's position in `labels`.
    position: HashMap<Label, usize>,
    /// The jumps still to come into the block at each position.
    pending: Vec<u32>,
}

impl Unsealed {
    /// Count every jump `lir`'s terminators make. A target that names no
    /// block counts nowhere: translating that terminator refuses the compile.
    pub(super) fn new(lir: &LirView<'_>) -> Self {
        let labels: Vec<Label> = lir.blocks().map(|block| block.label()).collect();
        let position: HashMap<Label, usize> = labels
            .iter()
            .enumerate()
            .map(|(at, label)| (*label, at))
            .collect();
        let mut pending = vec![0; labels.len()];
        for block in lir.blocks() {
            for target in jump_targets(&block.terminator()) {
                if let Some(&at) = position.get(&target) {
                    pending[at] += 1;
                }
            }
        }
        Unsealed {
            labels,
            position,
            pending,
        }
    }

    /// The blocks with no jump to come before translation starts, in view
    /// order: the ones no terminator names. The entry is one unless a loop
    /// returns to it, because the loop header's jump into it is emitted first.
    pub(super) fn ready(&self) -> impl Iterator<Item = Label> + '_ {
        self.labels
            .iter()
            .zip(&self.pending)
            .filter(|(_, pending)| **pending == 0)
            .map(|(label, _)| *label)
    }

    /// Note that the block `term` ends has been translated, whether or not
    /// its terminator was. Answers each target whose last jump that was.
    pub(super) fn translated(&mut self, term: &Terminator) -> impl Iterator<Item = Label> {
        let mut complete = [None, None];
        for (slot, target) in complete.iter_mut().zip(jump_targets(term)) {
            if let Some(&at) = self.position.get(&target) {
                self.pending[at] -= 1;
                if self.pending[at] == 0 {
                    *slot = Some(target);
                }
            }
        }
        complete.into_iter().flatten()
    }
}
