//! Bidirectional type inference and stdlib-to-intrinsic rewriting.
//!
//! Post-functionalize pass that:
//! 1. Infers types from literals, known return types, and type guards
//! 2. Propagates types through call sites (forward flow)
//! 3. Rewrites stdlib arithmetic/comparison calls to intrinsics when
//!    argument types prove ⊑ Number
//! 4. Updates signals for rewritten nodes (intrinsics are silent)
//! 5. Narrows signals on primitive calls with provably typed args
//!    (delegates to `narrow.rs`)
//! 6. Re-propagates signals bottom-up after narrowing
//!
//! The pass iterates to a fixed point: type refinements enable rewrites,
//! which change signals, which enable further refinements.

use super::arena::BindingArena;
use super::binding::Binding;
use super::expr::{Hir, HirId, HirKind, IntrinsicOp};
use super::types::{TyId, TypeInterner};
use crate::signals::Signal;
use crate::symbol::SymbolTable;

use std::collections::HashMap;

mod infer;
use infer::*;
mod prune;
pub(crate) use prune::prune_typeof_match_arms;

/// Result of type inference — currently just tracks whether the pass
/// found any immediates for region inference.
pub struct TypeInfo {
    pub hir_types: HashMap<HirId, TyId>,
}

/// Which stdlib function maps to which intrinsic, and what type constraint.
struct RewriteRule {
    op: IntrinsicOp,
    arity: (usize, usize),
    /// Required type for all arguments (None = always valid)
    constraint: Option<TyId>,
}

/// Build the rewrite table mapping function names to intrinsic rewrites.
fn build_rewrite_table() -> HashMap<&'static str, RewriteRule> {
    let mut table = HashMap::new();
    let number = Some(TypeInterner::NUMBER);

    let mut add =
        |name: &'static str, op: IntrinsicOp, arity: (usize, usize), constraint: Option<TyId>| {
            table.insert(
                name,
                RewriteRule {
                    op,
                    arity,
                    constraint,
                },
            );
        };

    // Arithmetic (require Number)
    // Note: / , rem, mod have division-by-zero checks in stdlib that %div/%rem/%mod bypass.
    // Only rewrite operations that are total on Number.
    add("+", IntrinsicOp::Add, (2, 2), number);
    add("-", IntrinsicOp::Sub, (1, 2), number);
    add("*", IntrinsicOp::Mul, (2, 2), number);

    // Comparison (require Number — stdlib also accepts strings/keywords
    // but we only rewrite when we know it's numeric)
    add("<", IntrinsicOp::Lt, (2, 2), number);
    add(">", IntrinsicOp::Gt, (2, 2), number);
    add("<=", IntrinsicOp::Le, (2, 2), number);
    add(">=", IntrinsicOp::Ge, (2, 2), number);

    // Equality (always valid)
    add("=", IntrinsicOp::Eq, (2, 2), None);

    // Logical (always valid)
    add("not", IntrinsicOp::Not, (1, 1), None);

    table
}

const MAX_ITERS: usize = 10;

