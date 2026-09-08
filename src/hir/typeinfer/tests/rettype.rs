// audited: 2026-09-07
// src/hir/AGENTS.md
// docs/intrinsics.md
//! What an op's result proves: every `%`-intrinsic and every constructor has a
//! result type, and that type is the next op's proof.

use super::{compile_fhir, compile_result, infer_and_rewrite, inferred_types};
use crate::hir::types::{TyId, TypeInterner};
use crate::hir::{Hir, HirId, HirKind};
use crate::symbol::SymbolTable;

/// The inferred type of the `%`-intrinsic node named `op` in `src`.
///
/// Sharper than `inferred_types`, which reports every node's type at once: over
/// int literals the operands put `Int` into that set themselves, so a family
/// member's own result rule can only be read off the op's own node.
fn intrinsic_result_type(src: &str, op: &str) -> TyId {
    fn find(h: &Hir, op: &str, found: &mut Option<HirId>) {
        if let HirKind::Intrinsic { op: this, .. } = &h.kind {
            if this.name() == op {
                *found = Some(h.id);
            }
        }
        h.for_each_child(|c| find(c, op, found));
    }
    let mut symbols = SymbolTable::new();
    let (mut hir, arena) = compile_fhir(src, &mut symbols);
    let info = infer_and_rewrite(&mut hir, &arena, &mut Default::default()).expect("infer");
    let mut found = None;
    find(&hir, op, &mut found);
    let id = found.unwrap_or_else(|| panic!("{src} compiled to no {op} node"));
    info.hir_types
        .get(&id)
        .copied()
        .unwrap_or_else(|| panic!("the {op} node in {src} carries no inferred type"))
}

/// Every member of the div family: `%add` and `%sub` and `%mul` share their
/// result rule with `%div`, `%rem` and `%mod`, so each row below runs over all
/// six and no member can drift away from the others.
const ARITHMETIC: [&str; 6] = ["%add", "%sub", "%mul", "%div", "%rem", "%mod"];

/// Two proven ints give Int, whichever member of the family runs.
///
/// The counter-factual is `%rem`, which was declared the constant Number: an
/// int remainder proved nothing downstream, so a bitwise op reading one was a
/// compile error and an `%add` reading one kept its polymorphic opcode.
#[test]
fn arithmetic_over_two_ints_is_int() {
    for op in ARITHMETIC {
        assert_eq!(
            intrinsic_result_type(&format!("({op} 7 2)"), op),
            TypeInterner::INT,
            "{op} over two proven ints must be Int"
        );
    }
}

/// Two proven floats give Float.
///
/// The counter-factual is `%mod`, which was declared the constant Int: a float
/// modulo then claimed to be an integer, and the bitwise contract believed it.
#[test]
fn arithmetic_over_two_floats_is_float() {
    for op in ARITHMETIC {
        assert_eq!(
            intrinsic_result_type(&format!("({op} 7.5 2.0)"), op),
            TypeInterner::FLOAT,
            "{op} over two proven floats must be Float"
        );
    }
}

/// A mixed pair joins to Number, which no bitwise op accepts. The over-narrowing
/// guard for the join: an int operand does not make the result an int.
#[test]
fn arithmetic_over_a_mixed_pair_is_number() {
    for op in ARITHMETIC {
        assert_eq!(
            intrinsic_result_type(&format!("({op} 7 2.0)"), op),
            TypeInterner::NUMBER,
            "{op} over one int and one float must be Number"
        );
    }
}

/// The chain, end to end: the remainder of two proven ints is a proven int, so
/// one guard carries a masking kernel through to the bitwise op.
///
/// The counter-factual: while `%rem` was typed Number this rejected with
/// "%bit-and: operand 1 is not a proven int (inferred: number)", so an integer
/// kernel that hashes or buckets through a remainder was unspellable.
#[test]
fn an_int_remainder_proves_a_bitwise_operand() {
    compile_result(
        "(defn hash1 [x] \
           (when (%not (%int? x)) (emit :error {:message \"hash1: int required\"})) \
           (%bit-and (%rem x 16) 15))",
    )
    .expect("the %rem of two proven ints is an Int and discharges %bit-and");
}

/// The other direction: a float modulo is a float at run time, so the bitwise
/// contract must reject it.
///
/// The counter-factual: while `%mod` was declared the constant Int this compiled,
/// and the bitwise opcode read the float's payload as an integer — `(%bit-and
/// (%mod 5.5 2.0) 1)` returned 0 where the `mod` wrapper raises :type-error.
#[test]
fn a_float_modulo_does_not_prove_a_bitwise_operand() {
    let err = compile_result("(%bit-and (%mod 5.5 2.0) 1)")
        .expect_err("a float %mod must not discharge the bitwise int contract");
    assert!(
        err.contains("%bit-and"),
        "error must name the op; got: {err}"
    );
}

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
