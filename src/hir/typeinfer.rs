//! Bidirectional type inference and the intrinsic operand proofs.
//!
//! Post-functionalize pass that:
//! 1. Infers types from literals, known return types, and type guards
//! 2. Propagates types through call sites (forward flow)
//! 3. Checks every call-position %-intrinsic against its operand contract
//!    (prove-or-reject; `contract.rs`)
//! 4. Narrows signals on primitive calls with provably typed args
//!    (delegates to `narrow.rs`)
//! 5. Re-propagates signals bottom-up after narrowing
//!
//! A stdlib wrapper call (`+`, `<`, `not`, …) is NEVER rewritten to its
//! intrinsic: the wrapper is the programmer's explicit request for the
//! validating, signaling, polymorphic surface, and substituting the silent
//! opcode would change both the failure mode and the site's signal profile.
//! The fast path is spelled `%add` — and proven.

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{Hir, HirId, HirKind, IntrinsicOp};
use super::types::{TyId, TypeInterner};
use crate::symbol::SymbolTable;

use std::collections::HashMap;

mod contract;
mod fuse;
pub(crate) use fuse::fuse_map_chains;
pub(crate) use fuse::{FnInlineRegistry, StoredFnInlineRegistry};
mod guard;
mod infer;
use infer::*;
mod monomorphize;
pub use monomorphize::DispatchWrapperRegistry;
pub(crate) use monomorphize::StoredDispatchRegistry;
mod prune;
pub(crate) use prune::prune_typeof_match_arms;

/// Result of type inference — currently just tracks whether the pass
/// found any immediates for region inference.
pub struct TypeInfo {
    pub hir_types: HashMap<HirId, TyId>,
}

const MAX_ITERS: usize = 10;

