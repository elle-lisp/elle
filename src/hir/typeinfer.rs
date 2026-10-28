// audited: 2026-10-28
//! Type inference, the fold it drives, and the intrinsic operand proofs.
//!
//! The pass runs on functionalized HIR:
//! 1. Infer types from literals, known return types, and type guards.
//! 2. Propagate types through call sites and through the operands a graft
//!    bound (the ascent in `infer/fixpoint.rs`).
//! 3. Delete the branches and dispatch arms the inferred types decide, and
//!    infer again until nothing more goes (`fold.rs`).
//! 4. Check every call-position `%`-intrinsic against its operand contract
//!    (`contract.rs`).
//! 5. Narrow the signals of primitive calls whose operand types rule out the
//!    error (`narrow.rs`).
//! 6. Re-propagate signals bottom-up after the narrowing.
//!
//! A wrapper call reaches its `%`-op by being grafted before this pass runs
//! (`graft.rs`). Nothing here rewrites a call by its callee's name.
//!
//! docs/impl/typeinfer.md
//! docs/impl/graft.md
//! docs/intrinsics.md

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{Hir, HirKind, IntrinsicOp};
use super::types::{NodeTypes, TyId, TypeInterner};
use crate::value::SymbolId;

use std::collections::HashMap;

mod contract;
mod fold;
mod fuse;
pub(crate) use fuse::fuse_map_chains;
pub(crate) mod graft;
mod guard;
mod infer;
use infer::*;
mod prune;
pub(crate) use prune::prune_typeof_match_arms;

/// What type inference hands the LIR lowerer: the type of every node, with no
/// Bottom left in the map.
pub struct TypeInfo {
    pub hir_types: NodeTypes,
}

/// Run type inference and the fold on functionalized HIR, then check the
/// operand proofs and narrow the signals over what the fold left.
///
/// `record` is what the graft bound to the parameters of this unit's
/// functions, joined into them as call sites.
///
/// `Err` names a call-position `%`-intrinsic whose operands are provably wrong
/// or unprovable (`contract::check_intrinsic_operand_proofs`).
pub(crate) fn infer_and_rewrite(
    hir: &mut Hir,
    arena: &BindingArena,
    record: &graft::GraftRecord,
) -> Result<TypeInfo, String> {
    let interner = TypeInterner::new();

    // The ascent: one context collects the unit's program facts, then iterates
    // the transfer function until the type environment stops moving. The fold
    // then deletes what those types decide. A deletion can narrow a join, so
    // the two alternate until a fold deletes nothing. Each round hands what it
    // read off the graft record to the next, which never reads it wider.
    let mut infer = Infer::new(hir, arena, record, graft::Readings::unread(record));
    infer.solve(hir);
    while fold::fold(hir, &infer.hir_types, arena) {
        infer = Infer::new(hir, arena, record, infer.readings);
        infer.solve(hir);
    }

    // The prove-or-reject gate: every call-position %-intrinsic must discharge
    // its operand contract from the (narrowed, per-occurrence) inferred types.
    contract::check_intrinsic_operand_proofs(hir, &infer.hir_types, arena)?;

    // Signal narrowing: strip SIG_ERROR from calls with provably typed args
    super::narrow::narrow_signals(
        hir,
        &interner,
        arena,
        &infer.hir_types,
        &infer.binding_min_length,
    );

    // Signal re-propagation: recompute parent signals bottom-up
    super::narrow::repropagate_signals(hir);

    Ok(TypeInfo {
        hir_types: infer.hir_types,
    })
}

