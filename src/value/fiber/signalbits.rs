//! The SignalBits newtype: signal-type bitset with named methods and operators.

/// Signal type bits. The first 16 are compiler-reserved.
///
/// Newtype over `u64` providing named methods and bitwise operator impls.
///
/// The inner representation is an implementation detail. All code outside
/// this impl block should use the provided methods instead of accessing
/// the raw field.
#[derive(Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SignalBits(u64);

impl SignalBits {
    // -- Constructors --------------------------------------------------------

    /// Wrap a raw bitmask.
    pub const fn new(bits: u64) -> Self {
        SignalBits(bits)
    }

    /// The empty set (no signals).
    pub const EMPTY: SignalBits = SignalBits(0);

    /// The full set (all bits set).
    pub const ALL: SignalBits = SignalBits(!0);

    /// A single-bit mask for bit position `pos`.
    pub const fn from_bit(pos: u32) -> Self {
        SignalBits(1u64 << pos)
    }

    /// Construct from an i64 (e.g. from an Elle integer value).
    pub const fn from_i64(v: i64) -> Self {
        SignalBits(v as u64)
    }

    // -- Predicates ----------------------------------------------------------

    /// True when no bits are set — a normal return, `SIG_OK`.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True when `self` and `other` share at least one bit.
    ///
    /// This is the routing question a fiber mask asks, so it is an overlap
    /// test and not a subset test: a mask of `|:log|` catches the compound
    /// signal `|:log :audit|`. It is symmetric, and the empty set intersects
    /// nothing — see `signalbits/tests.rs`.
    pub const fn intersects(self, other: SignalBits) -> bool {
        self.0 & other.0 != 0
    }

    /// True when `self` has bit at position `pos` set.
    pub const fn has_bit(self, pos: u32) -> bool {
        self.0 & (1 << pos) != 0
    }

    /// True iff this mask handles `other` for signal routing purposes.
    ///
    /// Plain overlap: a mask catches a signal when the two share any bit, and
    /// no bit is privileged over another. `|:log|` catches `|:log :audit|`;
    /// `|:exec|` catches a subprocess request; `|:io|` catches the same one.
    ///
    /// This used to require the mask to name `SIG_IO` whenever the signal
    /// carried it, so that a fiber masking `|:yield|` could not swallow a
    /// request the scheduler has to service. That rule bought the guarantee at
    /// the cost of every OTHER bit in such a signal: `|:exec|` in a mask matched
    /// nothing, because the subprocess request it named also carried `:io`
    /// (#895). The guarantee now holds at the source — an I/O request raises
    /// `|:io|` and no longer carries `:yield`, so a `|:yield|` mask shares no
    /// bit with it and cannot catch it by accident.
    ///
    /// The empty signal is covered by every mask: a child that returned
    /// normally emitted nothing for a mask to miss.
    pub fn covers(self, other: SignalBits) -> bool {
        other.is_empty() || self.intersects(other)
    }

    // -- Combining -----------------------------------------------------------

    /// Bitwise OR (const-compatible union).
    pub const fn union(self, other: SignalBits) -> Self {
        SignalBits(self.0 | other.0)
    }

    /// Bitwise AND (const-compatible intersection).
    pub const fn intersection(self, other: SignalBits) -> Self {
        SignalBits(self.0 & other.0)
    }

    /// Bits in `self` that are NOT in `other` (const-compatible set difference).
    pub const fn subtract(self, other: SignalBits) -> Self {
        SignalBits(self.0 & !other.0)
    }

    /// Bitwise complement.
    pub const fn complement(self) -> Self {
        SignalBits(!self.0)
    }

    // -- Conversion / inspection ---------------------------------------------

    /// Position of the lowest set bit (for single-bit values).
    pub const fn trailing_zeros(self) -> u32 {
        self.0.trailing_zeros()
    }

    /// Raw bits as `u64`. Prefer named methods; use this only for
    /// serialization, FFI, or bytecode encoding.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl std::ops::BitOr for SignalBits {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        SignalBits(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for SignalBits {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        SignalBits(self.0 & rhs.0)
    }
}

impl std::ops::BitOrAssign for SignalBits {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAndAssign for SignalBits {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl std::ops::Not for SignalBits {
    type Output = Self;
    fn not(self) -> Self {
        SignalBits(!self.0)
    }
}

impl std::fmt::Debug for SignalBits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SignalBits(0x{:x})", self.0)
    }
}

impl std::fmt::Display for SignalBits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:x}", self.0)
    }
}

impl From<u64> for SignalBits {
    fn from(v: u64) -> Self {
        SignalBits::new(v)
    }
}

impl From<u32> for SignalBits {
    fn from(v: u32) -> Self {
        SignalBits::new(v as u64)
    }
}

impl From<SignalBits> for u64 {
    fn from(v: SignalBits) -> u64 {
        v.raw()
    }
}

#[cfg(test)]
mod tests;
