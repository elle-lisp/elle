// audited: 2026-10-12
//! `ArityRec`: a function's arity as a payload field with every byte written.
//!
//! docs/impl/region/template.md
//!
//! `Arity` is an enum whose shorter variants leave their payload bytes
//! unwritten, and a code payload is copied whole into its code region, so the
//! payload stores the arity as this record instead: two counts, a kind byte,
//! and a pad written zero.

use crate::value::types::Arity;

/// Which `Arity` variant an [`ArityRec`] stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ArityKind {
    Exact,
    AtLeast,
    Range,
}

/// An `Arity` as twelve bytes, every one of them written. `Exact(n)` and
/// `AtLeast(n)` keep `n` in `min` and zero in `max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct ArityRec {
    pub(crate) min: u32,
    pub(crate) max: u32,
    pub(crate) kind: ArityKind,
    pub(crate) pad: [u8; 3],
}

impl ArityRec {
    /// The arity this record stores.
    pub fn arity(self) -> Arity {
        let (min, max) = (self.min as usize, self.max as usize);
        match self.kind {
            ArityKind::Exact => Arity::Exact(min),
            ArityKind::AtLeast => Arity::AtLeast(min),
            ArityKind::Range => Arity::Range(min, max),
        }
    }
}

impl From<Arity> for ArityRec {
    fn from(arity: Arity) -> Self {
        let count = |n: usize| u32::try_from(n).expect("an arity count fits a u32");
        let (kind, min, max) = match arity {
            Arity::Exact(n) => (ArityKind::Exact, count(n), 0),
            Arity::AtLeast(n) => (ArityKind::AtLeast, count(n), 0),
            Arity::Range(min, max) => (ArityKind::Range, count(min), count(max)),
        };
        ArityRec {
            min,
            max,
            kind,
            pad: [0; 3],
        }
    }
}
