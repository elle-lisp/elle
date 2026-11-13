// audited: 2026-11-13
//! A compiled call's hand-off word: the offers a call site hands a call helper, and the positions the helper writes back.
//!
//! docs/impl/region/handoff.md

use crate::vm::call::handoff::{Offered, Taken};

/// The offers a compiled call site handed a call helper in its hand-off word,
/// none for a null pointer. The word is zeroed here, so an exit that writes
/// nothing back reports that the callee took nothing.
#[inline]
pub(super) fn read_handoff(handoff: *mut u64) -> Offered {
    if handoff.is_null() {
        return Offered::NONE;
    }
    Offered::from_word(std::mem::take(unsafe { &mut *handoff }))
}

/// Report to the calling frame the positions its callee took, so the frame
/// stamps their slots `nil`.
#[inline]
pub(super) fn write_handoff(handoff: *mut u64, taken: Taken) {
    if !handoff.is_null() {
        unsafe { *handoff = taken.word() };
    }
}

/// A tail call's late mints, and the hand-off word it reports to the calling
/// frame in: the late mints it did not make, whose stashes the frame stamps
/// `nil`.
#[derive(Clone, Copy)]
pub(crate) struct TailHandoff {
    pub(super) late: Offered,
    word: *mut u64,
}

impl TailHandoff {
    /// A spliced tail call, which carries no stash.
    pub(crate) const NONE: TailHandoff = TailHandoff {
        late: Offered::NONE,
        word: std::ptr::null_mut(),
    };

    /// Read the late mints in `word`, and report every one unmade until the
    /// helper makes some.
    pub(super) fn read(word: *mut u64) -> TailHandoff {
        let handoff = TailHandoff {
            late: read_handoff(word),
            word,
        };
        handoff.made(Offered::NONE);
        handoff
    }

    /// Report that the helper made the late mints in `minted`.
    pub(super) fn made(self, minted: Offered) {
        if !self.word.is_null() {
            unsafe { *self.word = self.late.without(minted).word() };
        }
    }
}