/// Run type inference and stdlib-to-intrinsic rewriting on functionalized HIR.
///
/// `Err` is the monomorphization proof obligation firing (see
/// `check_monomorphic_proof_obligations`): a silent monomorphic container op
/// whose container is not statically proven. Only the silent path can raise it —
/// the checked path early-returns before inference runs.
pub fn infer_and_rewrite(
    hir: &mut Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
) -> Result<TypeInfo, String> {
    // When --checked-intrinsics is active, intrinsics route through
    // type-checked NativeFn paths. Don't rewrite to bypass those checks.
    if crate::config::get().checked_intrinsics {
        return Ok(TypeInfo {
            hir_types: HashMap::new(),
        });
    }

    let interner = TypeInterner::new();
    let rewrite_table = build_rewrite_table();
    // Build name lookup: SymbolId → name string, for matching callees
    let symbol_names = symbols.all_names();
    let mut binding_types: HashMap<Binding, TyId> = HashMap::new();
    let mut hir_types: HashMap<HirId, TyId> = HashMap::new();
    let mut binding_min_length: HashMap<Binding, usize> = HashMap::new();

    // Collect parameter info for lambdas: which bindings are params of which lambda
    let mut lambda_params: HashMap<Binding, Vec<Binding>> = HashMap::new();
    let mut lambda_body_type: HashMap<Binding, TyId> = HashMap::new();
    collect_lambda_info(hir, arena, &mut lambda_params);

    for _ in 0..MAX_ITERS {
        let mut changed = false;

        // Forward type inference
        changed |= infer_types(
            hir,
            &interner,
            arena,
            &mut binding_types,
            &mut hir_types,
            &lambda_params,
            &mut lambda_body_type,
            &symbol_names,
            &mut binding_min_length,
        );

        // Rewrite stdlib calls to intrinsics where types prove it's safe
        changed |= rewrite_calls(
            hir,
            &interner,
            arena,
            &rewrite_table,
            &symbol_names,
            &binding_types,
            &hir_types,
        );

        if !changed {
            break;
        }
    }

    // Proof obligation: a silent monomorphic container op must have its container
    // statically proven (no runtime guard exists on this path to catch a mismatch).
    check_monomorphic_proof_obligations(hir, &hir_types)?;

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
/// alias spellings alike), so inference reads the same const tables
/// `register_primitives` feeds and cannot drift from them. The only
/// names matched here are stdlib *closures* (defined in stdlib.lisp,
/// not in any primitive table) whose pass-through typing inference
/// still wants.
fn primitive_return_type(name: &str, arg_types: &[TyId], _interner: &TypeInterner) -> TyId {
    use crate::primitives::def::RetType;

    if let Some(def) = crate::primitives::registration::def_by_name(name) {
        return match def.ret {
            RetType::Unknown => TypeInterner::TOP,
            RetType::Int => TypeInterner::INT,
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
            RetType::FirstArg => arg_types.first().copied().unwrap_or(TypeInterner::TOP),
        };
    }

    match name {
        // stdlib.lisp closures (not primitives): mutating pass-throughs
        // that return their first argument.
        "push" | "put" => arg_types.first().copied().unwrap_or(TypeInterner::TOP),
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

        // Pair
        IntrinsicOp::Pair => TypeInterner::TOP,
        IntrinsicOp::First | IntrinsicOp::Rest => TypeInterner::TOP,

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

/// Extract type guard information from an If condition.
/// Returns `(binding, narrowed_type)` if the condition is a type predicate.
fn extract_type_guard(cond: &Hir, _arena: &BindingArena) -> Option<(Binding, TyId)> {
    match &cond.kind {
        // Direct type predicate: (%int? x), (%float? x), etc.
        HirKind::Intrinsic { op, args } if args.len() == 1 => {
            let binding = match &args[0].kind {
                HirKind::Var(b) => *b,
                HirKind::DerefCell { cell } => {
                    if let HirKind::Var(b) = &cell.kind {
                        *b
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
            let ty = match op {
                IntrinsicOp::IsInt => TypeInterner::INT,
                IntrinsicOp::IsFloat => TypeInterner::FLOAT,
                IntrinsicOp::IsString => TypeInterner::STRING,
                IntrinsicOp::IsKeyword => TypeInterner::KEYWORD,
                IntrinsicOp::IsSymbol => TypeInterner::SYMBOL,
                IntrinsicOp::IsBool => TypeInterner::BOOL,
                IntrinsicOp::IsNil => TypeInterner::NIL,
                _ => return None,
            };
            Some((binding, ty))
        }
        // Call to type predicate: (number? x), (integer? x), etc.
        // These haven't been rewritten to intrinsics yet since they're stdlib calls
        // Stdlib type predicate calls are handled after they get rewritten to intrinsics
        HirKind::Call { .. } => None,
        _ => None,
    }
}

/// Rewrite stdlib calls to intrinsics where types prove it's safe.
/// Returns true if any rewrites were applied.
fn rewrite_calls(
    hir: &mut Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    rewrite_table: &HashMap<&str, RewriteRule>,
    symbol_names: &HashMap<u32, String>,
    binding_types: &HashMap<Binding, TyId>,
    hir_types: &HashMap<HirId, TyId>,
) -> bool {
    let mut changed = false;

    // First, try to rewrite this node
    if let HirKind::Call { func, args, .. } = &hir.kind {
        if let HirKind::Var(callee_binding) = &func.kind {
            let callee_sym = arena.get(*callee_binding).name;
            // Look up name from SymbolId
            if let Some(name) = symbol_names.get(&callee_sym.0) {
                if let Some(rule) = rewrite_table.get(name.as_str()) {
                    let arg_count = args.len();
                    if arg_count >= rule.arity.0 && arg_count <= rule.arity.1 {
                        // Check type constraint
                        let types_ok = match rule.constraint {
                            None => true,
                            Some(constraint_ty) => args.iter().all(|arg| {
                                let arg_ty = hir_types
                                    .get(&arg.expr.id)
                                    .copied()
                                    .unwrap_or(TypeInterner::TOP);
                                interner.subtype(arg_ty, constraint_ty)
                            }),
                        };

                        if types_ok {
                            // Extract args and replace Call with Intrinsic
                            let intrinsic_args: Vec<Hir> =
                                if let HirKind::Call { args, .. } = &hir.kind {
                                    args.iter().map(|a| a.expr.clone()).collect()
                                } else {
                                    unreachable!()
                                };
                            let op = rule.op;
                            hir.kind = HirKind::Intrinsic {
                                op,
                                args: intrinsic_args,
                            };
                            hir.signal = Signal::silent();
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    // Recurse into children (must use mutable access)
    changed |= rewrite_children(
        hir,
        interner,
        arena,
        rewrite_table,
        symbol_names,
        binding_types,
        hir_types,
    );

    changed
}

/// Recursively rewrite children of a HIR node.
fn rewrite_children(
    hir: &mut Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    rewrite_table: &HashMap<&str, RewriteRule>,
    symbol_names: &HashMap<u32, String>,
    binding_types: &HashMap<Binding, TyId>,
    hir_types: &HashMap<HirId, TyId>,
) -> bool {
    let mut changed = false;
    hir.for_each_child_mut(|child| {
        changed |= rewrite_calls(
            child,
            interner,
            arena,
            rewrite_table,
            symbol_names,
            binding_types,
            hir_types,
        );
    });
    changed
}

#[cfg(test)]
mod tests;
