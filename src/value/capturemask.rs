// audited: 2026-11-26
//! `CaptureMask`: which of a function's locally-defined slots need a capture cell, for any number of slots.
//!
//! docs/impl/region/cells.md
//!
//! A slot past bit 63 is named as exactly as one below it. The environment
//! builders of the VM and the WASM backend, the JIT prologue and the emitter
//! each ask of a slot whether it is captured, and an uncaptured slot gets no
//! cell. A mask of one `u64` would make each of them cell every slot past
//! bit 63.
//!
//! Bit `i` is bit `i % 64` of `words[i / 64]`. The empty mask holds no words,
//! and every constructor trims trailing zero words, so two masks of one set
//! compare equal under the derived `PartialEq`, which `Closure`'s equality
//! relies on.

/// A capture bitmask over a function's locally-defined slots, unbounded in
/// width. Bit `i` set means slot `i` needs a capture cell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CaptureMask {
    words: Vec<u64>,
}

impl CaptureMask {
    /// The empty mask — no slot is captured.
    pub const fn empty() -> Self {
        CaptureMask { words: Vec::new() }
    }

    /// Mark slot `i` as captured (needs a cell). Grows the backing store as
    /// needed; the set bit keeps the top word non-zero, so no trailing-zero
    /// word is introduced.
    pub fn set(&mut self, i: usize) {
        let w = i / 64;
        if w >= self.words.len() {
            self.words.resize(w + 1, 0);
        }
        self.words[w] |= 1u64 << (i % 64);
    }

    /// True if slot `i` is captured.
    #[inline]
    pub fn is_set(&self, i: usize) -> bool {
        let w = i / 64;
        w < self.words.len() && (self.words[w] & (1u64 << (i % 64))) != 0
    }

    /// True if no slot is captured.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    /// Build a mask from a single `u64` of low bits (slots 0..63), for a
    /// boundary that holds the mask as one word.
    pub fn from_u64(bits: u64) -> Self {
        if bits == 0 {
            CaptureMask::empty()
        } else {
            CaptureMask { words: vec![bits] }
        }
    }

    /// The low 64 bits (slots 0..63). For places that still consume a `u64`
    /// view; loses information about slots ≥ 64, so only use where that is
    /// provably irrelevant.
    pub fn low_u64(&self) -> u64 {
        self.words.first().copied().unwrap_or(0)
    }

    /// The backing words, lowest slots first, as a payload or a frozen
    /// function stores them.
    pub fn words(&self) -> &[u64] {
        &self.words
    }

    /// Rebuild a mask from its stored words, trimming trailing zero words so
    /// the result compares equal to any other encoding of the same set.
    pub fn from_words(mut words: Vec<u64>) -> Self {
        while words.last() == Some(&0) {
            words.pop();
        }
        CaptureMask { words }
    }
}

/// Hex rendering for the `--dump=dfa` / `arena/dump` diagnostics: the words as a
/// big integer, most-significant first (`0` when empty), as
/// `capture_locals_mask=0x..` prints it.
impl std::fmt::LowerHex for CaptureMask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.words.iter().rposition(|&w| w != 0) {
            None => write!(f, "0"),
            Some(top) => {
                write!(f, "{:x}", self.words[top])?;
                for w in self.words[..top].iter().rev() {
                    write!(f, "{:016x}", w)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests;
