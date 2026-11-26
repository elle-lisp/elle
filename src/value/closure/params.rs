// audited: 2026-11-26
//! The parameter limit, the bit each parameter owns in a parameter mask, and how a variadic lambda collects its rest.
//!
//! docs/functions.md
//! docs/impl/region/restlist.md

/// The most parameters a function takes. The parameter mask is one word, so
/// the analyzer refuses a longer list.
pub const MAX_PARAMS: usize = 64;

/// The bit of parameter `i` in a parameter mask. Panics for `i` at
/// `MAX_PARAMS` or past it: no function the analyzer admits has that
/// parameter.
#[inline]
pub fn param_bit(i: usize) -> u64 {
    assert!(
        i < MAX_PARAMS,
        "parameter {i} past the {MAX_PARAMS}-parameter limit"
    );
    1 << i
}

/// How a lambda's rest parameter collects its arguments. The `&named` key set
/// itself is variable-length, so it lives beside this tag in the payload rather
/// than inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VarargTag {
    /// `&rest` — collect into a list.
    List,
    /// `&keys` — collect into an immutable struct, any key.
    Struct,
    /// `&named` — collect into an immutable struct, keys validated against the
    /// declared set ([`super::CodePayload::strict_keys`]).
    StrictStruct,
}

/// How a variadic call builds its `&` rest list: one region per cell, or one
/// region for the whole list. The region analysis proves which one a lambda may
/// take (docs/impl/region/restlist.md); every carrier that cannot say falls
/// back to `PerCell`, which is always correct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RestListLayout {
    /// Each cell in a region of its own, so a tail the callee hands out
    /// outlives the head it came from.
    PerCell,
    /// Every cell in one region, freed with the head. Admitted only where no
    /// cell can outlive the head.
    OneRegion,
}

impl RestListLayout {
    /// The layout a JIT prologue passes its helper as a `u32`, the inverse of
    /// `layout as u32`.
    pub fn from_raw(raw: u32) -> Self {
        if raw == RestListLayout::OneRegion as u32 {
            return RestListLayout::OneRegion;
        }
        debug_assert_eq!(
            raw,
            RestListLayout::PerCell as u32,
            "a JIT prologue passes a layout the type names"
        );
        RestListLayout::PerCell
    }
}
