//! `CaptureMask` — which of a function's locally-defined slots need a capture
//! cell (an env `CaptureCell`/LBox), indexed by slot with NO upper bound.
//!
//! WHY a type instead of a bare `u64`: a `u64` mask can only name the first 64
//! locally-defined slots, forcing every consumer (the VM env builder, the JIT
//! prologue, the WASM env builders, the bytecode emitter's stack-vs-env
//! decision) into a conservative `index >= 64` fallback that *always* treats a
//! local beyond bit 63 as a captured cell, because the mask cannot say
//! otherwise. That fallback mints a fresh per-execution region holding a dead
//! `CaptureCell` for every uncaptured high local — one leaked region per such
//! local PER CALL (docs/impl/region/rules.md Rule 8). Functions with > 64 locals
//! (stdlib `map`/`merge`/`group-by`, and `zip` transitively) would leak
//! `num_locals - 64` regions on every call. The newtype carries an unbounded
//! bitset, so no such fallback is needed.
//!
//! Naming every slot precisely lets each consumer ask exactly "is THIS slot
//! captured?": an uncaptured high local gets a bare-NIL env placeholder (no
//! cell, no leak), a genuinely captured local at any index is celled correctly.
//!
//! Representation: little-endian `u64` words, bit `i` at `words[i / 64]` bit
//! `i % 64`. The all-clear mask is the empty `words` vec; constructors trim
//! trailing zero words so equal sets compare equal under the derived `PartialEq`
//! (relied on by `Closure`'s equality).

/// A capture bitmask over a function's locally-defined slots, unbounded in
/// width. Bit `i` set means slot `i` needs a capture cell.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
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

    /// Build a mask from a single `u64` of low bits (slots 0..63). Used at the
    /// boundaries that still hand us a `u64` (e.g. the legacy params side).
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

    /// The backing words (little-endian), for serialization.
    pub fn words(&self) -> &[u64] {
        &self.words
    }

    /// Reconstruct a mask from its serialized words, trimming trailing zero
    /// words so the result compares equal to any other encoding of the same set.
    pub fn from_words(mut words: Vec<u64>) -> Self {
        while words.last() == Some(&0) {
            words.pop();
        }
        CaptureMask { words }
    }
}

/// Hex rendering for the `--dump=dfa` / `arena/dump` diagnostics: the words as a
/// big integer, most-significant first (`0` when empty). Keeps the historical
/// `capture_locals_mask=0x..` dump format working without caller changes.
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
