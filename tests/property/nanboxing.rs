// Unit tests for tagged-union Value representation.
//
// These tests verify the fundamental invariants of the Value type:
// roundtrip fidelity, type discrimination, truthiness, and equality.
// Converted from property tests to deterministic unit tests with concrete cases.

use elle::Value;

mod scalars {
    include!("nanboxing/scalars.rs");
}

mod heap {
    include!("nanboxing/heap.rs");
}
