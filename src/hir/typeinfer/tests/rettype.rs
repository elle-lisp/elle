// audited: 2026-09-07
// src/hir/AGENTS.md
// docs/intrinsics.md
//! What an op's result proves: every `%`-intrinsic and every constructor has a
//! result type, and that type is the next op's proof.

use super::inferred_types;
use crate::hir::types::TypeInterner;

/// Set constructors carry their declared RetType, read through `def_by_name`:
/// `(set …)` is `Set`, `(@set …)` is `MutableSet`.
#[test]
fn set_constructors_infer_their_declared_rettype() {
    assert!(inferred_types("(set 1 2)").contains(&TypeInterner::SET));
    assert!(inferred_types("(@set 1 2)").contains(&TypeInterner::MUTABLE_SET));
}

/// Constructor RetType forward-inference: `@string` always builds a fresh
/// mutable string (`RetType::MutableString`), and — now that their mutability
/// is fixed by the constructor rather than the argument (bytes.rs) —
/// `bytes`/`@bytes` declare `Bytes`/`MutableBytes`. The snippet is just the
/// constructor call, so the asserted type can only come from the declared
/// `RetType` read through `def_by_name`.
#[test]
fn constructors_infer_their_declared_rettype() {
    assert!(inferred_types("(@string)").contains(&TypeInterner::MUTABLE_STRING));
    assert!(inferred_types("(bytes 1 2)").contains(&TypeInterner::BYTES));
    assert!(inferred_types("(@bytes 1 2)").contains(&TypeInterner::MUTABLE_BYTES));
}

/// Monomorphic `%push-array-mut` pins its result `MutableArray` from the *op*,
/// not the input: applied to an *immutable* `[1 2]` the result is still
/// `MutableArray` (the funnel store returns arg0, which the `-mut` variant
/// asserts is mutable). Since the input is immutable `Array`, the only source of
/// `MUTABLE_ARRAY` in `hir_types` is the op's declared return type. Counter-factual
/// against a `FirstArg`/`Unknown` return that would inherit the input's immutability.
#[test]
fn push_array_mut_result_is_mutable_array_from_the_op() {
    assert!(
        inferred_types("(%push-array-mut [1 2] 3)").contains(&TypeInterner::MUTABLE_ARRAY),
        "%push-array-mut result must be MutableArray even on an immutable input"
    );
}

/// Symmetric: monomorphic `%push-array` (immutable) returns a fresh immutable
/// `Array` even when applied to a *mutable* `@[1 2]`. The input is `MutableArray`,
/// so the `ARRAY` TyId can only originate in the op's declared return type — the
/// Fresh immutable twin. Counter-factual against returning `FirstArg`.
#[test]
fn push_array_result_is_immutable_array_from_the_op() {
    assert!(
        inferred_types("(%push-array @[1 2] 3)").contains(&TypeInterner::ARRAY),
        "%push-array result must be immutable Array even on a mutable input"
    );
}

/// `%put-struct-mut` pins `MutableStruct` from the op: applied to an *immutable*
/// `{:a 1}` the only source of `MUTABLE_STRUCT` in `hir_types` is the op's return
/// type (the input is immutable `Struct`). Counter-factual against a `FirstArg`
/// return inheriting the input's immutability.
#[test]
fn put_struct_mut_result_is_mutable_struct_from_the_op() {
    assert!(
        inferred_types("(%put-struct-mut {:a 1} :b 2)").contains(&TypeInterner::MUTABLE_STRUCT),
        "%put-struct-mut result must be MutableStruct even on an immutable input"
    );
}

/// Symmetric: `%put-struct` (immutable) returns a fresh `Struct` even on a mutable
/// `@{:a 1}` input (whose own type is `MutableStruct`).
#[test]
fn put_struct_result_is_immutable_struct_from_the_op() {
    assert!(
        inferred_types("(%put-struct @{:a 1} :b 2)").contains(&TypeInterner::STRUCT),
        "%put-struct result must be immutable Struct even on a mutable input"
    );
}

/// `%put-array-mut` pins `MutableArray` from the op on an immutable `[1 2]` input.
#[test]
fn put_array_mut_result_is_mutable_array_from_the_op() {
    assert!(
        inferred_types("(%put-array-mut [1 2] 0 9)").contains(&TypeInterner::MUTABLE_ARRAY),
        "%put-array-mut result must be MutableArray even on an immutable input"
    );
}

/// `%put-array` (immutable) returns a fresh `Array` even on a mutable `@[1 2]` input.
#[test]
fn put_array_result_is_immutable_array_from_the_op() {
    assert!(
        inferred_types("(%put-array @[1 2] 0 9)").contains(&TypeInterner::ARRAY),
        "%put-array result must be immutable Array even on a mutable input"
    );
}
