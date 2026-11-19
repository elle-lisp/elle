// audited: 2026-11-18
//! The parameter limit, and the bit each parameter owns in a one-word parameter mask.
//!
//! docs/functions.md

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