/// Extract the binding from a callee expression.
/// Handles both `Var(b)` and `DerefCell { Var(b) }` (letrec recursive calls).
pub(super) fn unwrap_callee_binding(func: &Hir) -> Option<Binding> {
    match &func.kind {
        HirKind::Var(b) => Some(*b),
        HirKind::DerefCell { cell } => {
            if let HirKind::Var(b) = &cell.kind {
                Some(*b)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The operand count of a call, looking through a `MakeCell`. The binder reads
/// it as the length of an array a call builds.
fn unwrap_to_call(hir: &Hir) -> Option<usize> {
    match &hir.kind {
        HirKind::Call { args, .. } => Some(args.len()),
        HirKind::MakeCell { value } => unwrap_to_call(value),
        _ => None,
    }
}

/// The type a call to a binding the runtime supplies returns.
///
/// A primitive carries its return type in the registry (`PrimitiveDef::ret`,
/// looked up through `def_by_symbol`), so inference reads the tables
/// `register_primitives` feeds. The names matched here are the thaw and
/// freeze pair, whose result depends on the operand, and the stdlib closures,
/// which are in no primitive table.
fn primitive_return_type(sym: SymbolId, arg_types: &[TyId], _interner: &TypeInterner) -> TyId {
    use crate::primitives::def::RetType;

    // Recognition is by id, not by a resolved spelling: `SymbolId::of` is a
    // `const fn` over the name's hash, so these are integer compares against
    // constants and need no symbol table (docs/impl/symbol.md). The stdlib
    // closures below are in no primitive table, so nothing built at compile
    // time could hand back their names.
    const PCT_THAW: SymbolId = SymbolId::of("%thaw");
    const THAW: SymbolId = SymbolId::of("thaw");
    const PCT_FREEZE: SymbolId = SymbolId::of("%freeze");
    const FREEZE: SymbolId = SymbolId::of("freeze");

    // %thaw/thaw and %freeze/freeze map a container to its mutable/immutable
    // twin. The native's own RetType is Unknown, but the result is a function
    // of the operand. This types a mutable-collection literal (`@"..."`
    // desugars through `%thaw` of a string literal), so it can prove a
    // `%`-store op's container, for example `(%string-push-mut @"" "x")`.
    let arg0 = || arg_types.first().copied().unwrap_or(TypeInterner::TOP);
    match sym {
        PCT_THAW | THAW => return mutable_twin(arg0()),
        PCT_FREEZE | FREEZE => return immutable_twin(arg0()),
        _ => {}
    }

    if let Some(def) = crate::primitives::registration::def_by_symbol(sym) {
        return match def.ret {
            RetType::Unknown => TypeInterner::TOP,
            RetType::Int => TypeInterner::INT,
            RetType::Float => TypeInterner::FLOAT,
            RetType::Bool => TypeInterner::BOOL,
            RetType::String => TypeInterner::STRING,
            RetType::MutableString => TypeInterner::MUTABLE_STRING,
            RetType::Keyword => TypeInterner::KEYWORD,
            RetType::Bytes => TypeInterner::BYTES,
            RetType::MutableBytes => TypeInterner::MUTABLE_BYTES,
            RetType::Array => TypeInterner::ARRAY,
            RetType::MutableArray => TypeInterner::MUTABLE_ARRAY,
            RetType::Struct => TypeInterner::STRUCT,
            RetType::MutableStruct => TypeInterner::MUTABLE_STRUCT,
            RetType::Set => TypeInterner::SET,
            RetType::MutableSet => TypeInterner::MUTABLE_SET,
            // No `TyKind::Fiber` exists in the lattice (no operand contract
            // consumes one); the declaration's consumers are the `type-of`
            // dispatch prune (which reads `RetType` directly through
            // `keyword_of_rettype`) and the ownership forest's fiber-member
            // refusal (`RegionInfo::fiber_result_regions`).
            RetType::Fiber => TypeInterner::TOP,
            RetType::FirstArg => arg_types.first().copied().unwrap_or(TypeInterner::TOP),
        };
    }

    const PUSH: SymbolId = SymbolId::of("push");
    const PUT: SymbolId = SymbolId::of("put");
    const ADD: SymbolId = SymbolId::of("+");
    const SUB: SymbolId = SymbolId::of("-");
    const MUL: SymbolId = SymbolId::of("*");
    const DIV: SymbolId = SymbolId::of("/");
    const REM: SymbolId = SymbolId::of("rem");
    const MOD: SymbolId = SymbolId::of("mod");
    const ABS: SymbolId = SymbolId::of("abs");
    const MIN: SymbolId = SymbolId::of("min");
    const MAX: SymbolId = SymbolId::of("max");
    const INC: SymbolId = SymbolId::of("inc");
    const DEC: SymbolId = SymbolId::of("dec");
    const SUM: SymbolId = SymbolId::of("sum");
    const PRODUCT: SymbolId = SymbolId::of("product");
    const FLOOR: SymbolId = SymbolId::of("floor");
    const CEIL: SymbolId = SymbolId::of("ceil");
    const ROUND: SymbolId = SymbolId::of("round");
    match sym {
        // stdlib.lisp closures (not primitives): mutating pass-throughs
        // that return their first argument.
        PUSH | PUT => arg_types.first().copied().unwrap_or(TypeInterner::TOP),
        // The arithmetic wrappers raise on anything non-numeric, so every
        // path that returns yields a Number. This types a wrapper call the
        // graft left a call, such as `(+ a b c)`.
        ADD | SUB | MUL | DIV | REM | MOD | ABS | MIN | MAX | INC | DEC | SUM | PRODUCT => {
            TypeInterner::NUMBER
        }
        FLOOR | CEIL | ROUND => TypeInterner::NUMBER,
        _ => TypeInterner::TOP,
    }
}

/// Known return types for intrinsic operations.
fn intrinsic_return_type(
    op: IntrinsicOp,
    args: &[Hir],
    interner: &TypeInterner,
    hir_types: &NodeTypes,
) -> TyId {
    match op {
        // Arithmetic, the whole div family included: the join of the arg types
        // within Number. On every tier each member gives an integer only where
        // both operands are integers. So one rule serves all six, and a
        // remainder in the middle of an int chain keeps the chain proven.
        IntrinsicOp::Add
        | IntrinsicOp::Sub
        | IntrinsicOp::Mul
        | IntrinsicOp::Div
        | IntrinsicOp::Rem
        | IntrinsicOp::Mod => {
            let mut ty = TypeInterner::BOTTOM;
            for arg in args {
                let arg_ty = hir_types.get(&arg.id).copied().unwrap_or(TypeInterner::TOP);
                ty = interner.join(ty, arg_ty);
            }
            // Clamp to Number (intrinsics only operate on numbers)
            if interner.subtype(ty, TypeInterner::NUMBER) {
                ty
            } else {
                TypeInterner::NUMBER
            }
        }

        // Comparison: returns Bool
        IntrinsicOp::Eq
        | IntrinsicOp::Ne
        | IntrinsicOp::Lt
        | IntrinsicOp::Gt
        | IntrinsicOp::Le
        | IntrinsicOp::Ge => TypeInterner::BOOL,

        // Logical: returns Bool
        IntrinsicOp::Not => TypeInterner::BOOL,

        // Type predicates: return Bool
        IntrinsicOp::IsNil
        | IntrinsicOp::IsEmpty
        | IntrinsicOp::IsBool
        | IntrinsicOp::IsInt
        | IntrinsicOp::IsFloat
        | IntrinsicOp::IsString
        | IntrinsicOp::IsKeyword
        | IntrinsicOp::IsSymbol
        | IntrinsicOp::IsPair
        | IntrinsicOp::IsArray
        | IntrinsicOp::IsStruct
        | IntrinsicOp::IsSet
        | IntrinsicOp::IsBytes
        | IntrinsicOp::IsBox
        | IntrinsicOp::IsClosure
        | IntrinsicOp::IsFiber
        | IntrinsicOp::Identical => TypeInterner::BOOL,

        // Conversions
        IntrinsicOp::Int => TypeInterner::INT,
        IntrinsicOp::Float => TypeInterner::FLOAT,

        // A monomorphic push or put pins its result type; the polymorphic
        // %array-push and %put fall to Top below. An immutable variant yields
        // a fresh immutable container, and a -mut variant stores in place and
        // returns its mutable arg0.
        IntrinsicOp::PushArray => TypeInterner::ARRAY,
        IntrinsicOp::PushArrayMut => TypeInterner::MUTABLE_ARRAY,
        IntrinsicOp::PutStruct => TypeInterner::STRUCT,
        IntrinsicOp::PutStructMut => TypeInterner::MUTABLE_STRUCT,
        IntrinsicOp::PutArray => TypeInterner::ARRAY,
        IntrinsicOp::PutArrayMut => TypeInterner::MUTABLE_ARRAY,

        // Pair: the constructor pins its result type (feeds the %first/%rest
        // operand contract); element types are untracked, so First/Rest stay Top.
        IntrinsicOp::Pair => TypeInterner::PAIR,
        IntrinsicOp::First | IntrinsicOp::Rest => TypeInterner::TOP,

        // Freeze/Thaw are copying ops that route through the native funnel Call
        // (`routes_native_funnel`), so they are typed by `primitive_return_type`
        // by name, not here.

        // Bitwise: return Int
        IntrinsicOp::BitAnd
        | IntrinsicOp::BitOr
        | IntrinsicOp::BitXor
        | IntrinsicOp::BitNot
        | IntrinsicOp::Shl
        | IntrinsicOp::Shr => TypeInterner::INT,

        // TypeOf returns keyword
        IntrinsicOp::TypeOf => TypeInterner::KEYWORD,

        // Length returns Int
        IntrinsicOp::Length => TypeInterner::INT,

        // Everything else
        _ => TypeInterner::TOP,
    }
}

/// The mutable counterpart of a container type (`%thaw`'s result). An
/// already-mutable or immutable container maps to its mutable twin; a
/// non-container type has no twin and stays Top.
fn mutable_twin(ty: TyId) -> TyId {
    if ty == TypeInterner::STRING || ty == TypeInterner::MUTABLE_STRING {
        TypeInterner::MUTABLE_STRING
    } else if ty == TypeInterner::ARRAY || ty == TypeInterner::MUTABLE_ARRAY {
        TypeInterner::MUTABLE_ARRAY
    } else if ty == TypeInterner::BYTES || ty == TypeInterner::MUTABLE_BYTES {
        TypeInterner::MUTABLE_BYTES
    } else if ty == TypeInterner::STRUCT || ty == TypeInterner::MUTABLE_STRUCT {
        TypeInterner::MUTABLE_STRUCT
    } else if ty == TypeInterner::SET || ty == TypeInterner::MUTABLE_SET {
        TypeInterner::MUTABLE_SET
    } else {
        TypeInterner::TOP
    }
}

/// The immutable counterpart of a container type (`%freeze`'s result). The
/// inverse of [`mutable_twin`].
fn immutable_twin(ty: TyId) -> TyId {
    if ty == TypeInterner::STRING || ty == TypeInterner::MUTABLE_STRING {
        TypeInterner::STRING
    } else if ty == TypeInterner::ARRAY || ty == TypeInterner::MUTABLE_ARRAY {
        TypeInterner::ARRAY
    } else if ty == TypeInterner::BYTES || ty == TypeInterner::MUTABLE_BYTES {
        TypeInterner::BYTES
    } else if ty == TypeInterner::STRUCT || ty == TypeInterner::MUTABLE_STRUCT {
        TypeInterner::STRUCT
    } else if ty == TypeInterner::SET || ty == TypeInterner::MUTABLE_SET {
        TypeInterner::SET
    } else {
        TypeInterner::TOP
    }
}

// Guard recognition lives in `guard.rs` (`cond_facts`): intrinsic and
// Call-form predicates alike, `%not`/`not` negation, and the zero-tests that
// feed the div-family nonzero obligation.

#[cfg(test)]
mod tests;