/// Run type inference and stdlib-to-intrinsic rewriting on functionalized HIR.
///
/// `Err` is the intrinsic operand proof obligation firing (see
/// `contract::check_intrinsic_operand_proofs`): a call-position `%`-intrinsic
/// whose operands are provably wrong or unprovable. Prove-or-reject is the
/// language — the check runs on every compile.
pub fn infer_and_rewrite(
    hir: &mut Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    dispatch_wrappers: &mut DispatchWrapperRegistry,
) -> Result<TypeInfo, String> {
    let interner = TypeInterner::new();
    // Build name lookup: SymbolId → name string, for matching callees
    let symbol_names = symbols.all_names();
    let mut binding_types: HashMap<Binding, TyId> = HashMap::new();
    let mut hir_types: HashMap<HirId, TyId> = HashMap::new();
    let mut binding_min_length: HashMap<Binding, usize> = HashMap::new();

    // Collect parameter info for lambdas: which bindings are params of which lambda
    let mut lambda_params: HashMap<Binding, Vec<Binding>> = HashMap::new();
    let mut lambda_body_type: HashMap<Binding, TyId> = HashMap::new();
    collect_lambda_info(hir, arena, &mut lambda_params);
    // A parameter mutated in its body has flow the per-pass recomputation
    // cannot see; it never receives call-site proofs (guards only).
    let mutated_params = collect_mutated_bindings(hir);
    // Bindings read anywhere but callee position — their param joins are
    // not proofs (see `collect_value_position_uses`).
    let mut value_used = std::collections::HashSet::new();
    collect_value_position_uses(hir, &mut value_used);
    // Immutable let-bound aliases of `(type-of a)` → subject `a`, so the
    // `(let [ta (type-of a)] (match ta …))` idiom narrows `a` like the inline
    // dispatch (`collect_typeof_aliases`).
    let mut typeof_aliases: HashMap<Binding, Binding> = HashMap::new();
    collect_typeof_aliases(hir, arena, &symbol_names, &mut typeof_aliases);
    // Kleene start: every parameter that CAN be proven by complete call-site
    // enumeration (callee-only binding, unmutated param) begins at BOTTOM, so
    // an identity-passed argument in a self/mutual recursion contributes
    // nothing on the way up instead of reading the Top default and pinning
    // itself there. Parameters of value-used bindings stay ABSENT (read as
    // Top): their callers are not enumerable, so optimism there would let the
    // checker pass on ⊥. A never-called callee-only function's params stay ⊥
    // — its %-sites can never execute, so nothing unsound compiles.
    for (b, params) in &lambda_params {
        if value_used.contains(b) {
            continue;
        }
        for p in params {
            if !mutated_params.contains(p) {
                binding_types.insert(*p, TypeInterner::BOTTOM);
            }
        }
    }

    // Inference to a fixpoint. Convergence is judged on the whole type
    // environment, not the root node's type: a call site visited late in a
    // pass joins into a callee parameter whose occurrences were recorded
    // earlier, so the refinement only reaches them on the next pass.
    for _ in 0..MAX_ITERS {
        let before_hir = hir_types.clone();
        let before_bindings = binding_types.clone();
        let mut param_joins: HashMap<Binding, TyId> = HashMap::new();
        infer_types(
            hir,
            &interner,
            arena,
            &mut binding_types,
            &mut hir_types,
            &lambda_params,
            &mut lambda_body_type,
            &symbol_names,
            &mut binding_min_length,
            &value_used,
            &typeof_aliases,
            &mut param_joins,
        );
        // REPLACE each contributed parameter's type with this pass's complete
        // join (Top included). A `(numeric!)` declaration floors the result at
        // Number (meet: callers can refine to Int/Float, never widen past the
        // declared contract); a mutated parameter never receives proofs.
        for (param, joined) in param_joins {
            if mutated_params.contains(&param) {
                continue;
            }
            binding_types.insert(param, declared_floor(param, joined, arena, &interner));
        }
        if before_hir == hir_types && before_bindings == binding_types {
            break;
        }
    }

    // Collapse container-dispatch wrapper calls (`(put s :x j)` with `s` a proven
    // concrete container → `(%put-struct-mut s :x j)`) so the multi-arm dispatch and
    // the container over-keep it strands cease to exist (`monomorphize.rs`; the F1b
    // close). Runs after the inference fixpoint (types are known) and before the
    // operand proofs, so each rewritten op is contract-checked by the same proof that
    // selected it.
    monomorphize::monomorphize_dispatch_wrappers(
        hir,
        &hir_types,
        arena,
        &symbol_names,
        &typeof_aliases,
        dispatch_wrappers,
    );

    // The prove-or-reject gate: every call-position %-intrinsic must discharge
    // its operand contract from the (narrowed, per-occurrence) inferred types.
    contract::check_intrinsic_operand_proofs(hir, &hir_types, arena, &symbol_names)?;

    // Signal narrowing: strip SIG_ERROR from calls with provably typed args
    super::narrow::narrow_signals(
        hir,
        &interner,
        arena,
        &symbol_names,
        &hir_types,
        &binding_min_length,
    );

    // Signal re-propagation: recompute parent signals bottom-up
    super::narrow::repropagate_signals(hir);

    Ok(TypeInfo { hir_types })
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

/// Extract arg count from a Call expression, unwrapping MakeCell if needed.
/// Returns Some(arg_count) for array/struct constructor calls.
fn unwrap_to_call(hir: &Hir) -> Option<usize> {
    match &hir.kind {
        HirKind::Call { args, .. } => Some(args.len()),
        HirKind::MakeCell { value } => unwrap_to_call(value),
        _ => None,
    }
}

/// Known return types for callable (stdlib) function calls.
///
/// Primitives carry their return type in the registry
/// (`PrimitiveDef::ret`, looked up through `def_by_name` — name and
/// alias spellings alike), so inference reads the same static tables
/// `register_primitives` feeds and cannot drift from them. The only
/// names matched here are stdlib *closures* (defined in stdlib.lisp,
/// not in any primitive table) whose pass-through typing inference
/// still wants.
fn primitive_return_type(name: &str, arg_types: &[TyId], _interner: &TypeInterner) -> TyId {
    use crate::primitives::def::RetType;

    // %thaw/thaw and %freeze/freeze map a container to its mutable/immutable
    // twin. The native's own RetType is polymorphic (Unknown), but the result
    // is a function of the operand — a fact inference can carry. This proves a
    // mutable-collection literal's type (`@"..."` desugars through `%thaw` of a
    // string literal), so it can flow into a %-store op's proven container
    // contract (e.g. `(%string-push @"" "x")`), and types explicit
    // `thaw`/`freeze` calls on a proven container likewise.
    let arg0 = || arg_types.first().copied().unwrap_or(TypeInterner::TOP);
    match name {
        "%thaw" | "thaw" => return mutable_twin(arg0()),
        "%freeze" | "freeze" => return immutable_twin(arg0()),
        _ => {}
    }

    if let Some(def) = crate::primitives::registration::def_by_name(name) {
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

    match name {
        // stdlib.lisp closures (not primitives): mutating pass-throughs
        // that return their first argument.
        "push" | "put" => arg_types.first().copied().unwrap_or(TypeInterner::TOP),
        // The arithmetic wrappers validate their operands and raise on
        // anything non-numeric, so on every path that RETURNS the result is a
        // Number — the same stable-name authority the guard recognition uses
        // for the predicates. This is what lets `(%lt (- b a) 2)`-style
        // measurement code prove its operands without a hand guard.
        "+" | "-" | "*" | "/" | "rem" | "mod" | "abs" | "min" | "max" | "inc" | "dec" | "sum"
        | "product" => TypeInterner::NUMBER,
        "floor" | "ceil" | "round" => TypeInterner::NUMBER,
        _ => TypeInterner::TOP,
    }
}

/// Known return types for intrinsic operations.
fn intrinsic_return_type(
    op: IntrinsicOp,
    args: &[Hir],
    interner: &TypeInterner,
    hir_types: &HashMap<HirId, TyId>,
) -> TyId {
    match op {
        // Arithmetic: returns the join of arg types within Number
        IntrinsicOp::Add | IntrinsicOp::Sub | IntrinsicOp::Mul | IntrinsicOp::Div => {
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
        IntrinsicOp::Rem => TypeInterner::NUMBER,
        IntrinsicOp::Mod => TypeInterner::INT,

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

        // Monomorphic array push: the variant pins the result type (the whole point
        // of monomorphization — the polymorphic %array-push stays Top/FirstArg).
        // %push-array yields a fresh immutable Array twin; %push-array-mut stores in
        // place and returns its mutable arg0 (MutableArray).
        IntrinsicOp::PushArray => TypeInterner::ARRAY,
        IntrinsicOp::PushArrayMut => TypeInterner::MUTABLE_ARRAY,

        // Monomorphic put variants: the variant pins the result type (the polymorphic
        // %put stays Top). Immutable variants yield a fresh immutable twin; -mut stores
        // in place and returns its mutable arg0.
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
