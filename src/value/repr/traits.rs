// audited: 2026-11-18
//! Trait implementations for Value (PartialEq, Eq, Hash, Ord).
//!
//! docs/types.md
//!
//! The `traits` field on heap variants is NOT compared by PartialEq, NOT
//! hashed by Hash, and NOT compared by Ord. A structural comparison reads the
//! contents and an identity comparison reads the slot or the handle, so the
//! trait table never decides an answer.

use super::Value;

mod hash;
mod kind;
mod ord;
mod view;

pub(super) use kind::stores_values;

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        super::eq::eq_with(self, other, super::eq::Relation::Identity)
    }
}

// NOTE: PartialEq is reflexive for all Value variants:
// - Immediate values: compared by tag+payload (always reflexive)
// - Heap structural types: compared by contents (reflexive by induction)
// - Heap identity types: compared by pointer (always reflexive)
//
// Float NaN == NaN (same bit pattern) since payload is f64::to_bits(),
// which violates IEEE 754 but satisfies Eq's reflexivity requirement.
// This is intentional — set membership requires reflexivity.
impl Eq for Value {}
